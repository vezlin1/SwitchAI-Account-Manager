use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use serde::Serialize;
use tauri::State;

use crate::app_state::{SharedState, lock_data, lock_flows, lock_startup_error};
use crate::auto_refresh;
use crate::codex::{reconcile_codex_auth, restart_codex_process};
use crate::dto::{
    AccountDto, AccountSnapshotDto, AntigravitySurfaceDto, AppDataDto, AppSettingsDto,
    AutoRefreshStatusDto, CommandWarningDto, RecoveryStatusDto, StartupStatusDto, StateResultDto,
};
use crate::errors::{AppError, AppResult, IpcErrorDto, to_command_error};
use crate::models::{Account, AccountProvider, AppData, TokenHealth, TokenHealthStatus, now_ts};
use crate::oauth::{
    build_oauth_flow, cancel_oauth_flow as cancel_oauth_flow_state, ensure_callback_server,
    flow_to_response, prune_oauth_flows,
};
use crate::refresh_service::{QuotaRefreshWarning, RefreshService, write_account_auth};
use crate::shell;
use crate::storage::{commit_app_data, persist_app_data};

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SwitchAccountRestartResponse {
    pub state: AppDataDto,
    pub restart_warning: Option<String>,
}

fn warning(
    code: &str,
    domain: &str,
    message: String,
    account_id: Option<String>,
    retryable: bool,
) -> CommandWarningDto {
    CommandWarningDto {
        code: code.to_string(),
        domain: domain.to_string(),
        message,
        account_id,
        retryable,
    }
}

fn account_for_selection(data: &AppData, account_id: Option<&str>) -> Option<Account> {
    let account_id = account_id?;
    data.accounts
        .iter()
        .find(|account| account.id == account_id)
        .cloned()
}

fn commit_active_selection_changes(
    current: &mut AppData,
    mut next: AppData,
    codex_changed: bool,
    gemini_changed: bool,
) -> AppResult<()> {
    let previous_codex = account_for_selection(current, current.active_account_id.as_deref());
    let next_codex = account_for_selection(&next, next.active_account_id.as_deref());
    let previous_antigravity = if gemini_changed {
        crate::gemini::read_antigravity_auth()?
    } else {
        None
    };
    let next_gemini = account_for_selection(&next, next.active_gemini_account_id.as_deref());
    next.revision = current.revision.saturating_add(1);

    if codex_changed {
        write_account_auth(next_codex.as_ref())?;
    }
    if gemini_changed {
        let result = match next_gemini.as_ref() {
            Some(account) => crate::gemini::write_antigravity_account_auth(account),
            None => crate::gemini::clear_antigravity_auth(),
        };
        if let Err(error) = result {
            if codex_changed
                && let Err(rollback_error) = write_account_auth(previous_codex.as_ref())
            {
                return Err(AppError::msg(format!(
                    "{}; failed to restore previous Codex auth.json: {}",
                    error.user_message(),
                    rollback_error.user_message()
                )));
            }
            return Err(error);
        }
    }

    if let Err(save_error) = persist_app_data(current, &next) {
        let mut rollback_failures = Vec::new();
        if codex_changed && let Err(error) = write_account_auth(previous_codex.as_ref()) {
            rollback_failures.push(format!("Codex auth.json: {}", error.user_message()));
        }
        if gemini_changed
            && let Err(error) =
                crate::gemini::restore_antigravity_auth(previous_antigravity.as_ref())
        {
            rollback_failures.push(format!("Antigravity credential: {}", error.user_message()));
        }
        if !rollback_failures.is_empty() {
            return Err(AppError::msg(format!(
                "{}; failed to restore previous credentials: {}",
                save_error.user_message(),
                rollback_failures.join("; ")
            )));
        }
        return Err(save_error);
    }

    *current = next;
    Ok(())
}

fn remove_account_from_data(data: &mut AppData, account_id: &str) -> AppResult<(bool, bool)> {
    let previous_len = data.accounts.len();
    let was_codex_active = data.active_account_id.as_deref() == Some(account_id);
    let was_gemini_active = data.active_gemini_account_id.as_deref() == Some(account_id);
    data.accounts.retain(|account| account.id != account_id);
    data.app_settings
        .hidden_account_ids
        .retain(|hidden_id| hidden_id != account_id);

    if data.accounts.len() == previous_len {
        return Err(AppError::msg("Account not found"));
    }

    if was_codex_active {
        data.active_account_id = data
            .accounts
            .iter()
            .find(|account| account.provider == crate::models::AccountProvider::Codex)
            .map(|account| account.id.clone());
    }
    if was_gemini_active {
        data.active_gemini_account_id = data
            .accounts
            .iter()
            .find(|account| {
                account.provider == AccountProvider::Gemini
                    && account
                        .token_expires_at
                        .is_some_and(|expiry| expiry > now_ts())
            })
            .map(|account| account.id.clone());
    }

    Ok((was_codex_active, was_gemini_active))
}

fn command_result<T>(result: AppResult<T>) -> Result<T, IpcErrorDto> {
    result.map_err(to_command_error)
}

fn startup_status(state: &Arc<SharedState>) -> AppResult<StartupStatusDto> {
    let warnings = state
        .startup_warnings
        .lock()
        .map_err(|_| AppError::msg("State lock poisoned (startup warnings)"))?
        .clone();
    if let Some(error) = lock_startup_error(state)?.clone() {
        let recovery = crate::storage::recovery_status()?;
        return Ok(StartupStatusDto {
            mode: "recovery_required".to_string(),
            state: None,
            warnings,
            recovery: Some(RecoveryStatusDto {
                error,
                data_directory: recovery.data_directory.display().to_string(),
                state_path: recovery.state_path.display().to_string(),
                backup_available: recovery.backup_available,
            }),
        });
    }

    Ok(StartupStatusDto {
        mode: "ready".to_string(),
        state: Some(AppDataDto::from(&*lock_data(state)?)),
        warnings,
        recovery: None,
    })
}

#[tauri::command]
pub fn get_app_state(state: State<'_, Arc<SharedState>>) -> Result<AppDataDto, IpcErrorDto> {
    command_result((|| {
        if let Some(error) = lock_startup_error(state.inner())?.as_ref() {
            return Err(AppError::msg(error.clone()));
        }
        let data = lock_data(state.inner())?;
        Ok(AppDataDto::from(&*data))
    })())
}

#[tauri::command]
pub fn get_startup_status(
    state: State<'_, Arc<SharedState>>,
) -> Result<StartupStatusDto, IpcErrorDto> {
    command_result(startup_status(state.inner()))
}

fn install_recovered_state(state: &Arc<SharedState>, mut recovered: AppData) -> AppResult<()> {
    let mut current = lock_data(state)?;
    recovered.revision = current.revision.saturating_add(1);
    *current = recovered;
    drop(current);
    *lock_startup_error(state)? = None;
    let _ = auto_refresh::restart(state)?;
    crate::tray_dashboard::refresh_dashboard(state);
    crate::tray_dashboard::emit_state_changed(state, "accounts", Vec::new());
    Ok(())
}

fn reconcile_recovered_auth(state: &Arc<SharedState>, recovered: &mut AppData) {
    if let Err(error) = crate::codex::reconcile_codex_auth_at_startup(recovered) {
        let message = format!(
            "Application state recovered, but auth.json could not be reconciled: {}",
            error.user_message()
        );
        log::warn!("{message}");
        if let Ok(mut warnings) = state.startup_warnings.lock() {
            warnings.push(message);
        }
    }
}

#[tauri::command]
pub fn restore_state_backup(
    state: State<'_, Arc<SharedState>>,
) -> Result<StartupStatusDto, IpcErrorDto> {
    command_result((|| {
        let mut recovered = crate::storage::restore_app_data_backup()?;
        reconcile_recovered_auth(state.inner(), &mut recovered);
        install_recovered_state(state.inner(), recovered)?;
        startup_status(state.inner())
    })())
}

#[tauri::command]
pub fn start_fresh(state: State<'_, Arc<SharedState>>) -> Result<StartupStatusDto, IpcErrorDto> {
    command_result((|| {
        let mut recovered = crate::storage::start_fresh_app_data()?;
        reconcile_recovered_auth(state.inner(), &mut recovered);
        install_recovered_state(state.inner(), recovered)?;
        startup_status(state.inner())
    })())
}

#[tauri::command]
pub fn open_recovery_data_directory() -> Result<(), IpcErrorDto> {
    command_result((|| {
        let path = crate::storage::app_storage_dir()?;
        let path = path
            .to_str()
            .ok_or_else(|| AppError::msg("Application data path is not valid Unicode"))?;
        shell::open_target(path, "Failed to open application data directory")
    })())
}

#[tauri::command]
pub fn get_account(
    account_id: String,
    state: State<'_, Arc<SharedState>>,
) -> Result<AccountSnapshotDto, IpcErrorDto> {
    command_result((|| {
        let data = lock_data(state.inner())?;
        let account = data
            .accounts
            .iter()
            .find(|account| account.id == account_id)
            .map(AccountDto::from)
            .ok_or_else(|| AppError::msg("Account not found"))?;
        Ok(AccountSnapshotDto {
            revision: data.revision,
            account,
        })
    })())
}

#[tauri::command]
pub fn get_auto_refresh_status(
    state: State<'_, Arc<SharedState>>,
) -> Result<AutoRefreshStatusDto, IpcErrorDto> {
    command_result(
        auto_refresh::snapshot(state.inner()).map(|status| AutoRefreshStatusDto::from(&status)),
    )
}

#[tauri::command]
pub fn set_app_settings(
    settings: AppSettingsDto,
    state: State<'_, Arc<SharedState>>,
) -> Result<AppDataDto, IpcErrorDto> {
    command_result((|| {
        let mut settings = crate::models::AppSettings::from(settings).normalized();
        let next = {
            let mut data = lock_data(state.inner())?;
            settings.hidden_account_ids.retain(|account_id| {
                data.accounts
                    .iter()
                    .any(|account| &account.id == account_id)
            });
            let mut next = data.clone();
            next.app_settings = settings;
            commit_app_data(&mut data, next)?;
            data.clone()
        };

        let _ = auto_refresh::restart(state.inner())?;
        crate::tray_dashboard::refresh_dashboard(state.inner());
        Ok(AppDataDto::from(&next))
    })())
}

#[tauri::command]
pub fn start_oauth_flow(
    provider: Option<String>,
    target_account_id: Option<String>,
    state: State<'_, Arc<SharedState>>,
) -> Result<crate::oauth::OauthStartResponse, IpcErrorDto> {
    command_result((|| {
        let account_provider = match provider.as_deref() {
            Some("gemini") => crate::models::AccountProvider::Gemini,
            _ => crate::models::AccountProvider::Codex,
        };
        ensure_callback_server(state.inner())?;
        prune_oauth_flows(state.inner(), now_ts());
        let login_hint = if let Some(target_account_id) = target_account_id.as_deref() {
            let data = lock_data(state.inner())?;
            let account = data
                .accounts
                .iter()
                .find(|account| account.id == target_account_id)
                .ok_or_else(|| AppError::msg("Re-login target account not found"))?;
            if account.provider != account_provider {
                return Err(AppError::msg(
                    "Re-login provider does not match the selected account",
                ));
            }
            account.email.clone()
        } else {
            None
        };

        let (flow, response) = build_oauth_flow(
            account_provider,
            target_account_id.clone(),
            login_hint.as_deref(),
        )?;
        let mut flows = lock_flows(state.inner())?;
        if target_account_id.is_some()
            && flows.values().any(|existing| {
                existing.target_account_id == target_account_id
                    && matches!(
                        existing.status,
                        crate::oauth::OauthFlowStatus::WaitingCallback
                            | crate::oauth::OauthFlowStatus::Exchanging
                    )
            })
        {
            return Err(AppError::msg(
                "Re-login is already running for this account",
            ));
        }
        flows.insert(response.flow_id.clone(), flow);
        Ok(response)
    })())
}

#[tauri::command]
pub fn get_oauth_flow_status(
    flow_id: String,
    state: State<'_, Arc<SharedState>>,
) -> Result<crate::oauth::OauthFlowResponse, IpcErrorDto> {
    command_result((|| {
        prune_oauth_flows(state.inner(), now_ts());
        let flow = {
            let flows = lock_flows(state.inner())?;
            flows
                .get(&flow_id)
                .cloned()
                .ok_or_else(|| AppError::msg("OAuth flow not found"))?
        };
        let data = lock_data(state.inner())?;
        Ok(flow_to_response(&flow, &data))
    })())
}

#[tauri::command]
pub fn cancel_oauth_flow(
    flow_id: String,
    state: State<'_, Arc<SharedState>>,
) -> Result<(), IpcErrorDto> {
    command_result(cancel_oauth_flow_state(state.inner(), &flow_id))
}

#[tauri::command]
pub fn open_external_url(url: String) -> Result<(), IpcErrorDto> {
    command_result(shell::open_external_url(&url))
}

#[tauri::command]
pub fn remove_account(
    account_id: String,
    state: State<'_, Arc<SharedState>>,
) -> Result<AppDataDto, IpcErrorDto> {
    command_result((|| {
        let mut data = lock_data(state.inner())?;
        let mut next = data.clone();
        let _ = reconcile_codex_auth(&mut next)?;
        let (codex_changed, gemini_changed) = remove_account_from_data(&mut next, &account_id)?;

        if codex_changed || gemini_changed {
            commit_active_selection_changes(&mut data, next, codex_changed, gemini_changed)?;
        } else {
            commit_app_data(&mut data, next)?;
        }

        let result = data.clone();
        drop(data);
        crate::tray_dashboard::refresh_dashboard(state.inner());
        Ok(AppDataDto::from(&result))
    })())
}

pub(crate) fn set_active_account_data(
    account_id: &str,
    state: &Arc<SharedState>,
) -> AppResult<AppData> {
    let mut data = lock_data(state)?;
    let mut next = data.clone();
    let _ = reconcile_codex_auth(&mut next)?;
    if !next
        .accounts
        .iter()
        .any(|account| account.id == account_id && account.provider == AccountProvider::Codex)
    {
        return Err(AppError::msg("Codex account not found"));
    }

    next.active_account_id = Some(account_id.to_string());
    commit_active_selection_changes(&mut data, next, true, false)?;
    let result = data.clone();
    drop(data);
    crate::tray_dashboard::refresh_dashboard(state);
    auto_refresh::request_account_refresh_if_stale(state, account_id.to_string());
    Ok(result)
}

#[tauri::command]
pub fn switch_active_account_and_restart_codex(
    account_id: String,
    state: State<'_, Arc<SharedState>>,
) -> Result<SwitchAccountRestartResponse, IpcErrorDto> {
    command_result((|| {
        let state_data = set_active_account_data(&account_id, state.inner())?;
        crate::tray_dashboard::emit_state_changed(
            state.inner(),
            "accounts",
            vec![account_id.clone()],
        );
        let restart_warning = restart_codex_process().err().map(|err| err.user_message());

        Ok(SwitchAccountRestartResponse {
            state: AppDataDto::from(&state_data),
            restart_warning,
        })
    })())
}

pub(crate) async fn set_active_gemini_account_data(
    account_id: &str,
    state: &Arc<SharedState>,
) -> AppResult<AppData> {
    let gate = auto_refresh::account_gate_for_id(state, account_id)?;
    let _guard = gate.lock().await;
    let snapshot = {
        let data = lock_data(state)?;
        data.accounts
            .iter()
            .find(|account| account.id == account_id && account.provider == AccountProvider::Gemini)
            .cloned()
            .ok_or_else(|| AppError::msg("Antigravity account not found"))?
    };
    let token_set = crate::gemini_quota::fresh_google_tokens(
        &state.http_client,
        &snapshot.tokens,
        snapshot.token_expires_at,
    )
    .await?;

    let mut data = lock_data(state)?;
    let current = data
        .accounts
        .iter()
        .find(|account| account.id == account_id)
        .ok_or_else(|| AppError::msg("Antigravity account disappeared during switch"))?;
    if current.tokens != snapshot.tokens || current.tokens_updated_at != snapshot.tokens_updated_at
    {
        return Err(AppError::msg(
            "Antigravity credentials changed during the switch. Try again.",
        ));
    }
    let mut next = data.clone();
    let selected = next
        .accounts
        .iter_mut()
        .find(|account| account.id == account_id)
        .ok_or_else(|| AppError::msg("Antigravity account disappeared during switch"))?;
    if selected.tokens != token_set.tokens
        || selected.token_expires_at != Some(token_set.expires_at)
    {
        selected.tokens = token_set.tokens;
        selected.token_expires_at = Some(token_set.expires_at);
        selected.tokens_updated_at = Some(now_ts());
        selected.token_health = TokenHealth::refreshed();
    }
    next.active_gemini_account_id = Some(account_id.to_string());
    commit_active_selection_changes(&mut data, next, false, true)?;
    let result = data.clone();
    drop(data);
    crate::tray_dashboard::refresh_dashboard(state);
    auto_refresh::request_account_refresh_if_stale(state, account_id.to_string());
    Ok(result)
}

#[tauri::command]
pub async fn switch_active_gemini_account_and_restart_antigravity(
    account_id: String,
    state: State<'_, Arc<SharedState>>,
) -> Result<SwitchAccountRestartResponse, IpcErrorDto> {
    command_result(
        async {
            let state_data = set_active_gemini_account_data(&account_id, state.inner()).await?;
            crate::tray_dashboard::emit_state_changed(state.inner(), "accounts", vec![account_id]);

            let targets = &state_data.app_settings.gemini_switch_targets;
            let mut warnings = Vec::new();

            if targets.iter().any(|t| t == "antigravity")
                && let Err(err) = crate::gemini::restart_antigravity_process()
            {
                warnings.push(format!("Antigravity: {}", err.user_message()));
            }
            if targets.iter().any(|t| t == "ide")
                && let Err(err) = crate::gemini::restart_antigravity_ide_process()
            {
                warnings.push(format!("Antigravity IDE: {}", err.user_message()));
            }

            let restart_warning = if warnings.is_empty() {
                None
            } else {
                Some(warnings.join("; "))
            };

            Ok(SwitchAccountRestartResponse {
                state: AppDataDto::from(&state_data),
                restart_warning,
            })
        }
        .await,
    )
}

#[tauri::command]
pub fn get_antigravity_surfaces() -> Vec<AntigravitySurfaceDto> {
    crate::gemini::get_antigravity_surfaces()
}

#[tauri::command]
pub async fn import_antigravity_account(
    state: State<'_, Arc<SharedState>>,
) -> Result<StateResultDto, IpcErrorDto> {
    command_result(
        async {
            let external = crate::gemini::read_antigravity_auth()?.ok_or_else(|| {
                let storage_name = if cfg!(target_os = "macos") {
                    "macOS Keychain"
                } else {
                    "Windows Credential Manager"
                };
                AppError::msg(format!(
                    "No Antigravity session was found in {storage_name}. Sign in to Antigravity or use Sign in with Google."
                ))
            })?;
            let token_set = crate::gemini_quota::fresh_google_tokens(
                &state.http_client,
                &external.tokens,
                external.expires_at,
            )
            .await?;
            let (email, google_account_id) = crate::oauth_gemini::fetch_google_user_info(
                &state.http_client,
                &token_set.tokens,
            )
            .await?;
            let quota_result = crate::gemini_quota::fetch_gemini_quota(
                &state.http_client,
                &token_set.tokens,
                None,
            )
            .await;
            let project_id = quota_result
                .as_ref()
                .ok()
                .and_then(|result| result.project_id.clone());

            let mut data = lock_data(state.inner())?;
            let mut next = data.clone();
            let account = crate::oauth_gemini::save_authenticated_gemini_account(
                &mut next,
                token_set.tokens,
                Some(token_set.expires_at),
                email,
                google_account_id,
                project_id,
                None,
            )?;
            let account_mut = next
                .accounts
                .iter_mut()
                .find(|entry| entry.id == account.id)
                .ok_or_else(|| AppError::msg("Account disappeared during Antigravity import"))?;
            let mut warnings = Vec::new();
            match quota_result {
                Ok(result) => {
                    if let Some(plan_type) = result.quota.plan_type.as_ref() {
                        account_mut.subscription_plan = Some(plan_type.clone());
                        account_mut.subscription_detected_at = Some(now_ts());
                    }
                    account_mut.quota = Some(result.quota);
                    account_mut.token_health = TokenHealth::healthy();
                    account_mut.last_error = None;
                }
                Err(error) => {
                    let message = error.user_message();
                    account_mut.last_error = Some(message.clone());
                    warnings.push(warning(
                        "antigravity_quota_unavailable",
                        "account",
                        message,
                        Some(account.id.clone()),
                        true,
                    ));
                }
            }

            next.active_gemini_account_id = Some(account.id.clone());
            commit_active_selection_changes(&mut data, next, false, true)?;
            let result = data.clone();
            drop(data);
            crate::tray_dashboard::refresh_dashboard(state.inner());
            crate::tray_dashboard::emit_state_changed(
                state.inner(),
                "accounts",
                vec![account.id],
            );

            Ok(StateResultDto {
                state: AppDataDto::from(&result),
                warnings,
            })
        }
        .await,
    )
}

#[tauri::command]
pub async fn import_codex_account(
    state: State<'_, Arc<SharedState>>,
) -> Result<StateResultDto, IpcErrorDto> {
    command_result(
        async {
            let external = crate::codex::read_codex_auth()?.ok_or_else(|| {
                AppError::msg(
                    "No active ChatGPT session was found in ~/.codex/auth.json. Sign in to ChatGPT or use Sign in with ChatGPT."
                )
            })?;

            let tokens = external.tokens;
            let account_id = external.account_id;
            let email = external.email;

            let saved_id = {
                let mut data = lock_data(state.inner())?;
                let mut next = data.clone();
                let account = crate::oauth::save_authenticated_account(
                    &mut next,
                    tokens,
                    email,
                    account_id,
                    None,
                )?;

                next.active_account_id = Some(account.id.clone());
                commit_active_selection_changes(&mut data, next, true, false)?;
                account.id
            };

            let refresh_result = RefreshService::new(Arc::clone(state.inner()))
                .refresh_account_subscription(&saved_id)
                .await;

            let mut warnings = Vec::new();
            let final_state = match refresh_result {
                Ok(result) => {
                    if let Some(msg) = result.warning {
                        warnings.push(warning(
                            "subscription_refresh_warning",
                            "account",
                            msg,
                            Some(saved_id.clone()),
                            true,
                        ));
                    }
                    result.state
                }
                Err(err) => {
                    warnings.push(warning(
                        "subscription_refresh_warning",
                        "account",
                        err.user_message(),
                        Some(saved_id.clone()),
                        true,
                    ));
                    lock_data(state.inner())?.clone()
                }
            };

            crate::tray_dashboard::refresh_dashboard(state.inner());
            crate::tray_dashboard::emit_state_changed(
                state.inner(),
                "accounts",
                vec![saved_id],
            );

            Ok(StateResultDto {
                state: AppDataDto::from(&final_state),
                warnings,
            })
        }
        .await,
    )
}

#[tauri::command]
pub fn set_account_order(
    account_ids: Vec<String>,
    state: State<'_, Arc<SharedState>>,
) -> Result<AppDataDto, IpcErrorDto> {
    command_result((|| {
        let mut data = lock_data(state.inner())?;

        if account_ids.len() != data.accounts.len() {
            return Err(AppError::msg(
                "Account order must include every account exactly once",
            ));
        }

        let existing_ids: HashSet<String> = data
            .accounts
            .iter()
            .map(|account| account.id.clone())
            .collect();
        let mut seen = HashSet::with_capacity(account_ids.len());

        for account_id in &account_ids {
            if !seen.insert(account_id.as_str()) {
                return Err(AppError::msg(
                    "Account order contains a duplicate account id",
                ));
            }
            if !existing_ids.contains(account_id) {
                return Err(AppError::msg(
                    "Account order contains an unknown account id",
                ));
            }
        }

        let mut next = data.clone();
        let mut by_id: HashMap<String, Account> = next
            .accounts
            .drain(..)
            .map(|account| (account.id.clone(), account))
            .collect();

        next.accounts = account_ids
            .into_iter()
            .filter_map(|account_id| by_id.remove(&account_id))
            .collect();

        commit_app_data(&mut data, next)?;
        let result = data.clone();
        drop(data);
        crate::tray_dashboard::refresh_dashboard(state.inner());
        Ok(AppDataDto::from(&result))
    })())
}

#[tauri::command]
pub async fn refresh_account_subscription(
    account_id: String,
    state: State<'_, Arc<SharedState>>,
) -> Result<StateResultDto, IpcErrorDto> {
    command_result(
        async {
            let is_gemini = lock_data(state.inner())?
                .accounts
                .iter()
                .find(|account| account.id == account_id)
                .is_some_and(|account| account.provider == AccountProvider::Gemini);
            let gate = auto_refresh::account_gate_for_id(state.inner(), &account_id)?;
            let _guard = gate.lock().await;
            if is_gemini {
                let outcome =
                    crate::gemini_quota::refresh_gemini_account(state.inner(), &account_id).await?;
                auto_refresh::update_account_schedule(
                    state.inner(),
                    &account_id,
                    outcome.succeeded,
                )?;
                let snapshot = lock_data(state.inner())?.clone();
                let warnings = outcome
                    .warnings
                    .into_iter()
                    .map(|message| {
                        warning(
                            "antigravity_refresh_warning",
                            "account",
                            message,
                            Some(account_id.clone()),
                            true,
                        )
                    })
                    .collect();
                return Ok(StateResultDto {
                    state: AppDataDto::from(&snapshot),
                    warnings,
                });
            }

            let result = RefreshService::new(Arc::clone(state.inner()))
                .refresh_account_subscription(&account_id)
                .await?;
            let warnings = result
                .warning
                .into_iter()
                .map(|message| {
                    warning(
                        "subscription_refresh_warning",
                        "account",
                        message,
                        Some(account_id.clone()),
                        true,
                    )
                })
                .collect();
            crate::tray_dashboard::refresh_dashboard(state.inner());
            Ok(StateResultDto {
                state: AppDataDto::from(&result.state),
                warnings,
            })
        }
        .await,
    )
}

#[tauri::command]
pub async fn refresh_account_quota(
    account_id: String,
    state: State<'_, Arc<SharedState>>,
) -> Result<StateResultDto, IpcErrorDto> {
    command_result(
        async {
            let is_gemini = lock_data(state.inner())?
                .accounts
                .iter()
                .find(|account| account.id == account_id)
                .is_some_and(|account| account.provider == AccountProvider::Gemini);
            let gate = auto_refresh::account_gate_for_id(state.inner(), &account_id)?;
            let _guard = gate.lock().await;
            if is_gemini {
                let outcome =
                    crate::gemini_quota::refresh_gemini_account(state.inner(), &account_id).await?;
                auto_refresh::update_account_schedule(
                    state.inner(),
                    &account_id,
                    outcome.succeeded,
                )?;
                let next_state = lock_data(state.inner())?.clone();
                crate::tray_dashboard::refresh_dashboard_and_alerts(state.inner());
                let mut warnings = outcome
                    .warnings
                    .into_iter()
                    .map(|message| {
                        warning(
                            "antigravity_refresh_warning",
                            "account",
                            message,
                            Some(account_id.clone()),
                            true,
                        )
                    })
                    .collect::<Vec<_>>();
                if !outcome.succeeded {
                    let is_needs_relogin = next_state
                        .accounts
                        .iter()
                        .find(|account| account.id == account_id)
                        .is_some_and(|account| {
                            account.token_health.status == TokenHealthStatus::NeedsRelogin
                        });
                    if !is_needs_relogin {
                        let message = next_state
                            .accounts
                            .iter()
                            .find(|account| account.id == account_id)
                            .and_then(|account| account.last_error.clone())
                            .unwrap_or_else(|| {
                                "Antigravity quota refresh did not complete".to_string()
                            });
                        warnings.push(warning(
                            "antigravity_quota_warning",
                            "account",
                            message,
                            Some(account_id.clone()),
                            true,
                        ));
                    }
                }
                return Ok(StateResultDto {
                    state: AppDataDto::from(&next_state),
                    warnings,
                });
            }

            let outcome = RefreshService::new(Arc::clone(state.inner()))
                .refresh_account_quota(&account_id)
                .await?;
            let refresh_succeeded = outcome.succeeded;
            auto_refresh::update_account_schedule(state.inner(), &account_id, refresh_succeeded)?;
            let next_state = lock_data(state.inner())?.clone();
            crate::tray_dashboard::refresh_dashboard_and_alerts(state.inner());

            let mut warnings = outcome
                .warnings
                .into_iter()
                .map(|message| {
                    warning(
                        "auth_reconcile_warning",
                        "account",
                        message,
                        Some(account_id.clone()),
                        true,
                    )
                })
                .collect::<Vec<_>>();
            if refresh_succeeded && outcome.warning == Some(QuotaRefreshWarning::TokenUpdateSkipped)
            {
                warnings.push(warning(
                    "token_update_skipped",
                    "account",
                    "Token update skipped: credentials changed externally during refresh"
                        .to_string(),
                    Some(account_id.clone()),
                    false,
                ));
            }
            if !refresh_succeeded {
                let is_needs_relogin = next_state
                    .accounts
                    .iter()
                    .find(|account| account.id == account_id)
                    .is_some_and(|account| {
                        account.token_health.status == TokenHealthStatus::NeedsRelogin
                    });
                if !is_needs_relogin {
                    let message = next_state
                        .accounts
                        .iter()
                        .find(|account| account.id == account_id)
                        .and_then(|account| account.last_error.clone())
                        .unwrap_or_else(|| "Quota refresh did not complete".to_string());
                    warnings.push(warning(
                        "quota_refresh_warning",
                        "account",
                        message,
                        Some(account_id.clone()),
                        true,
                    ));
                }
            }

            Ok(StateResultDto {
                state: AppDataDto::from(&next_state),
                warnings,
            })
        }
        .await,
    )
}

#[tauri::command]
pub async fn refresh_all_quotas(
    provider: Option<String>,
    state: State<'_, Arc<SharedState>>,
) -> Result<StateResultDto, IpcErrorDto> {
    command_result(
        async {
            let target_provider =
                provider
                    .as_deref()
                    .and_then(|p| match p.to_lowercase().as_str() {
                        "codex" | "chatgpt" => Some(AccountProvider::Codex),
                        "gemini" | "antigravity" => Some(AccountProvider::Gemini),
                        _ => None,
                    });
            let result =
                auto_refresh::refresh_accounts_for_provider(state.inner(), target_provider).await?;
            let mut warnings = result
                .warnings
                .into_iter()
                .map(|message| warning("refresh_warning", "account", message, None, false))
                .collect::<Vec<_>>();
            for account_id in result.failed_account_ids {
                let is_needs_relogin = result
                    .state
                    .accounts
                    .iter()
                    .find(|account| account.id == account_id)
                    .is_some_and(|account| {
                        account.token_health.status == TokenHealthStatus::NeedsRelogin
                    });
                if !is_needs_relogin {
                    warnings.push(warning(
                        "refresh_failed",
                        "account",
                        "Quota refresh failed for this account".to_string(),
                        Some(account_id),
                        true,
                    ));
                }
            }
            Ok(StateResultDto {
                state: AppDataDto::from(&result.state),
                warnings,
            })
        }
        .await,
    )
}

#[cfg(test)]
mod tests {
    use crate::errors::AppError;
    use crate::models::{Account, AppData, TokenHealth, Tokens};
    use crate::refresh_service::apply_subscription_result;

    use super::remove_account_from_data;

    fn account(id: &str) -> Account {
        Account {
            id: id.to_string(),
            provider: crate::models::AccountProvider::Codex,
            email: Some(format!("{id}@example.com")),
            account_id: Some(format!("openai-{id}")),
            provider_project_id: None,
            subscription_expires_at: Some(2_000_000_000),
            subscription_plan: Some("Plus".to_string()),
            subscription_detected_at: Some(1_900_000_000),
            subscription_checked_at: Some(1_900_000_000),
            subscription_next_check_at: Some(2_000_000_000),
            subscription_endpoint_hint: Some("account-payments".to_string()),
            tokens: Tokens {
                id_token: format!("id-{id}"),
                access_token: format!("access-{id}"),
                refresh_token: format!("refresh-{id}"),
            },
            token_expires_at: None,
            tokens_updated_at: Some(1_900_000_000),
            token_health: TokenHealth::healthy(),
            quota: None,
            quota_next_refresh_at: None,
            quota_refresh_failures: 0,
            created_at: 1_800_000_000,
            last_login_at: 1_900_000_000,
            last_error: None,
            subscription_error: None,
        }
    }

    #[test]
    fn removing_active_account_selects_the_next_account() {
        let first = account("first");
        let second = account("second");
        let mut data = AppData {
            accounts: vec![first.clone(), second.clone()],
            active_account_id: Some(first.id.clone()),
            ..AppData::default()
        };
        data.app_settings.hidden_account_ids = vec![first.id.clone(), second.id.clone()];

        assert_eq!(
            remove_account_from_data(&mut data, "first").expect("remove account"),
            (true, false)
        );
        assert_eq!(data.accounts.len(), 1);
        assert_eq!(data.active_account_id.as_deref(), Some(second.id.as_str()));
        assert_eq!(data.app_settings.hidden_account_ids, vec![second.id]);
    }

    #[test]
    fn removing_the_last_active_account_clears_selection() {
        let only = account("only");
        let mut data = AppData {
            accounts: vec![only.clone()],
            active_account_id: Some(only.id),
            ..AppData::default()
        };

        assert_eq!(
            remove_account_from_data(&mut data, "only").expect("remove account"),
            (true, false)
        );
        assert!(data.accounts.is_empty());
        assert_eq!(data.active_account_id, None);
    }

    #[test]
    fn removing_active_antigravity_account_selects_only_a_non_expired_fallback() {
        let mut active = account("google-active");
        active.provider = crate::models::AccountProvider::Gemini;
        active.token_expires_at = Some(1_900_000_000);
        let mut expired = account("google-expired");
        expired.provider = crate::models::AccountProvider::Gemini;
        expired.token_expires_at = Some(1);
        let mut usable = account("google-usable");
        usable.provider = crate::models::AccountProvider::Gemini;
        usable.token_expires_at = Some(crate::models::now_ts() + 3_600);
        let mut data = AppData {
            accounts: vec![active.clone(), expired, usable.clone()],
            active_gemini_account_id: Some(active.id),
            ..AppData::default()
        };

        assert_eq!(
            remove_account_from_data(&mut data, "google-active").expect("remove account"),
            (false, true)
        );
        assert_eq!(
            data.active_gemini_account_id.as_deref(),
            Some(usable.id.as_str())
        );
    }

    #[test]
    fn subscription_errors_preserve_last_known_values() {
        let mut account = account("subscription");
        let previous_expiry = account.subscription_expires_at;
        let previous_detected_at = account.subscription_detected_at;
        let result = Err(AppError::msg("Subscription request timed out"));

        apply_subscription_result(&mut account, &result);

        assert_eq!(account.subscription_expires_at, previous_expiry);
        assert_eq!(account.subscription_detected_at, previous_detected_at);
        assert_eq!(account.subscription_plan.as_deref(), Some("Plus"));
    }
}
