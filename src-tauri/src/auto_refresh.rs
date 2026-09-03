use std::sync::Arc;
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;
use std::time::Duration;

use rand::RngExt;
use tauri::{Emitter, Manager};

use crate::app_state::{
    SharedState, account_update_gate, lock_auto_refresh, lock_data, lock_last_refresh_result,
};
use crate::codex::reconcile_codex_auth_transactionally;
use crate::errors::{AppError, AppResult};
use crate::models::{
    Account, AccountProvider, AppData, AppSettings, AutoRefreshStatus, RefreshRunSummary,
    TokenHealthStatus, now_ts,
};
use crate::refresh_service::{RefreshService, account_belongs_to_active_codex, refresh_gate_key};
use crate::storage::commit_state_data;

const MAX_PARALLEL_REFRESHES: usize = 5;
const MAX_BACKOFF_SECONDS: i64 = 6 * 60 * 60;
const MAX_SCHEDULER_SLEEP_SECONDS: u64 = 24 * 60 * 60;
const ACTIVE_MIN_REFRESH_SECONDS: i64 = 15 * 60;
const INACTIVE_MIN_REFRESH_SECONDS: i64 = 60 * 60;
const LOW_QUOTA_REFRESH_ACTIVE_SECONDS: i64 = 5 * 60;
const LOW_QUOTA_REFRESH_INACTIVE_SECONDS: i64 = 15 * 60;
const RESET_SOON_SECONDS: i64 = 30 * 60;
const EXHAUSTED_THRESHOLD_PERCENT: f64 = 99.5;
const LOW_REMAINING_THRESHOLD_PERCENT: f64 = 85.0;

enum AutoRefreshThreadEvent {
    Stop,
    Wake,
}

pub struct AutoRefreshRuntime {
    pub status: AutoRefreshStatus,
    pub chatgpt_geo_blocked: bool,
    pub reachability_cache: crate::geo::ReachabilityCache,
    stop_tx: Option<Sender<AutoRefreshThreadEvent>>,
    running: bool,
    generation: u64,
    in_flight_runs: u32,
}

type RefreshGate = Arc<tauri::async_runtime::Mutex<()>>;

#[derive(Debug, Clone)]
pub struct RefreshAllResult {
    pub state: AppData,
    pub warnings: Vec<String>,
    pub failed_account_ids: Vec<String>,
}

struct RefreshWorkItem {
    account_id: String,
    gate: RefreshGate,
}

#[derive(Debug, Clone, Default)]
struct RefreshRunOutcome {
    succeeded: u32,
    failed: u32,
    failed_account_ids: Vec<String>,
    warnings: Vec<String>,
}

pub(crate) fn is_account_active(
    account: &Account,
    active_codex_account: Option<&Account>,
    active_gemini_account_id: Option<&str>,
) -> bool {
    match account.provider {
        AccountProvider::Codex => active_codex_account
            .is_some_and(|active| account_belongs_to_active_codex(account, active)),
        AccountProvider::Gemini => active_gemini_account_id == Some(account.id.as_str()),
    }
}

pub(crate) fn is_account_active_in_data(account: &Account, data: &AppData) -> bool {
    let active_codex = data
        .active_account_id
        .as_ref()
        .and_then(|active_id| data.accounts.iter().find(|a| &a.id == active_id));
    is_account_active(
        account,
        active_codex,
        data.active_gemini_account_id.as_deref(),
    )
}

pub async fn get_chatgpt_reachability(
    state: &SharedState,
    force_fresh: bool,
) -> crate::geo::ChatGptReachability {
    if !force_fresh
        && let Ok(runtime) = state.auto_refresh.lock()
        && let Some(cached) = runtime.reachability_cache.get_cached(60)
    {
        return cached;
    }

    let reachability = crate::geo::probe_chatgpt_reachability(&state.http_client).await;
    if let Ok(mut runtime) = state.auto_refresh.lock() {
        runtime.reachability_cache.set(reachability.clone());
    }
    reachability
}

pub(crate) fn account_gate_for_id(
    state: &Arc<SharedState>,
    account_id: &str,
) -> AppResult<RefreshGate> {
    let data = lock_data(state)?;
    let active_account = data.active_account_id.as_ref().and_then(|active_id| {
        data.accounts
            .iter()
            .find(|account| &account.id == active_id)
    });
    let account = data
        .accounts
        .iter()
        .find(|account| account.id == account_id)
        .ok_or_else(|| AppError::msg("Account not found"))?;
    account_update_gate(state, refresh_gate_key(account, active_account))
}

impl AutoRefreshRuntime {
    pub fn new(settings: &AppSettings) -> Self {
        Self {
            status: AutoRefreshStatus::from_settings(&settings.clone().normalized()),
            chatgpt_geo_blocked: false,
            reachability_cache: crate::geo::ReachabilityCache::default(),
            stop_tx: None,
            running: false,
            generation: 0,
            in_flight_runs: 0,
        }
    }
}

pub fn snapshot(state: &Arc<SharedState>) -> AppResult<AutoRefreshStatus> {
    let chatgpt_geo_blocked = lock_auto_refresh(state)
        .map(|r| r.chatgpt_geo_blocked)
        .unwrap_or(false);
    let (settings, scheduled_accounts, backed_off_accounts, next_run_at) = {
        let data = lock_data(state)?;
        let settings = data.app_settings.clone().normalized();
        let scheduled_accounts = data
            .accounts
            .iter()
            .filter(|account| {
                account.token_health.status != TokenHealthStatus::NeedsRelogin
                    && !(account.provider == AccountProvider::Codex && chatgpt_geo_blocked)
            })
            .count() as u32;
        let backed_off_accounts = data
            .accounts
            .iter()
            .filter(|account| {
                account.token_health.status != TokenHealthStatus::NeedsRelogin
                    && !(account.provider == AccountProvider::Codex && chatgpt_geo_blocked)
                    && account.quota_refresh_failures > 0
            })
            .count() as u32;
        let next_run_at = next_scheduled_run_for_data(&data, &settings, chatgpt_geo_blocked);
        (
            settings,
            scheduled_accounts,
            backed_off_accounts,
            next_run_at,
        )
    };
    let mut runtime = lock_auto_refresh(state)?;

    runtime.status.enabled = settings.auto_refresh_enabled;
    runtime.status.scheduled_accounts = scheduled_accounts;
    runtime.status.backed_off_accounts = backed_off_accounts;
    runtime.status.next_run_at = next_run_at;

    Ok(runtime.status.clone())
}

pub fn start(state: &Arc<SharedState>) -> AppResult<AutoRefreshStatus> {
    let chatgpt_geo_blocked = lock_auto_refresh(state)
        .map(|r| r.chatgpt_geo_blocked)
        .unwrap_or(false);
    let (settings, scheduled_next_run) = {
        let data = lock_data(state)?;
        let settings = data.app_settings.clone().normalized();
        let next_run = next_scheduled_run_for_data(&data, &settings, chatgpt_geo_blocked);
        (settings, next_run)
    };

    if !settings.auto_refresh_enabled {
        return stop(state);
    }

    {
        let mut runtime = lock_auto_refresh(state)?;
        runtime.status.enabled = true;
        runtime.status.next_run_at = scheduled_next_run;

        if runtime.running {
            return Ok(runtime.status.clone());
        }

        let (tx, rx) = mpsc::channel();
        runtime.stop_tx = Some(tx);
        runtime.running = true;
        runtime.generation = runtime.generation.saturating_add(1);
        let generation = runtime.generation;

        let shared = Arc::clone(state);
        thread::spawn(move || auto_refresh_thread(shared, rx, generation));
    }

    snapshot(state)
}

pub fn stop(state: &Arc<SharedState>) -> AppResult<AutoRefreshStatus> {
    let mut runtime = lock_auto_refresh(state)?;

    if let Some(tx) = runtime.stop_tx.take() {
        let _ = tx.send(AutoRefreshThreadEvent::Stop);
    }

    runtime.running = false;
    runtime.generation = runtime.generation.saturating_add(1);
    runtime.status.enabled = false;
    runtime.status.next_run_at = None;
    Ok(runtime.status.clone())
}

pub fn restart(state: &Arc<SharedState>) -> AppResult<AutoRefreshStatus> {
    let settings = {
        let data = lock_data(state)?;
        data.app_settings.clone().normalized()
    };

    let _ = stop(state)?;
    if settings.auto_refresh_enabled {
        start(state)
    } else {
        snapshot(state)
    }
}

pub fn request_refresh_now(state: &Arc<SharedState>) {
    let shared = Arc::clone(state);
    tauri::async_runtime::spawn(async move {
        let _ = refresh_accounts_for_provider_with_notification(&shared, None, true).await;
    });
}

pub fn notify_schedule_changed(state: &Arc<SharedState>) {
    if let Ok(runtime) = lock_auto_refresh(state)
        && let Some(sender) = runtime.stop_tx.as_ref()
    {
        let _ = sender.send(AutoRefreshThreadEvent::Wake);
    }
}

fn emit_status_changed(state: &Arc<SharedState>) {
    let Some(app) = state.app_handle.get() else {
        return;
    };
    if let Ok(status) = snapshot(state) {
        let _ = app.emit(
            "auto-refresh-status-changed",
            crate::dto::AutoRefreshStatusDto::from(&status),
        );
    }
}

pub fn request_account_refresh_if_stale(state: &Arc<SharedState>, account_id: String) {
    let shared = Arc::clone(state);
    tauri::async_runtime::spawn(async move {
        let result = async {
            let gate = account_gate_for_id(&shared, &account_id)?;
            let _guard = gate.lock().await;
            let (should_refresh, is_codex, skip_unsupported) = {
                let data = lock_data(&shared)?;
                let settings = data.app_settings.clone().normalized();
                let account = data
                    .accounts
                    .iter()
                    .find(|account| account.id == account_id)
                    .ok_or_else(|| AppError::msg("Account not found"))?;
                let is_active = is_account_active_in_data(account, &data);
                let should = account.token_health.status != TokenHealthStatus::NeedsRelogin
                    && effective_next_attempt_at(account, &settings, is_active, now_ts())
                        <= now_ts();
                (
                    should,
                    account.provider == AccountProvider::Codex,
                    settings.skip_unsupported_region_refresh,
                )
            };
            if !should_refresh {
                return Ok(());
            }
            if is_codex && skip_unsupported {
                let reachability = get_chatgpt_reachability(&shared, false).await;
                if !reachability.is_available() {
                    log::info!(
                        "Skipping stale refresh for Codex account {account_id}: {}",
                        reachability.user_summary()
                    );
                    let now = now_ts();
                    let next = {
                        let data = lock_data(&shared)?;
                        let mut next = data.clone();
                        if let Some(acc) = next.accounts.iter_mut().find(|a| a.id == account_id) {
                            acc.quota_next_refresh_at = Some(now.saturating_add(300));
                        }
                        next
                    };
                    commit_state_data(&shared, next)?;
                    return Ok(());
                }
            }

            let outcome = refresh_single_account(&shared, &account_id).await?;
            update_account_schedule(&shared, &account_id, outcome.succeeded)?;
            for warning in outcome.warnings {
                log::warn!("Refresh for {account_id} completed with warning: {warning}");
            }
            crate::tray_dashboard::emit_state_changed(&shared, "account", vec![account_id.clone()]);
            crate::tray_dashboard::refresh_dashboard_and_alerts(&shared);
            Ok::<(), AppError>(())
        }
        .await;
        if let Err(error) = result {
            log::warn!(
                "Failed to refresh newly active account {account_id}: {}",
                error.user_message()
            );
        }
    });
}

pub async fn refresh_accounts_for_provider(
    state: &Arc<SharedState>,
    provider: Option<AccountProvider>,
) -> AppResult<RefreshAllResult> {
    refresh_accounts_for_provider_with_notification(state, provider, false).await
}

async fn refresh_accounts_for_provider_with_notification(
    state: &Arc<SharedState>,
    provider: Option<AccountProvider>,
    notify_ui: bool,
) -> AppResult<RefreshAllResult> {
    match state.refresh_all_gate.try_lock() {
        Ok(_guard) => run_refresh_accounts_for_provider(state, provider, true, notify_ui).await,
        Err(_) => wait_for_active_refresh(state).await,
    }
}

async fn wait_for_active_refresh(state: &Arc<SharedState>) -> AppResult<RefreshAllResult> {
    let shared = Arc::clone(state);
    tauri::async_runtime::spawn(async move {
        let _guard = shared.refresh_all_gate.lock().await;
        let result = shared
            .last_refresh_result
            .lock()
            .map_err(|_| AppError::msg("State lock poisoned (last refresh result)"))?;
        result
            .clone()
            .ok_or_else(|| AppError::msg("Refresh did not complete"))
    })
    .await
    .map_err(|error| AppError::msg(format!("Refresh wait task failed: {error}")))?
}

async fn refresh_all_accounts_if_idle(
    state: &Arc<SharedState>,
) -> AppResult<Option<RefreshAllResult>> {
    let Ok(_guard) = state.refresh_all_gate.try_lock() else {
        return Ok(None);
    };
    run_refresh_accounts_for_provider(state, None, false, true)
        .await
        .map(Some)
}

async fn run_refresh_accounts_for_provider(
    state: &Arc<SharedState>,
    provider: Option<AccountProvider>,
    force: bool,
    notify_ui: bool,
) -> AppResult<RefreshAllResult> {
    let (mut work_items, mut reconcile_warnings) = refresh_work_items(state, force, provider)?;
    let (skip_unsupported, should_probe_codex) = {
        let data = lock_data(state)?;
        let skip = data.app_settings.skip_unsupported_region_refresh;
        let is_codex_target = provider.is_none_or(|p| p == AccountProvider::Codex);
        let has_any_codex = data
            .accounts
            .iter()
            .any(|acc| acc.provider == AccountProvider::Codex);
        let has_codex_in_work = work_items.iter().any(|item| {
            data.accounts
                .iter()
                .find(|acc| acc.id == item.account_id)
                .is_some_and(|acc| acc.provider == AccountProvider::Codex)
        });
        let was_geo_blocked = lock_auto_refresh(state)
            .map(|rt| rt.chatgpt_geo_blocked)
            .unwrap_or(false);
        (
            skip,
            is_codex_target && has_any_codex && (has_codex_in_work || was_geo_blocked || force),
        )
    };

    if skip_unsupported && should_probe_codex {
        let reachability = get_chatgpt_reachability(state, force).await;
        if !reachability.is_available() {
            if let Ok(mut runtime) = lock_auto_refresh(state) {
                runtime.chatgpt_geo_blocked = true;
            }
            let (codex_ids, next) = {
                let now = now_ts();
                let data = lock_data(state)?;
                let mut next = data.clone();
                let settings = next.app_settings.clone().normalized();
                let active_codex = next.active_account_id.as_ref().and_then(|active_id| {
                    next.accounts.iter().find(|a| &a.id == active_id).cloned()
                });
                let active_gemini_id = next.active_gemini_account_id.clone();

                let mut codex_ids = std::collections::HashSet::new();
                for acc in &mut next.accounts {
                    if acc.provider == AccountProvider::Codex {
                        codex_ids.insert(acc.id.clone());
                        let is_active = is_account_active(
                            acc,
                            active_codex.as_ref(),
                            active_gemini_id.as_deref(),
                        );
                        let delay = base_refresh_seconds(&settings, is_active).max(300);
                        acc.quota_next_refresh_at = Some(now.saturating_add(jittered_delay(delay)));
                    }
                }
                (codex_ids, next)
            };
            commit_state_data(state, next)?;

            work_items.retain(|item| !codex_ids.contains(&item.account_id));
            let warning_msg = reachability.user_summary();
            log::info!("Skipping ChatGPT refresh due to region/reachability: {warning_msg}.");
            reconcile_warnings.push(format!("ChatGPT refresh skipped: {warning_msg}"));
        } else {
            let was_geo_blocked = if let Ok(mut runtime) = lock_auto_refresh(state) {
                let prev = runtime.chatgpt_geo_blocked;
                runtime.chatgpt_geo_blocked = false;
                prev
            } else {
                false
            };
            if was_geo_blocked
                && let Ok((reloaded_items, _)) = refresh_work_items(state, false, provider)
            {
                work_items = reloaded_items;
            }
        }
    }

    if work_items.is_empty() {
        mark_refresh_started(state);
        let outcome = RefreshRunOutcome {
            warnings: reconcile_warnings.clone(),
            ..RefreshRunOutcome::default()
        };
        mark_refresh_finished(state, None, Some(outcome))?;
        *lock_last_refresh_result(state)? = Some(RefreshAllResult {
            state: lock_data(state)?.clone(),
            warnings: reconcile_warnings.clone(),
            failed_account_ids: Vec::new(),
        });
        return Ok(RefreshAllResult {
            state: lock_data(state)?.clone(),
            warnings: reconcile_warnings,
            failed_account_ids: Vec::new(),
        });
    }
    let changed_account_ids = work_items
        .iter()
        .map(|item| item.account_id.clone())
        .collect::<Vec<_>>();
    mark_refresh_started(state);
    let mut outcome = refresh_all_accounts_inner(state, work_items).await;
    outcome.warnings.extend(reconcile_warnings);
    let warnings = outcome.warnings.clone();
    let failed_account_ids = outcome.failed_account_ids.clone();
    mark_refresh_finished(state, None, Some(outcome))?;
    *lock_last_refresh_result(state)? = Some(RefreshAllResult {
        state: lock_data(state)?.clone(),
        warnings: warnings.clone(),
        failed_account_ids: failed_account_ids.clone(),
    });
    if notify_ui {
        crate::tray_dashboard::emit_state_changed(state, "accounts", changed_account_ids);
    }
    crate::tray_dashboard::refresh_dashboard_and_alerts(state);
    Ok(RefreshAllResult {
        state: lock_data(state)?.clone(),
        warnings,
        failed_account_ids,
    })
}

async fn refresh_all_accounts_inner(
    state: &Arc<SharedState>,
    work_items: Vec<RefreshWorkItem>,
) -> RefreshRunOutcome {
    use tokio::sync::Semaphore;

    let requested_at = now_ts();
    let semaphore = Arc::new(Semaphore::new(MAX_PARALLEL_REFRESHES));
    let mut handles = Vec::with_capacity(work_items.len());

    for work_item in work_items {
        let shared = Arc::clone(state);
        let account_id = work_item.account_id.clone();
        let gate = Arc::clone(&work_item.gate);
        let permit_sem = Arc::clone(&semaphore);

        handles.push(tauri::async_runtime::spawn(async move {
            let _permit = permit_sem.acquire().await;
            let result = async {
                let _guard = gate.lock().await;
                if quota_was_refreshed_since(&shared, &account_id, requested_at)? {
                    return Ok(crate::refresh_service::AccountRefreshOutcome {
                        succeeded: true,
                        warnings: Vec::new(),
                    });
                }
                match refresh_single_account(&shared, &account_id).await {
                    Ok(refresh) => {
                        if let Ok(data) = lock_data(&shared)
                            && let Some(app) = shared.app_handle.get()
                            && let Some(acc) = data.accounts.iter().find(|a| a.id == account_id)
                        {
                            let should_emit = app
                                .get_webview_window("main")
                                .map(|w| {
                                    w.is_visible().unwrap_or(false)
                                        && !w.is_minimized().unwrap_or(false)
                                })
                                .unwrap_or(false);
                            if should_emit {
                                let _ =
                                    app.emit("account-updated", crate::dto::AccountDto::from(acc));
                            }
                        }
                        Ok(refresh)
                    }
                    Err(error) => Err(error),
                }
            }
            .await;
            (account_id, result)
        }));
    }

    let mut outcome = RefreshRunOutcome::default();
    let mut schedule_updates = Vec::with_capacity(handles.len());

    for handle in handles {
        match handle.await {
            Ok((account_id, Ok(refresh))) => {
                if refresh.succeeded {
                    outcome.succeeded = outcome.succeeded.saturating_add(1);
                    schedule_updates.push((account_id, true, None));
                } else {
                    outcome.failed = outcome.failed.saturating_add(1);
                    schedule_updates.push((account_id.clone(), false, None));
                    outcome.failed_account_ids.push(account_id);
                }
                outcome.warnings.extend(refresh.warnings);
            }
            Ok((account_id, Err(error))) => {
                outcome.failed = outcome.failed.saturating_add(1);
                let retry_after = error.retry_after_seconds();
                schedule_updates.push((account_id.clone(), false, retry_after));
                outcome.failed_account_ids.push(account_id);
            }
            Err(_) => {
                outcome.failed = outcome.failed.saturating_add(1);
                outcome
                    .failed_account_ids
                    .push("unknown-worker".to_string());
            }
        }
    }

    if let Err(error) = batch_update_account_schedules(state, &schedule_updates) {
        log::warn!(
            "Failed to persist batch account schedules: {}",
            error.user_message()
        );
    }

    outcome
}

fn refresh_work_items(
    state: &Arc<SharedState>,
    force: bool,
    provider: Option<AccountProvider>,
) -> AppResult<(Vec<RefreshWorkItem>, Vec<String>)> {
    let mut data = lock_data(state)?;
    let warnings = reconcile_codex_auth_transactionally(&mut data)
        .err()
        .map(|error| {
            let message = format!(
                "auth.json reconciliation was skipped before refresh-all: {}",
                error.user_message()
            );
            log::warn!("{message}");
            message
        })
        .into_iter()
        .collect::<Vec<_>>();
    let active_account = data.active_account_id.as_ref().and_then(|active_id| {
        data.accounts
            .iter()
            .find(|account| &account.id == active_id)
            .cloned()
    });
    let now = now_ts();
    let settings = data.app_settings.clone().normalized();
    let chatgpt_geo_blocked = lock_auto_refresh(state)
        .map(|r| r.chatgpt_geo_blocked)
        .unwrap_or(false);

    let work_items = data
        .accounts
        .iter()
        .filter(|account| {
            if account.token_health.status == TokenHealthStatus::NeedsRelogin {
                return false;
            }
            if let Some(target_provider) = provider
                && account.provider != target_provider
            {
                return false;
            }
            if !force && account.provider == AccountProvider::Codex && chatgpt_geo_blocked {
                return false;
            }
            let is_active = is_account_active(
                account,
                active_account.as_ref(),
                data.active_gemini_account_id.as_deref(),
            );
            force
                || (account.token_health.status != TokenHealthStatus::NeedsRelogin
                    && effective_next_attempt_at(account, &settings, is_active, now) <= now)
        })
        .map(|account| {
            let key = refresh_gate_key(account, active_account.as_ref());
            account_update_gate(state, key).map(|gate| RefreshWorkItem {
                account_id: account.id.clone(),
                gate,
            })
        })
        .collect::<AppResult<Vec<_>>>()?;
    Ok((work_items, warnings))
}

fn quota_was_refreshed_since(
    state: &Arc<SharedState>,
    account_id: &str,
    requested_at: i64,
) -> AppResult<bool> {
    let data = lock_data(state)?;
    Ok(data
        .accounts
        .iter()
        .find(|account| account.id == account_id)
        .and_then(|account| account.quota.as_ref())
        .is_some_and(|quota| quota.fetched_at >= requested_at))
}

async fn refresh_single_account(
    state: &Arc<SharedState>,
    account_id: &str,
) -> AppResult<crate::refresh_service::AccountRefreshOutcome> {
    let provider = lock_data(state)?
        .accounts
        .iter()
        .find(|account| account.id == account_id)
        .map(|account| account.provider)
        .ok_or_else(|| AppError::msg("Account not found"))?;
    match provider {
        AccountProvider::Gemini => {
            crate::gemini_quota::refresh_gemini_account(state, account_id).await
        }
        AccountProvider::Codex => {
            RefreshService::new(Arc::clone(state))
                .refresh_single_account(account_id)
                .await
        }
    }
}

fn jittered_delay(seconds: i64) -> i64 {
    let spread = (seconds / 10).max(1);
    let mut rng = rand::rng();
    seconds
        .saturating_add(rng.random_range(-spread..=spread))
        .max(30)
}

fn base_refresh_seconds(settings: &AppSettings, is_active: bool) -> i64 {
    let active_seconds =
        (settings.auto_refresh_interval_minutes as i64 * 60).max(ACTIVE_MIN_REFRESH_SECONDS);
    if is_active {
        active_seconds
    } else {
        active_seconds
            .saturating_mul(4)
            .max(INACTIVE_MIN_REFRESH_SECONDS)
    }
}

fn most_constrained_window(account: &Account) -> Option<(f64, Option<i64>)> {
    let quota = account.quota.as_ref()?;
    [&quota.primary, &quota.secondary]
        .into_iter()
        .filter_map(|window| {
            window
                .used_percent
                .map(|used_percent| (used_percent, window.reset_at))
        })
        .max_by(|left, right| left.0.total_cmp(&right.0))
}

pub(crate) fn next_after_success(
    account: &Account,
    settings: &AppSettings,
    is_active: bool,
    anchor: i64,
    now: i64,
    jitter: bool,
) -> i64 {
    if let Some((used_percent, reset_at)) = most_constrained_window(account) {
        if used_percent >= EXHAUSTED_THRESHOLD_PERCENT
            && let Some(reset_at) = reset_at
            && reset_at > now
        {
            return reset_at.saturating_add(60);
        }

        let reset_is_soon = reset_at.is_some_and(|reset_at| {
            reset_at > now && reset_at.saturating_sub(now) <= RESET_SOON_SECONDS
        });
        if used_percent >= LOW_REMAINING_THRESHOLD_PERCENT || reset_is_soon {
            let seconds = if is_active {
                LOW_QUOTA_REFRESH_ACTIVE_SECONDS
            } else {
                LOW_QUOTA_REFRESH_INACTIVE_SECONDS
            };
            return anchor.saturating_add(if jitter {
                jittered_delay(seconds)
            } else {
                seconds
            });
        }
    }

    let seconds = base_refresh_seconds(settings, is_active);
    anchor.saturating_add(if jitter {
        jittered_delay(seconds)
    } else {
        seconds
    })
}

fn effective_next_attempt_at(
    account: &Account,
    settings: &AppSettings,
    is_active: bool,
    now: i64,
) -> i64 {
    // If ANY quota reset window has already passed and the quota hasn't been fetched since that reset,
    // trigger immediate refresh!
    if let Some(quota) = &account.quota {
        let fetched_at = quota.fetched_at;
        for window in [&quota.primary, &quota.secondary] {
            if let Some(reset_at) = window.reset_at
                && reset_at <= now
                && fetched_at < reset_at
            {
                return now;
            }
        }
    }

    if let Some(persisted) = account.quota_next_refresh_at.filter(|value| *value > 0) {
        return persisted;
    }
    if account.quota_refresh_failures > 0 {
        return now;
    }

    account.quota.as_ref().map_or(now, |quota| {
        next_after_success(account, settings, is_active, quota.fetched_at, now, false)
    })
}

pub(crate) fn sync_schedule_runtime_status(state: &Arc<SharedState>) -> AppResult<()> {
    let chatgpt_geo_blocked = lock_auto_refresh(state)
        .map(|r| r.chatgpt_geo_blocked)
        .unwrap_or(false);
    let (backed_off_accounts, next_run_at) = {
        let data = lock_data(state)?;
        let settings = data.app_settings.clone().normalized();
        let backed_off_accounts = data
            .accounts
            .iter()
            .filter(|account| {
                account.token_health.status != TokenHealthStatus::NeedsRelogin
                    && !(account.provider == AccountProvider::Codex && chatgpt_geo_blocked)
                    && account.quota_refresh_failures > 0
            })
            .count() as u32;
        let next_run_at = next_scheduled_run_for_data(&data, &settings, chatgpt_geo_blocked);
        (backed_off_accounts, next_run_at)
    };

    let mut runtime = lock_auto_refresh(state)?;
    runtime.status.backed_off_accounts = backed_off_accounts;
    runtime.status.next_run_at = next_run_at;
    drop(runtime);
    notify_schedule_changed(state);
    emit_status_changed(state);
    Ok(())
}

pub(crate) fn update_account_schedule(
    state: &Arc<SharedState>,
    account_id: &str,
    success: bool,
) -> AppResult<()> {
    update_account_schedule_with_retry(state, account_id, success, None)
}

pub(crate) fn update_account_schedule_with_retry(
    state: &Arc<SharedState>,
    account_id: &str,
    success: bool,
    retry_after_seconds: Option<i64>,
) -> AppResult<()> {
    if success {
        return sync_schedule_runtime_status(state);
    }
    let now = now_ts();
    let chatgpt_geo_blocked = lock_auto_refresh(state)
        .map(|r| r.chatgpt_geo_blocked)
        .unwrap_or(false);
    let (backed_off_accounts, next_run_at, next) = {
        let data = lock_data(state)?;
        let mut next = data.clone();
        let settings = next.app_settings.clone().normalized();
        let active_codex = next
            .active_account_id
            .as_ref()
            .and_then(|active_id| next.accounts.iter().find(|a| &a.id == active_id).cloned());
        let active_gemini_id = next.active_gemini_account_id.clone();
        let account = next
            .accounts
            .iter_mut()
            .find(|account| account.id == account_id)
            .ok_or_else(|| AppError::msg("Account disappeared while scheduling refresh"))?;
        let is_active =
            is_account_active(account, active_codex.as_ref(), active_gemini_id.as_deref());

        account.quota_refresh_failures = account.quota_refresh_failures.saturating_add(1);
        if let Some(retry_after) = retry_after_seconds {
            account.quota_next_refresh_at = Some(now.saturating_add(retry_after.max(5)));
        } else {
            let multiplier = 1_i64 << account.quota_refresh_failures.min(6);
            let delay = base_refresh_seconds(&settings, is_active)
                .saturating_mul(multiplier)
                .min(MAX_BACKOFF_SECONDS);
            account.quota_next_refresh_at = Some(now.saturating_add(jittered_delay(delay)));
        }

        let backed_off_accounts = next
            .accounts
            .iter()
            .filter(|account| {
                account.token_health.status != TokenHealthStatus::NeedsRelogin
                    && !(account.provider == AccountProvider::Codex && chatgpt_geo_blocked)
                    && account.quota_refresh_failures > 0
            })
            .count() as u32;
        let next_run_at = next_scheduled_run_for_data(&next, &settings, chatgpt_geo_blocked);
        (backed_off_accounts, next_run_at, next)
    };

    commit_state_data(state, next)?;

    let mut runtime = lock_auto_refresh(state)?;
    runtime.status.backed_off_accounts = backed_off_accounts;
    runtime.status.next_run_at = next_run_at;
    drop(runtime);
    notify_schedule_changed(state);
    emit_status_changed(state);
    Ok(())
}

fn batch_update_account_schedules(
    state: &Arc<SharedState>,
    updates: &[(String, bool, Option<i64>)],
) -> AppResult<()> {
    if updates.is_empty() {
        return Ok(());
    }

    let now = now_ts();
    let chatgpt_geo_blocked = lock_auto_refresh(state)
        .map(|runtime| runtime.chatgpt_geo_blocked)
        .unwrap_or(false);

    let (backed_off_accounts, next_run_at, next) = {
        let data = lock_data(state)?;
        let mut next = data.clone();
        let settings = next.app_settings.clone().normalized();
        let active_codex = next
            .active_account_id
            .as_ref()
            .and_then(|active_id| next.accounts.iter().find(|a| &a.id == active_id).cloned());
        let active_gemini_id = next.active_gemini_account_id.clone();

        for (account_id, succeeded, retry_after) in updates {
            let Some(account) = next.accounts.iter_mut().find(|a| &a.id == account_id) else {
                continue;
            };
            let is_active =
                is_account_active(account, active_codex.as_ref(), active_gemini_id.as_deref());
            if *succeeded {
                account.quota_refresh_failures = 0;
                let delay = base_refresh_seconds(&settings, is_active);
                account.quota_next_refresh_at = Some(now.saturating_add(jittered_delay(delay)));
            } else {
                account.quota_refresh_failures = account.quota_refresh_failures.saturating_add(1);
                if let Some(retry_after) = retry_after {
                    account.quota_next_refresh_at = Some(now.saturating_add((*retry_after).max(5)));
                } else {
                    let multiplier = 1_i64 << account.quota_refresh_failures.min(6);
                    let delay = base_refresh_seconds(&settings, is_active)
                        .saturating_mul(multiplier)
                        .min(MAX_BACKOFF_SECONDS);
                    account.quota_next_refresh_at = Some(now.saturating_add(jittered_delay(delay)));
                }
            }
        }

        let backed_off_accounts = next
            .accounts
            .iter()
            .filter(|account| {
                account.token_health.status != TokenHealthStatus::NeedsRelogin
                    && !(account.provider == AccountProvider::Codex && chatgpt_geo_blocked)
                    && account.quota_refresh_failures > 0
            })
            .count() as u32;
        let next_run_at = next_scheduled_run_for_data(&next, &settings, chatgpt_geo_blocked);
        (backed_off_accounts, next_run_at, next)
    };

    commit_state_data(state, next)?;

    let mut runtime = lock_auto_refresh(state)?;
    runtime.status.backed_off_accounts = backed_off_accounts;
    runtime.status.next_run_at = next_run_at;
    drop(runtime);
    notify_schedule_changed(state);
    emit_status_changed(state);
    Ok(())
}

fn next_scheduled_run_for_data(
    data: &AppData,
    settings: &AppSettings,
    chatgpt_geo_blocked: bool,
) -> Option<i64> {
    if !settings.auto_refresh_enabled {
        return None;
    }

    let now = now_ts();
    data.accounts
        .iter()
        .filter(|account| {
            account.token_health.status != TokenHealthStatus::NeedsRelogin
                && !(account.provider == AccountProvider::Codex && chatgpt_geo_blocked)
        })
        .map(|account| {
            let is_active = is_account_active_in_data(account, data);
            effective_next_attempt_at(account, settings, is_active, now)
        })
        .min()
}

fn auto_refresh_thread(
    shared: Arc<SharedState>,
    rx: Receiver<AutoRefreshThreadEvent>,
    generation: u64,
) {
    loop {
        let (settings, next_run_at) = match lock_data(&shared) {
            Ok(data) => {
                let settings = data.app_settings.clone().normalized();
                let chatgpt_geo_blocked = lock_auto_refresh(&shared)
                    .map(|r| r.chatgpt_geo_blocked)
                    .unwrap_or(false);
                let next_run_at =
                    next_scheduled_run_for_data(&data, &settings, chatgpt_geo_blocked);
                (settings, next_run_at)
            }
            Err(err) => {
                if let Err(mark_error) =
                    mark_refresh_finished(&shared, Some(err.user_message()), None)
                {
                    log::warn!(
                        "Failed to record auto-refresh terminal error: {}",
                        mark_error.user_message()
                    );
                }
                break;
            }
        };

        if !settings.auto_refresh_enabled {
            break;
        }

        if let Ok(mut runtime) = lock_auto_refresh(&shared) {
            runtime.status.enabled = true;
            runtime.status.next_run_at = next_run_at;
        }

        let wait_seconds = scheduler_wait_seconds(next_run_at, now_ts());
        let wait = Duration::from_secs(wait_seconds);
        match rx.recv_timeout(wait) {
            Ok(AutoRefreshThreadEvent::Stop) | Err(mpsc::RecvTimeoutError::Disconnected) => break,
            Ok(AutoRefreshThreadEvent::Wake) => continue,
            Err(mpsc::RecvTimeoutError::Timeout) => {
                let refresh_state = Arc::clone(&shared);
                tauri::async_runtime::spawn(async move {
                    let _ = refresh_all_accounts_if_idle(&refresh_state).await;
                });
            }
        }
    }

    let enabled = lock_data(&shared)
        .map(|data| data.app_settings.auto_refresh_enabled)
        .unwrap_or(false);
    if let Ok(mut runtime) = lock_auto_refresh(&shared)
        && runtime.generation == generation
    {
        runtime.running = false;
        runtime.stop_tx = None;
        runtime.status.enabled = enabled;
        runtime.status.in_flight = false;
        runtime.status.next_run_at = None;
    }
    emit_status_changed(&shared);
}

fn scheduler_wait_seconds(next_run_at: Option<i64>, now: i64) -> u64 {
    let Some(next_run_at) = next_run_at else {
        return MAX_SCHEDULER_SLEEP_SECONDS;
    };
    if next_run_at <= now {
        return 1;
    }
    ((next_run_at - now) as u64).clamp(1, MAX_SCHEDULER_SLEEP_SECONDS)
}

fn mark_refresh_started(state: &Arc<SharedState>) {
    let now = now_ts();
    if let Ok(mut runtime) = lock_auto_refresh(state) {
        runtime.in_flight_runs = runtime.in_flight_runs.saturating_add(1);
        runtime.status.in_flight = true;
        runtime.status.last_started_at = Some(now);
        runtime.status.last_error = None;
    }
    emit_status_changed(state);
}

fn mark_refresh_finished(
    state: &Arc<SharedState>,
    error: Option<String>,
    outcome: Option<RefreshRunOutcome>,
) -> AppResult<()> {
    let (settings, next_run_at) = lock_data(state).map(|data| {
        let settings = data.app_settings.clone().normalized();
        let chatgpt_geo_blocked = lock_auto_refresh(state)
            .map(|r| r.chatgpt_geo_blocked)
            .unwrap_or(false);
        let next_run_at = next_scheduled_run_for_data(&data, &settings, chatgpt_geo_blocked);
        (settings, next_run_at)
    })?;

    let mut runtime = lock_auto_refresh(state)?;
    let now = now_ts();
    runtime.in_flight_runs = runtime.in_flight_runs.saturating_sub(1);
    runtime.status.enabled = settings.auto_refresh_enabled;
    runtime.status.in_flight = runtime.in_flight_runs > 0;
    runtime.status.last_finished_at = Some(now);
    if let Some(outcome) = outcome {
        runtime.status.last_run = Some(RefreshRunSummary {
            started_at: runtime.status.last_started_at.unwrap_or(now),
            finished_at: now,
            succeeded: outcome.succeeded,
            failed: outcome.failed,
            failed_account_ids: outcome.failed_account_ids,
            warnings: outcome.warnings,
        });
    }
    if error.is_some() || runtime.in_flight_runs == 0 {
        runtime.status.last_error = error;
    }
    runtime.status.next_run_at = next_run_at;
    drop(runtime);
    emit_status_changed(state);
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use crate::app_state::{SharedState, lock_auto_refresh};
    use crate::models::{
        Account, AppData, AppSettings, QuotaInfo, QuotaWindow, TokenHealth, TokenHealthStatus,
        Tokens,
    };

    use super::{
        ACTIVE_MIN_REFRESH_SECONDS, INACTIVE_MIN_REFRESH_SECONDS, RefreshRunOutcome,
        effective_next_attempt_at, mark_refresh_finished, next_after_success, refresh_work_items,
        scheduler_wait_seconds,
    };

    fn account(used_percent: f64, fetched_at: i64, reset_at: Option<i64>) -> Account {
        Account {
            id: "account-1".to_string(),
            provider: crate::models::AccountProvider::Codex,
            email: Some("user@example.com".to_string()),
            account_id: Some("openai-1".to_string()),
            provider_project_id: None,
            subscription_expires_at: None,
            subscription_plan: Some("Plus".to_string()),
            subscription_detected_at: Some(fetched_at),
            subscription_checked_at: Some(fetched_at),
            subscription_next_check_at: None,
            subscription_endpoint_hint: None,
            tokens: Tokens::default(),
            token_expires_at: None,
            tokens_updated_at: Some(fetched_at),
            token_health: TokenHealth::healthy(),
            quota: Some(QuotaInfo {
                plan_type: Some("Plus".to_string()),
                primary: QuotaWindow {
                    used_percent: Some(used_percent),
                    limit_window_seconds: Some(7 * 24 * 60 * 60),
                    reset_at,
                    fetched_at: Some(fetched_at),
                },
                secondary: QuotaWindow::default(),
                fetched_at,
            }),
            quota_next_refresh_at: None,
            quota_refresh_failures: 0,
            created_at: fetched_at,
            last_login_at: fetched_at,
            last_error: None,
            subscription_error: None,
        }
    }

    #[test]
    fn normal_quota_uses_active_and_inactive_minimums() {
        let settings = AppSettings::default();
        let now = 1_000_000;
        let account = account(20.0, now, Some(now + 4 * 24 * 60 * 60));

        assert_eq!(
            next_after_success(&account, &settings, true, now, now, false),
            now + ACTIVE_MIN_REFRESH_SECONDS
        );
        assert_eq!(
            next_after_success(&account, &settings, false, now, now, false),
            now + INACTIVE_MIN_REFRESH_SECONDS
        );
    }

    #[test]
    fn exhausted_quota_sleeps_until_reset() {
        let settings = AppSettings::default();
        let now = 1_000_000;
        let reset_at = now + 20_000;
        let account = account(100.0, now, Some(reset_at));

        assert_eq!(
            next_after_success(&account, &settings, true, now, now, false),
            reset_at + 60
        );
    }

    #[test]
    fn persisted_backoff_wins_over_freshness_derivation() {
        let settings = AppSettings::default();
        let now = 1_000_000;
        let mut account = account(20.0, now - 10_000, None);
        account.quota_refresh_failures = 2;
        account.quota_next_refresh_at = Some(now + 5_000);

        assert_eq!(
            effective_next_attempt_at(&account, &settings, true, now),
            now + 5_000
        );
    }

    #[test]
    fn scheduler_waits_until_the_actual_due_time() {
        assert_eq!(scheduler_wait_seconds(Some(1_300), 1_000), 300);
        assert_eq!(scheduler_wait_seconds(Some(900), 1_000), 1);
        assert_eq!(
            scheduler_wait_seconds(None, 1_000),
            super::MAX_SCHEDULER_SLEEP_SECONDS
        );
    }

    #[test]
    fn finished_run_records_partial_refresh_summary() {
        let state = Arc::new(
            SharedState::new_with_startup_error(AppData::default(), None)
                .expect("create shared state"),
        );
        {
            let mut runtime = lock_auto_refresh(&state).expect("lock runtime");
            runtime.status.last_started_at = Some(1_000);
        }

        mark_refresh_finished(
            &state,
            None,
            Some(RefreshRunOutcome {
                succeeded: 2,
                failed: 1,
                failed_account_ids: vec!["account-b".to_string()],
                warnings: vec!["quota refresh failed".to_string()],
            }),
        )
        .expect("mark refresh finished");

        let runtime = lock_auto_refresh(&state).expect("lock runtime");
        let summary = runtime.status.last_run.as_ref().expect("run summary");
        assert_eq!(summary.started_at, 1_000);
        assert_eq!(summary.succeeded, 2);
        assert_eq!(summary.failed, 1);
        assert_eq!(summary.failed_account_ids, vec!["account-b".to_string()]);
        assert_eq!(summary.warnings, vec!["quota refresh failed".to_string()]);
        assert!(runtime.status.last_error.is_none());
    }

    #[test]
    fn refresh_work_items_skips_needs_relogin_accounts_even_when_forced() {
        let mut healthy_account = account(20.0, 1_000_000, None);
        healthy_account.id = "healthy-1".to_string();
        healthy_account.token_health = TokenHealth {
            status: TokenHealthStatus::Healthy,
            last_checked_at: Some(1_000_000),
            last_refreshed_at: None,
            last_error: None,
        };

        let mut expired_account = account(10.0, 1_000_000, None);
        expired_account.id = "expired-1".to_string();
        expired_account.token_health = TokenHealth {
            status: TokenHealthStatus::NeedsRelogin,
            last_checked_at: Some(1_000_000),
            last_refreshed_at: None,
            last_error: Some("Session expired".to_string()),
        };

        let data = AppData {
            accounts: vec![healthy_account, expired_account],
            ..AppData::default()
        };
        let state =
            Arc::new(SharedState::new_with_startup_error(data, None).expect("create shared state"));

        let (items, _) = refresh_work_items(&state, true, None).expect("refresh work items");
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].account_id, "healthy-1");
    }
}
