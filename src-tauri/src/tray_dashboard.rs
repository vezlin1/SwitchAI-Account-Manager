use std::cmp::Ordering;
use std::collections::HashSet;
use std::sync::{Arc, Mutex};

use serde::Serialize;
use tauri::menu::{Menu, MenuBuilder, MenuItemBuilder};
use tauri::{Emitter, Manager, Runtime};
#[cfg(not(test))]
use tauri_plugin_notification::NotificationExt;

use crate::app_state::{SharedState, lock_data};
use crate::errors::{AppError, AppResult};
use crate::models::{Account, AccountProvider, AppData, TokenHealthStatus};

pub const TRAY_ID: &str = "main-tray";
pub const SWITCH_CODEX_ACCOUNT_PREFIX: &str = "switch-codex-account:";
pub const SWITCH_GEMINI_ACCOUNT_PREFIX: &str = "switch-antigravity-account:";
pub const QUICK_SWITCH_CODEX_PREFIX: &str = "quick-switch-codex:";
pub const QUICK_SWITCH_GEMINI_PREFIX: &str = "quick-switch-antigravity:";

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppStateChangedEvent {
    pub scope: String,
    pub account_ids: Vec<String>,
    pub revision: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub state: Option<crate::dto::AppDataDto>,
}

pub fn emit_state_changed(
    state: &Arc<SharedState>,
    scope: impl Into<String>,
    account_ids: Vec<String>,
) {
    let Some(app) = state.app_handle.get() else {
        return;
    };
    if let Some(window) = app.get_webview_window("main")
        && (!window.is_visible().unwrap_or(false) || window.is_minimized().unwrap_or(false))
    {
        return;
    }
    let revision = lock_data(state).map(|data| data.revision).unwrap_or(0);
    let _ = app.emit(
        "app-state-changed",
        AppStateChangedEvent {
            scope: scope.into(),
            account_ids,
            revision,
            state: None,
        },
    );
}

pub fn emit_state_changed_forced(
    state: &Arc<SharedState>,
    scope: impl Into<String>,
    account_ids: Vec<String>,
) {
    let Some(app) = state.app_handle.get() else {
        return;
    };
    let revision = lock_data(state).map(|data| data.revision).unwrap_or(0);
    let _ = app.emit(
        "app-state-changed",
        AppStateChangedEvent {
            scope: scope.into(),
            account_ids,
            revision,
            state: None,
        },
    );
}

pub(crate) fn emit_refresh_snapshot(state: &Arc<SharedState>, snapshot: &AppData) {
    let Some(app) = state.app_handle.get() else {
        return;
    };
    if let Some(window) = app.get_webview_window("main")
        && (!window.is_visible().unwrap_or(false) || window.is_minimized().unwrap_or(false))
    {
        return;
    }
    let _ = app.emit(
        "app-state-changed",
        AppStateChangedEvent {
            scope: "accounts".into(),
            account_ids: Vec::new(),
            revision: snapshot.revision,
            state: Some(crate::dto::AppDataDto::from(snapshot)),
        },
    );
}

fn show_notification(app: &tauri::AppHandle, title: &str, body: String) -> AppResult<()> {
    #[cfg(not(test))]
    {
        app.notification()
            .builder()
            .title(title)
            .body(body)
            .show()
            .map_err(|error| AppError::msg(format!("Failed to show notification: {error}")))
    }
    #[cfg(test)]
    {
        let _ = (app, title, body);
        Ok(())
    }
}

pub fn mask_email(email: Option<&str>) -> String {
    let Some(email) = email.map(str::trim).filter(|s| !s.is_empty()) else {
        return "••••••".to_string();
    };
    let Some(at_index) = email.find('@') else {
        return "••••••••".to_string();
    };
    if at_index == 0 {
        return "••••••••".to_string();
    }
    let local = &email[..at_index];
    let domain = &email[at_index..];
    let local_chars: Vec<char> = local.chars().collect();
    match local_chars.len() {
        1 | 2 => {
            format!("{}•••{}", local_chars[0], domain)
        }
        3 | 4 => {
            format!(
                "{}••••{}{}",
                local_chars[0],
                local_chars.last().unwrap(),
                domain
            )
        }
        _ => {
            let prefix: String = local_chars[..2].iter().collect();
            format!("{prefix}••••••{}{domain}", local_chars.last().unwrap())
        }
    }
}

pub fn mask_account_id(id: Option<&str>) -> String {
    let Some(id) = id.map(str::trim).filter(|s| !s.is_empty()) else {
        return "••••••••".to_string();
    };
    let chars: Vec<char> = id.chars().collect();
    if chars.len() <= 6 {
        "••••••••".to_string()
    } else {
        let prefix: String = chars[..3].iter().collect();
        let suffix: String = chars[chars.len() - 3..].iter().collect();
        format!("{prefix}••••{suffix}")
    }
}

pub fn account_label(account: &Account, privacy_mode: bool) -> String {
    if privacy_mode {
        if let Some(email) = account
            .email
            .as_deref()
            .filter(|value| !value.trim().is_empty())
        {
            mask_email(Some(email))
        } else if let Some(account_id) = account
            .account_id
            .as_deref()
            .filter(|value| !value.trim().is_empty())
        {
            mask_account_id(Some(account_id))
        } else {
            "Unnamed account".to_string()
        }
    } else {
        account
            .email
            .as_deref()
            .filter(|value| !value.trim().is_empty())
            .or(account.account_id.as_deref())
            .unwrap_or("Unnamed account")
            .to_string()
    }
}

pub fn remaining_percent(account: &Account) -> Option<f64> {
    let quota = account.quota.as_ref()?;
    [&quota.primary, &quota.secondary]
        .into_iter()
        .filter_map(|window| window.used_percent)
        .map(|used| (100.0 - used).clamp(0.0, 100.0))
        .min_by(|left, right| left.partial_cmp(right).unwrap_or(Ordering::Equal))
}

pub fn quota_suffix(account: &Account) -> String {
    if account.token_health.status == TokenHealthStatus::NeedsRelogin {
        " · re-login required".to_string()
    } else if let Some(remaining) = remaining_percent(account) {
        format!(" · {remaining:.0}% left")
    } else {
        " · quota unavailable".to_string()
    }
}

pub fn account_menu_label(
    account: &Account,
    is_active: bool,
    is_recommended: bool,
    privacy_mode: bool,
) -> String {
    let name = account_label(account, privacy_mode);
    let rec_suffix = if is_recommended { " (recommended)" } else { "" };

    if account.token_health.status == TokenHealthStatus::NeedsRelogin {
        let prefix = if is_active {
            "✓ ⚠️ "
        } else if is_recommended {
            "★ ⚠️ "
        } else {
            "⚠️ "
        };
        format!("{prefix}{name} (re-login required){rec_suffix}")
    } else {
        let prefix = if is_active {
            "✓ "
        } else if is_recommended {
            "★ "
        } else {
            "• "
        };
        let quota = quota_suffix(account);
        format!("{prefix}{name}{quota}{rec_suffix}")
    }
}

pub fn sort_accounts_for_provider<'a>(
    accounts: &[&'a Account],
    active_id: Option<&str>,
) -> Vec<&'a Account> {
    let mut sorted = accounts.to_vec();
    sorted.sort_by(|a, b| {
        let a_active = active_id == Some(a.id.as_str());
        let b_active = active_id == Some(b.id.as_str());
        if a_active && !b_active {
            return Ordering::Less;
        }
        if !a_active && b_active {
            return Ordering::Greater;
        }

        let a_relogin = a.token_health.status == TokenHealthStatus::NeedsRelogin;
        let b_relogin = b.token_health.status == TokenHealthStatus::NeedsRelogin;
        if !a_relogin && b_relogin {
            return Ordering::Less;
        }
        if a_relogin && !b_relogin {
            return Ordering::Greater;
        }

        let a_rem = remaining_percent(a);
        let b_rem = remaining_percent(b);
        match (a_rem, b_rem) {
            (Some(ar), Some(br)) => br
                .partial_cmp(&ar)
                .unwrap_or(Ordering::Equal)
                .then_with(|| a.email.cmp(&b.email))
                .then_with(|| a.id.cmp(&b.id)),
            (Some(_), None) => Ordering::Less,
            (None, Some(_)) => Ordering::Greater,
            (None, None) => a.email.cmp(&b.email).then_with(|| a.id.cmp(&b.id)),
        }
    });
    sorted
}

pub fn recommended_account_for_provider(
    data: &AppData,
    provider: AccountProvider,
) -> Option<&Account> {
    data.accounts
        .iter()
        .filter(|account| account.provider == provider)
        .filter(|account| !data.app_settings.hidden_account_ids.contains(&account.id))
        .filter(|account| account.token_health.status != TokenHealthStatus::NeedsRelogin)
        .filter_map(|account| remaining_percent(account).map(|remaining| (account, remaining)))
        .max_by(|(_, left), (_, right)| left.partial_cmp(right).unwrap_or(Ordering::Equal))
        .map(|(account, _)| account)
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum DashboardEntry {
    Item {
        id: String,
        label: String,
        enabled: bool,
    },
    Separator,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct DashboardView {
    entries: Vec<DashboardEntry>,
    tooltip: String,
}

// Only rendered contents are sent to the main thread; no credentials are copied.
fn dashboard_view(data: &AppData) -> DashboardView {
    let mut entries = Vec::new();
    let item = |id: String, label: String, enabled| DashboardEntry::Item { id, label, enabled };
    let privacy = data.app_settings.privacy_mode;
    let providers = [
        (
            AccountProvider::Codex,
            "codex",
            "Codex",
            "codex",
            SWITCH_CODEX_ACCOUNT_PREFIX,
            QUICK_SWITCH_CODEX_PREFIX,
            data.active_account_id.as_deref(),
        ),
        (
            AccountProvider::Gemini,
            "gemini",
            "Antigravity",
            "gemini",
            SWITCH_GEMINI_ACCOUNT_PREFIX,
            QUICK_SWITCH_GEMINI_PREFIX,
            data.active_gemini_account_id.as_deref(),
        ),
    ];
    let visible_accounts = |provider| {
        data.accounts
            .iter()
            .filter(|account| account.provider == provider)
            .filter(|account| !data.app_settings.hidden_account_ids.contains(&account.id))
            .collect::<Vec<_>>()
    };
    let enabled = |key: &str| {
        data.app_settings
            .enabled_providers
            .iter()
            .any(|entry| entry == key)
    };
    let mut any_accounts = false;
    for &(provider, key, title, menu_key, _, quick_prefix, active_id) in &providers {
        if !enabled(key) || visible_accounts(provider).is_empty() {
            continue;
        }
        any_accounts = true;
        if let Some(best) = recommended_account_for_provider(data, provider) {
            let active = active_id == Some(best.id.as_str());
            let quota = remaining_percent(best)
                .map(|remaining| format!("{remaining:.0}% left"))
                .unwrap_or_else(|| "available".to_string());
            let label = account_label(best, privacy);
            entries.push(item(
                if active {
                    format!("quick-{menu_key}-active")
                } else {
                    format!("{quick_prefix}{}", best.id)
                },
                if active {
                    format!("⚡ Best {title}: {label} ({quota}) (active)")
                } else {
                    format!("⚡ Switch to Best {title}: {label} ({quota})")
                },
                !active,
            ));
        } else {
            entries.push(item(
                format!("quick-{menu_key}-none"),
                format!("⚡ No {title} recommendation available"),
                false,
            ));
        }
    }
    if !any_accounts {
        entries.push(item(
            "no-accounts".into(),
            "No accounts added".into(),
            false,
        ));
    }
    entries.push(DashboardEntry::Separator);
    let mut added_provider = false;
    for &(provider, key, title, menu_key, switch_prefix, _, active_id) in &providers {
        if !enabled(key) {
            continue;
        }
        if added_provider {
            entries.push(DashboardEntry::Separator);
        }
        added_provider = true;
        let header_id = if provider == AccountProvider::Codex {
            "header-codex"
        } else {
            "header-antigravity"
        };
        entries.push(item(header_id.into(), format!("── {title} ──"), false));
        let accounts = visible_accounts(provider);
        if accounts.is_empty() {
            entries.push(item(
                format!("empty-{menu_key}"),
                format!("   No {title} accounts added"),
                false,
            ));
            continue;
        }
        let recommended = recommended_account_for_provider(data, provider);
        for account in sort_accounts_for_provider(&accounts, active_id) {
            let active = active_id == Some(account.id.as_str());
            let needs_relogin = account.token_health.status == TokenHealthStatus::NeedsRelogin;
            let best = !needs_relogin && recommended.is_some_and(|best| best.id == account.id);
            entries.push(item(
                format!("{switch_prefix}{}", account.id),
                account_menu_label(account, active, best, privacy),
                !active && !needs_relogin,
            ));
        }
    }
    entries.extend([
        DashboardEntry::Separator,
        item("show".into(), "Show account manager".into(), true),
        item("refresh".into(), "Refresh quotas now".into(), true),
        DashboardEntry::Separator,
        item("quit".into(), "Quit".into(), true),
    ]);
    DashboardView {
        entries,
        tooltip: tray_tooltip(data),
    }
}

fn build_view_menu<R: Runtime>(
    app: &tauri::AppHandle<R>,
    view: &DashboardView,
) -> AppResult<Menu<R>> {
    let mut builder = MenuBuilder::new(app);
    for entry in &view.entries {
        match entry {
            DashboardEntry::Separator => builder = builder.separator(),
            DashboardEntry::Item { id, label, enabled } => {
                let item = MenuItemBuilder::with_id(id, label)
                    .enabled(*enabled)
                    .build(app)
                    .map_err(|error| {
                        AppError::msg(format!("Failed to build tray item: {error}"))
                    })?;
                builder = builder.item(&item);
            }
        }
    }
    builder
        .build()
        .map_err(|error| AppError::msg(format!("Failed to build tray menu: {error}")))
}

pub fn build_initial_menu<R: Runtime>(
    app: &tauri::AppHandle<R>,
    state: &Arc<SharedState>,
) -> AppResult<(Menu<R>, String)> {
    let (view, revision) = {
        let data = lock_data(state)?;
        (dashboard_view(&data), data.revision)
    };
    let menu = build_view_menu(app, &view)?;
    let tooltip = view.tooltip.clone();
    if let Some(queue) = app.try_state::<DashboardRefreshState>()
        && let Ok(mut queue) = queue.0.lock()
    {
        queue.applied = Some(view);
        queue.latest_revision = Some(revision);
    }
    Ok((menu, tooltip))
}

#[derive(Default)]
struct DashboardRefreshQueue {
    pending: Option<DashboardView>,
    applied: Option<DashboardView>,
    scheduled: bool,
    latest_revision: Option<u64>,
}

impl DashboardRefreshQueue {
    // Only the first caller dispatches; subsequent updates replace the pending view.
    fn enqueue(&mut self, view: DashboardView, revision: u64) -> bool {
        if self.latest_revision.is_some_and(|latest| revision < latest) {
            return false;
        }
        self.latest_revision = Some(revision);
        if !self.scheduled && self.applied.as_ref() == Some(&view) {
            return false;
        }
        self.pending = Some(view);
        if self.scheduled {
            return false;
        }
        self.scheduled = true;
        true
    }

    fn finish(&mut self, applied: Option<DashboardView>) -> bool {
        if let Some(applied) = applied {
            self.applied = Some(applied);
        }
        if self.pending.as_ref() == self.applied.as_ref() {
            self.pending = None;
        }
        self.scheduled = self.pending.is_some();
        self.scheduled
    }
}

#[derive(Default)]
pub(crate) struct DashboardRefreshState(Mutex<DashboardRefreshQueue>);

pub fn tray_tooltip(data: &AppData) -> String {
    let codex_enabled = data
        .app_settings
        .enabled_providers
        .iter()
        .any(|p| p == "codex");
    let gemini_enabled = data
        .app_settings
        .enabled_providers
        .iter()
        .any(|p| p == "gemini");
    let rec_codex = if codex_enabled {
        recommended_account_for_provider(data, AccountProvider::Codex)
    } else {
        None
    };
    let rec_gemini = if gemini_enabled {
        recommended_account_for_provider(data, AccountProvider::Gemini)
    } else {
        None
    };
    let privacy_mode = data.app_settings.privacy_mode;
    match (rec_codex, rec_gemini) {
        (Some(c), Some(g)) => format!(
            "SwitchAI — Codex: {} ({:.0}%) · Antigravity: {} ({:.0}%)",
            account_label(c, privacy_mode),
            remaining_percent(c).unwrap_or_default(),
            account_label(g, privacy_mode),
            remaining_percent(g).unwrap_or_default()
        ),
        (Some(c), None) => format!(
            "SwitchAI — [Codex] {} ({:.0}% left)",
            account_label(c, privacy_mode),
            remaining_percent(c).unwrap_or_default()
        ),
        (None, Some(g)) => format!(
            "SwitchAI — [Antigravity] {} ({:.0}% left)",
            account_label(g, privacy_mode),
            remaining_percent(g).unwrap_or_default()
        ),
        (None, None) => "SwitchAI".to_string(),
    }
}

fn dispatch_dashboard_refresh(app: &tauri::AppHandle) {
    let dispatch_app = app.clone();
    let result = app.run_on_main_thread(move || {
        let Some(queue) = dispatch_app.try_state::<DashboardRefreshState>() else {
            return;
        };
        let view = match queue.0.lock() {
            Ok(mut queue) => queue.pending.take(),
            Err(_) => return,
        };
        let mut applied = None;
        if let Some(view) = view {
            let already_applied = queue
                .0
                .lock()
                .ok()
                .is_some_and(|queue| queue.applied.as_ref() == Some(&view));
            if already_applied {
                applied = Some(view);
            } else if let Some(tray) = dispatch_app.tray_by_id(TRAY_ID) {
                match build_view_menu(&dispatch_app, &view) {
                    Ok(menu) => match tray.set_menu(Some(menu)) {
                        Ok(()) => match tray.set_tooltip(Some(&view.tooltip)) {
                            Ok(()) => applied = Some(view),
                            Err(error) => log::warn!("Could not update tray tooltip: {error}"),
                        },
                        Err(error) => log::warn!("Could not update tray menu: {error}"),
                    },
                    Err(error) => {
                        log::warn!("Could not rebuild tray dashboard: {}", error.user_message())
                    }
                }
            }
        }
        let again = queue
            .0
            .lock()
            .map(|mut queue| queue.finish(applied))
            .unwrap_or(false);
        if again {
            // run_on_main_thread executes inline when called on the window
            // thread. Dispatch the continuation from the executor to yield
            // to input/paint events and avoid recursive native menu rebuilds.
            tauri::async_runtime::spawn(async move {
                dispatch_dashboard_refresh(&dispatch_app);
            });
        }
    });
    if let Err(error) = result {
        if let Some(queue) = app.try_state::<DashboardRefreshState>()
            && let Ok(mut queue) = queue.0.lock()
        {
            queue.scheduled = false;
        }
        log::warn!("Could not schedule tray refresh: {error}");
    }
}

fn queue_dashboard_refresh(app: &tauri::AppHandle, view: DashboardView, revision: u64) {
    let Some(queue) = app.try_state::<DashboardRefreshState>() else {
        return;
    };
    let dispatch = queue
        .0
        .lock()
        .map(|mut queue| queue.enqueue(view, revision))
        .unwrap_or(false);
    if dispatch {
        dispatch_dashboard_refresh(app);
    }
}

pub fn refresh_dashboard(state: &Arc<SharedState>) {
    let Some(app) = state.app_handle.get() else {
        return;
    };
    let (view, revision) = match lock_data(state) {
        Ok(data) => (dashboard_view(&data), data.revision),
        Err(error) => {
            log::warn!("Could not refresh tray dashboard: {}", error.user_message());
            return;
        }
    };
    queue_dashboard_refresh(app, view, revision);
}

fn alert_level(remaining: f64) -> u8 {
    if remaining <= 0.0 {
        3
    } else if remaining <= 10.0 {
        2
    } else if remaining <= 20.0 {
        1
    } else {
        0
    }
}

fn alert_body(account: &Account, level: u8, remaining: f64, privacy_mode: bool) -> String {
    match level {
        3 => format!(
            "{} has exhausted its available quota.",
            account_label(account, privacy_mode)
        ),
        2 => format!(
            "{} is critical: only {remaining:.0}% quota remains.",
            account_label(account, privacy_mode)
        ),
        1 => format!(
            "{} is running low: {remaining:.0}% quota remains.",
            account_label(account, privacy_mode)
        ),
        _ => format!(
            "{} quota has recovered.",
            account_label(account, privacy_mode)
        ),
    }
}

pub fn refresh_dashboard_and_alerts(state: &Arc<SharedState>) {
    let Some(app) = state.app_handle.get() else {
        return;
    };
    let (view, revision, notifications) = {
        let data = match lock_data(state) {
            Ok(data) => data,
            Err(_) => return,
        };
        let mut levels = match state.quota_alert_levels.lock() {
            Ok(levels) => levels,
            Err(_) => return,
        };
        let mut notifications = Vec::new();
        for account in &data.accounts {
            let Some(remaining) = remaining_percent(account) else {
                continue;
            };
            let next_level = alert_level(remaining);
            let previous = levels.insert(account.id.clone(), next_level);
            let should_notify = match previous {
                None => next_level > 0,
                Some(old) => next_level > old || (old > 0 && next_level == 0),
            };
            if should_notify {
                notifications.push((
                    if next_level == 0 {
                        "Quota recovered"
                    } else {
                        "Quota alert"
                    },
                    alert_body(
                        account,
                        next_level,
                        remaining,
                        data.app_settings.privacy_mode,
                    ),
                ));
            }
        }
        let account_ids: HashSet<&str> = data
            .accounts
            .iter()
            .map(|account| account.id.as_str())
            .collect();
        levels.retain(|id, _| account_ids.contains(id.as_str()));
        (dashboard_view(&data), data.revision, notifications)
    };
    queue_dashboard_refresh(app, view, revision);
    for (title, body) in notifications {
        if let Err(error) = show_notification(app, title, body) {
            log::warn!("Could not show quota notification: {error}");
        }
    }
}

pub fn notify_account_selected(state: &Arc<SharedState>, account: &Account) {
    let Some(app) = state.app_handle.get() else {
        return;
    };
    let privacy_mode = lock_data(state)
        .map(|data| data.app_settings.privacy_mode)
        .unwrap_or(false);
    let client = match account.provider {
        AccountProvider::Codex => "Codex",
        AccountProvider::Gemini => "Antigravity",
    };
    let _ = show_notification(
        app,
        "Account selected",
        format!(
            "{} will be used on the next {client} launch. The running client was not restarted.",
            account_label(account, privacy_mode)
        ),
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{QuotaInfo, QuotaWindow, TokenHealth};

    #[test]
    fn dashboard_queue_coalesces_bursts_and_skips_identical_views() {
        let mut queue = DashboardRefreshQueue::default();
        let mut data = AppData::default();
        let first = dashboard_view(&data);
        data.app_settings.enabled_providers = vec!["codex".to_string()];
        let latest = dashboard_view(&data);
        assert!(queue.enqueue(first.clone(), 1));
        assert!(!queue.enqueue(latest.clone(), 1));
        assert_eq!(queue.pending.take(), Some(latest.clone()));
        assert!(!queue.finish(Some(latest.clone())));
        assert!(!queue.enqueue(latest, 1));
        assert!(queue.enqueue(first, 1));
    }

    #[test]
    fn dashboard_queue_rejects_older_revisions_without_rebuilding_equal_contents() {
        let mut queue = DashboardRefreshQueue::default();
        let mut data = AppData::default();
        let initial = dashboard_view(&data);
        data.app_settings.enabled_providers = vec!["codex".to_string()];
        let updated = dashboard_view(&data);
        assert!(queue.enqueue(updated.clone(), 10));
        assert!(!queue.enqueue(initial.clone(), 9));
        assert_eq!(queue.pending.take(), Some(updated.clone()));
        assert!(!queue.finish(Some(updated.clone())));
        // A token-only state change advances revision without changing the menu.
        assert!(!queue.enqueue(updated, 11));
        assert!(!queue.enqueue(initial.clone(), 10));
        // A view at the newest revision may still differ from the applied view.
        assert!(queue.enqueue(initial, 11));
    }

    #[test]
    fn dashboard_queue_preserves_changes_arriving_during_render() {
        let mut queue = DashboardRefreshQueue::default();
        let mut data = AppData::default();
        let first = dashboard_view(&data);
        data.app_settings.enabled_providers = vec!["codex".to_string()];
        let next = dashboard_view(&data);
        assert!(queue.enqueue(first.clone(), 1));
        assert_eq!(queue.pending.take(), Some(first.clone()));
        assert!(!queue.enqueue(next.clone(), 1));
        assert!(queue.finish(Some(first)));
        assert_eq!(queue.pending.take(), Some(next.clone()));
        assert!(!queue.finish(Some(next)));
        assert!(!queue.scheduled);
    }

    #[test]
    fn dashboard_projection_ignores_credentials_and_refresh_timestamps() {
        let mut data = AppData::default();
        data.accounts.push(make_account(
            "c1",
            "user@test.com",
            AccountProvider::Codex,
            Some(10.0),
        ));
        let before = dashboard_view(&data);
        data.revision += 1;
        data.accounts[0].tokens.access_token = "new private token".to_string();
        data.accounts[0].tokens_updated_at = Some(2_000_000_000);
        data.accounts[0].quota.as_mut().unwrap().fetched_at += 1;
        assert_eq!(before, dashboard_view(&data));
        data.app_settings.privacy_mode = true;
        assert_ne!(before, dashboard_view(&data));
    }

    #[test]
    fn dashboard_projection_retains_account_actions_and_relogin_gating() {
        let mut data = AppData::default();
        let mut account = make_account("c1", "user@test.com", AccountProvider::Codex, Some(10.0));
        account.token_health.status = TokenHealthStatus::NeedsRelogin;
        data.accounts.push(account);
        let view = dashboard_view(&data);
        assert!(view.entries.contains(&DashboardEntry::Item {
            id: "switch-codex-account:c1".to_string(),
            label: "⚠️ user@test.com (re-login required)".to_string(),
            enabled: false,
        }));
        data.app_settings.hidden_account_ids.push("c1".to_string());
        assert!(!dashboard_view(&data).entries.iter().any(|entry| matches!(entry, DashboardEntry::Item { id, .. } if id == "switch-codex-account:c1")));
    }

    fn make_account(
        id: &str,
        email: &str,
        provider: AccountProvider,
        used_percent: Option<f64>,
    ) -> Account {
        Account {
            id: id.to_string(),
            provider,
            email: Some(email.to_string()),
            account_id: None,
            provider_project_id: None,
            subscription_expires_at: None,
            subscription_plan: None,
            subscription_detected_at: None,
            subscription_checked_at: None,
            subscription_next_check_at: None,
            subscription_endpoint_hint: None,
            tokens: crate::models::Tokens::default(),
            token_expires_at: None,
            tokens_updated_at: None,
            token_health: TokenHealth {
                status: TokenHealthStatus::Healthy,
                last_checked_at: None,
                last_refreshed_at: None,
                last_error: None,
            },
            quota: used_percent.map(|used| QuotaInfo {
                plan_type: Some("Plus".to_string()),
                primary: QuotaWindow {
                    used_percent: Some(used),
                    limit_window_seconds: Some(18000),
                    reset_at: None,
                    fetched_at: None,
                },
                secondary: QuotaWindow::default(),
                fetched_at: 1000,
            }),
            quota_next_refresh_at: None,
            quota_refresh_failures: 0,
            created_at: 1000,
            last_login_at: 1000,
            last_error: None,
            subscription_error: None,
        }
    }

    #[test]
    fn calculates_remaining_percent() {
        let account = make_account("1", "user@test.com", AccountProvider::Codex, Some(25.0));
        assert_eq!(remaining_percent(&account), Some(75.0));

        let no_quota = make_account("2", "user2@test.com", AccountProvider::Codex, None);
        assert_eq!(remaining_percent(&no_quota), None);
    }

    #[test]
    fn recommended_account_per_provider_separates_codex_and_gemini() {
        let mut data = AppData::default();
        data.accounts.push(make_account(
            "c1",
            "codex-low@openai.com",
            AccountProvider::Codex,
            Some(80.0), // 20% left
        ));
        data.accounts.push(make_account(
            "c2",
            "codex-high@openai.com",
            AccountProvider::Codex,
            Some(10.0), // 90% left
        ));
        data.accounts.push(make_account(
            "g1",
            "gemini-high@google.com",
            AccountProvider::Gemini,
            Some(5.0), // 95% left
        ));
        data.accounts.push(make_account(
            "g2",
            "gemini-low@google.com",
            AccountProvider::Gemini,
            Some(60.0), // 40% left
        ));

        let best_codex = recommended_account_for_provider(&data, AccountProvider::Codex);
        assert_eq!(best_codex.map(|a| a.id.as_str()), Some("c2"));

        let best_gemini = recommended_account_for_provider(&data, AccountProvider::Gemini);
        assert_eq!(best_gemini.map(|a| a.id.as_str()), Some("g1"));

        let tooltip = tray_tooltip(&data);
        assert!(tooltip.contains("Codex: codex-high@openai.com (90%)"));
        assert!(tooltip.contains("Antigravity: gemini-high@google.com (95%)"));
    }

    #[test]
    fn tooltip_with_single_provider() {
        let mut data = AppData::default();
        data.accounts.push(make_account(
            "g1",
            "gemini@google.com",
            AccountProvider::Gemini,
            Some(30.0), // 70% left
        ));

        let tooltip = tray_tooltip(&data);
        assert_eq!(
            tooltip,
            "SwitchAI — [Antigravity] gemini@google.com (70% left)"
        );
    }

    #[test]
    fn test_mask_email() {
        assert_eq!(mask_email(None), "••••••");
        assert_eq!(mask_email(Some("")), "••••••");
        assert_eq!(mask_email(Some("invalid")), "••••••••");
        assert_eq!(mask_email(Some("@domain.com")), "••••••••");
        // local length <= 2: first char + 3 dots + domain
        assert_eq!(mask_email(Some("a@test.com")), "a•••@test.com");
        assert_eq!(mask_email(Some("ab@test.com")), "a•••@test.com");
        // local length 3..=4: first char + 4 dots + last char + domain
        assert_eq!(mask_email(Some("abc@test.com")), "a••••c@test.com");
        assert_eq!(mask_email(Some("user@test.com")), "u••••r@test.com");
        // local length >= 5: first 2 chars + 6 dots + last char + domain
        assert_eq!(mask_email(Some("admin@test.com")), "ad••••••n@test.com");
        assert_eq!(
            mask_email(Some("vezlin13@gmail.com")),
            "ve••••••3@gmail.com"
        );
    }

    #[test]
    fn test_mask_account_id() {
        assert_eq!(mask_account_id(None), "••••••••");
        assert_eq!(mask_account_id(Some("")), "••••••••");
        assert_eq!(mask_account_id(Some("short")), "••••••••");
        assert_eq!(mask_account_id(Some("123456")), "••••••••");
        assert_eq!(mask_account_id(Some("1234567")), "123••••567");
        assert_eq!(mask_account_id(Some("account-id-xyz")), "acc••••xyz");
    }

    #[test]
    fn tooltip_with_privacy_mode_masks_emails() {
        let mut data = AppData::default();
        data.app_settings.privacy_mode = true;
        data.accounts.push(make_account(
            "c1",
            "codex-high@openai.com",
            AccountProvider::Codex,
            Some(10.0), // 90% left
        ));
        data.accounts.push(make_account(
            "g1",
            "gemini-high@google.com",
            AccountProvider::Gemini,
            Some(5.0), // 95% left
        ));

        let tooltip = tray_tooltip(&data);
        assert!(tooltip.contains("Codex: co••••••h@openai.com (90%)"));
        assert!(tooltip.contains("Antigravity: ge••••••h@google.com (95%)"));
    }

    #[test]
    fn alert_body_respects_privacy_mode() {
        let account = make_account(
            "1",
            "vezlin13@gmail.com",
            AccountProvider::Codex,
            Some(25.0),
        );

        let plain = alert_body(&account, 1, 20.0, false);
        assert!(plain.contains("vezlin13@gmail.com"));

        let masked = alert_body(&account, 1, 20.0, true);
        assert!(masked.contains("ve••••••3@gmail.com"));
        assert!(!masked.contains("vezlin13@gmail.com"));
    }

    #[test]
    fn test_quota_suffix() {
        let healthy = make_account("1", "u@test.com", AccountProvider::Codex, Some(10.0)); // 90% left
        assert_eq!(quota_suffix(&healthy), " · 90% left");

        let no_quota = make_account("4", "u@test.com", AccountProvider::Codex, None);
        assert_eq!(quota_suffix(&no_quota), " · quota unavailable");

        let mut relogin = make_account("5", "u@test.com", AccountProvider::Codex, Some(10.0));
        relogin.token_health.status = TokenHealthStatus::NeedsRelogin;
        assert_eq!(quota_suffix(&relogin), " · re-login required");
    }

    #[test]
    fn test_account_menu_label() {
        let account = make_account("1", "user@test.com", AccountProvider::Codex, Some(10.0)); // 90% left

        // Active
        let label_active = account_menu_label(&account, true, false, false);
        assert_eq!(label_active, "✓ user@test.com · 90% left");

        // Active & Recommended
        let label_active_rec = account_menu_label(&account, true, true, false);
        assert_eq!(label_active_rec, "✓ user@test.com · 90% left (recommended)");

        // Inactive & Recommended
        let label_rec = account_menu_label(&account, false, true, false);
        assert_eq!(label_rec, "★ user@test.com · 90% left (recommended)");

        // Inactive normal
        let label_normal = account_menu_label(&account, false, false, false);
        assert_eq!(label_normal, "• user@test.com · 90% left");

        // Needs relogin
        let mut relogin = make_account("2", "relogin@test.com", AccountProvider::Codex, Some(10.0));
        relogin.token_health.status = TokenHealthStatus::NeedsRelogin;

        let label_relogin = account_menu_label(&relogin, false, false, false);
        assert_eq!(label_relogin, "⚠️ relogin@test.com (re-login required)");

        let label_relogin_active = account_menu_label(&relogin, true, false, false);
        assert_eq!(
            label_relogin_active,
            "✓ ⚠️ relogin@test.com (re-login required)"
        );
    }

    #[test]
    fn test_sort_accounts_for_provider() {
        let c1 = make_account("c1", "low@test.com", AccountProvider::Codex, Some(80.0)); // 20% left
        let c2 = make_account("c2", "high@test.com", AccountProvider::Codex, Some(10.0)); // 90% left
        let mut c3 = make_account("c3", "relogin@test.com", AccountProvider::Codex, Some(5.0)); // 95% left but relogin
        c3.token_health.status = TokenHealthStatus::NeedsRelogin;
        let c4 = make_account("c4", "mid@test.com", AccountProvider::Codex, Some(50.0)); // 50% left

        let accounts = vec![&c1, &c2, &c3, &c4];

        // Case 1: c1 is active -> c1 must be first, then c2 (90%), c4 (50%), and c3 (relogin) last
        let sorted = sort_accounts_for_provider(&accounts, Some("c1"));
        let ids: Vec<&str> = sorted.into_iter().map(|a| a.id.as_str()).collect();
        assert_eq!(ids, vec!["c1", "c2", "c4", "c3"]);

        // Case 2: no active account -> c2 (90%), c4 (50%), c1 (20%), c3 (relogin)
        let sorted2 = sort_accounts_for_provider(&accounts, None);
        let ids2: Vec<&str> = sorted2.into_iter().map(|a| a.id.as_str()).collect();
        assert_eq!(ids2, vec!["c2", "c4", "c1", "c3"]);
    }
}
