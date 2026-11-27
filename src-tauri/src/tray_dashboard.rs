use std::cmp::Ordering;
use std::collections::HashMap;
use std::sync::Arc;

use serde::Serialize;
use tauri::menu::{CheckMenuItemBuilder, Menu, MenuBuilder, MenuItemBuilder};
use tauri::{Emitter, Manager, Runtime};
#[cfg(not(test))]
use tauri_plugin_notification::NotificationExt;

use crate::app_state::{SharedState, lock_data};
use crate::errors::{AppError, AppResult};
use crate::models::{Account, AccountProvider, AppData, TokenHealthStatus};
use crate::recommendation::{QuotaFocus, Recommendation, quota_focus};

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

pub fn remaining_percent(account: &Account, focus: QuotaFocus) -> Option<f64> {
    crate::recommendation::remaining_percent(account, focus)
}

pub fn quota_suffix(account: &Account, focus: QuotaFocus) -> String {
    if account.token_health.status == TokenHealthStatus::NeedsRelogin {
        " · signed out".to_string()
    } else if let Some(remaining) = remaining_percent(account, focus) {
        format!(" · {remaining:.0}% left")
    } else {
        " · no data yet".to_string()
    }
}

/// A tray line for one account. The account in use is shown by the menu's own
/// check mark, so the text carries no symbols.
pub fn account_menu_label(
    account: &Account,
    focus: QuotaFocus,
    is_recommended: bool,
    privacy_mode: bool,
) -> String {
    let name = account_label(account, privacy_mode);
    let best = if is_recommended {
        " · best right now"
    } else {
        ""
    };
    format!("{name}{}{best}", quota_suffix(account, focus))
}

pub fn sort_accounts_for_provider<'a>(
    accounts: &[&'a Account],
    active_id: Option<&str>,
    focus: QuotaFocus,
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

        let a_rem = remaining_percent(a, focus);
        let b_rem = remaining_percent(b, focus);
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

/// The recommended account and its decision details; see `recommendation`.
pub fn recommended_account_for_provider(
    data: &AppData,
    provider: AccountProvider,
) -> Option<(&Account, Recommendation)> {
    let recommendation = crate::recommendation::recommend(data, provider, crate::models::now_ts())?;
    let account = data
        .accounts
        .iter()
        .find(|account| account.id == recommendation.account_id)?;
    Some((account, recommendation))
}

fn recommended_quota_text(recommendation: &Recommendation) -> String {
    if recommendation.projected {
        format!("~{:.0}% left after reset", recommendation.remaining)
    } else {
        format!("{:.0}% left", recommendation.remaining)
    }
}

pub fn build_menu<R: Runtime>(app: &tauri::AppHandle<R>, data: &AppData) -> AppResult<Menu<R>> {
    let mut builder = MenuBuilder::new(app);

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
    let privacy_mode = data.app_settings.privacy_mode;

    let codex_accounts: Vec<&Account> = if codex_enabled {
        data.accounts
            .iter()
            .filter(|a| a.provider == AccountProvider::Codex)
            .filter(|a| !data.app_settings.hidden_account_ids.contains(&a.id))
            .collect()
    } else {
        Vec::new()
    };

    let gemini_accounts: Vec<&Account> = if gemini_enabled {
        data.accounts
            .iter()
            .filter(|a| a.provider == AccountProvider::Gemini)
            .filter(|a| !data.app_settings.hidden_account_ids.contains(&a.id))
            .collect()
    } else {
        Vec::new()
    };

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

    // 1. Actionable 1-click Quick-Switch section at the top
    let has_any_accounts = !codex_accounts.is_empty() || !gemini_accounts.is_empty();
    if has_any_accounts {
        if codex_enabled && !codex_accounts.is_empty() {
            if let Some((best, recommendation)) = &rec_codex {
                let is_active = data.active_account_id.as_deref() == Some(best.id.as_str());
                let quota_str = recommended_quota_text(recommendation);
                let label = account_label(best, privacy_mode);

                let item = if is_active {
                    MenuItemBuilder::with_id(
                        "quick-codex-active",
                        format!("Codex is on the best account: {label} · {quota_str}"),
                    )
                    .enabled(false)
                    .build(app)
                    .map_err(|error| {
                        AppError::msg(format!("Failed to build quick switch item: {error}"))
                    })?
                } else {
                    MenuItemBuilder::with_id(
                        format!("{QUICK_SWITCH_CODEX_PREFIX}{}", best.id),
                        format!("Switch Codex to {label} · {quota_str}"),
                    )
                    .enabled(true)
                    .build(app)
                    .map_err(|error| {
                        AppError::msg(format!("Failed to build quick switch item: {error}"))
                    })?
                };
                builder = builder.item(&item);
            } else {
                let item = MenuItemBuilder::with_id(
                    "quick-codex-none",
                    "Codex: no account to recommend yet",
                )
                .enabled(false)
                .build(app)
                .map_err(|error| {
                    AppError::msg(format!("Failed to build quick switch item: {error}"))
                })?;
                builder = builder.item(&item);
            }
        }

        if gemini_enabled && !gemini_accounts.is_empty() {
            if let Some((best, recommendation)) = &rec_gemini {
                let is_active = data.active_gemini_account_id.as_deref() == Some(best.id.as_str());
                let quota_str = recommended_quota_text(recommendation);
                let label = account_label(best, privacy_mode);

                let item = if is_active {
                    MenuItemBuilder::with_id(
                        "quick-gemini-active",
                        format!("Antigravity is on the best account: {label} · {quota_str}"),
                    )
                    .enabled(false)
                    .build(app)
                    .map_err(|error| {
                        AppError::msg(format!("Failed to build quick switch item: {error}"))
                    })?
                } else {
                    MenuItemBuilder::with_id(
                        format!("{QUICK_SWITCH_GEMINI_PREFIX}{}", best.id),
                        format!("Switch Antigravity to {label} · {quota_str}"),
                    )
                    .enabled(true)
                    .build(app)
                    .map_err(|error| {
                        AppError::msg(format!("Failed to build quick switch item: {error}"))
                    })?
                };
                builder = builder.item(&item);
            } else {
                let item = MenuItemBuilder::with_id(
                    "quick-gemini-none",
                    "Antigravity: no account to recommend yet",
                )
                .enabled(false)
                .build(app)
                .map_err(|error| {
                    AppError::msg(format!("Failed to build quick switch item: {error}"))
                })?;
                builder = builder.item(&item);
            }
        }
    } else {
        let empty_item = MenuItemBuilder::with_id("no-accounts", "No accounts yet")
            .enabled(false)
            .build(app)
            .map_err(|error| AppError::msg(format!("Failed to build tray summary: {error}")))?;
        builder = builder.item(&empty_item);
    }

    builder = builder.separator();

    // 2. Sectioned accounts by provider
    if codex_enabled {
        let header_codex = MenuItemBuilder::with_id("header-codex", "Codex")
            .enabled(false)
            .build(app)
            .map_err(|error| AppError::msg(format!("Failed to build header: {error}")))?;
        builder = builder.item(&header_codex);

        if codex_accounts.is_empty() {
            let item = MenuItemBuilder::with_id("empty-codex", "No Codex accounts yet")
                .enabled(false)
                .build(app)
                .map_err(|error| AppError::msg(format!("Failed to build tray item: {error}")))?;
            builder = builder.item(&item);
        } else {
            let sorted_codex = sort_accounts_for_provider(
                &codex_accounts,
                data.active_account_id.as_deref(),
                quota_focus(&data.app_settings, AccountProvider::Codex),
            );
            for account in sorted_codex {
                let is_active = data.active_account_id.as_deref() == Some(account.id.as_str());
                let needs_relogin = account.token_health.status == TokenHealthStatus::NeedsRelogin;
                let is_recommended = !needs_relogin
                    && rec_codex
                        .as_ref()
                        .is_some_and(|(item, _)| item.id == account.id);
                let label = account_menu_label(
                    account,
                    quota_focus(&data.app_settings, AccountProvider::Codex),
                    is_recommended && !is_active,
                    privacy_mode,
                );

                let item = CheckMenuItemBuilder::with_id(
                    format!("{SWITCH_CODEX_ACCOUNT_PREFIX}{}", account.id),
                    label,
                )
                .checked(is_active)
                .enabled(!is_active && !needs_relogin)
                .build(app)
                .map_err(|error| {
                    AppError::msg(format!("Failed to build tray account item: {error}"))
                })?;
                builder = builder.item(&item);
            }
        }
    }

    if codex_enabled && gemini_enabled {
        builder = builder.separator();
    }

    if gemini_enabled {
        let header_gemini = MenuItemBuilder::with_id("header-antigravity", "Antigravity")
            .enabled(false)
            .build(app)
            .map_err(|error| AppError::msg(format!("Failed to build header: {error}")))?;
        builder = builder.item(&header_gemini);

        if gemini_accounts.is_empty() {
            let item = MenuItemBuilder::with_id("empty-gemini", "No Antigravity accounts yet")
                .enabled(false)
                .build(app)
                .map_err(|error| AppError::msg(format!("Failed to build tray item: {error}")))?;
            builder = builder.item(&item);
        } else {
            let sorted_gemini = sort_accounts_for_provider(
                &gemini_accounts,
                data.active_gemini_account_id.as_deref(),
                quota_focus(&data.app_settings, AccountProvider::Gemini),
            );
            for account in sorted_gemini {
                let is_active =
                    data.active_gemini_account_id.as_deref() == Some(account.id.as_str());
                let needs_relogin = account.token_health.status == TokenHealthStatus::NeedsRelogin;
                let is_recommended = !needs_relogin
                    && rec_gemini
                        .as_ref()
                        .is_some_and(|(item, _)| item.id == account.id);
                let label = account_menu_label(
                    account,
                    quota_focus(&data.app_settings, AccountProvider::Gemini),
                    is_recommended && !is_active,
                    privacy_mode,
                );

                let item = CheckMenuItemBuilder::with_id(
                    format!("{SWITCH_GEMINI_ACCOUNT_PREFIX}{}", account.id),
                    label,
                )
                .checked(is_active)
                .enabled(!is_active && !needs_relogin)
                .build(app)
                .map_err(|error| {
                    AppError::msg(format!("Failed to build tray account item: {error}"))
                })?;
                builder = builder.item(&item);
            }
        }
    }

    let menu = builder
        .separator()
        .text("show", "Open SwitchAI")
        .text("refresh", "Refresh limits now")
        .separator()
        .text("quit", "Quit")
        .build()
        .map_err(|error| AppError::msg(format!("Failed to build tray menu: {error}")))?;
    Ok(menu)
}

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
        (Some((c, rc)), Some((g, rg))) => format!(
            "SwitchAI — Codex: {} ({:.0}%) · Antigravity: {} ({:.0}%)",
            account_label(c, privacy_mode),
            rc.remaining,
            account_label(g, privacy_mode),
            rg.remaining
        ),
        (Some((c, rc)), None) => format!(
            "SwitchAI — [Codex] {} ({:.0}% left)",
            account_label(c, privacy_mode),
            rc.remaining
        ),
        (None, Some((g, rg))) => format!(
            "SwitchAI — [Antigravity] {} ({:.0}% left)",
            account_label(g, privacy_mode),
            rg.remaining
        ),
        (None, None) => "SwitchAI".to_string(),
    }
}

pub fn refresh_dashboard(state: &Arc<SharedState>) {
    use std::sync::atomic::Ordering;
    let Some(app) = state.app_handle.get() else {
        return;
    };
    if state.tray_refresh_pending.swap(true, Ordering::SeqCst) {
        return;
    }
    let app = app.clone();
    let shared = Arc::clone(state);
    std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_millis(150));
        let dispatch_state = Arc::clone(&shared);
        let dispatch_app = app.clone();
        if let Err(error) = app.run_on_main_thread(move || {
            dispatch_state
                .tray_refresh_pending
                .store(false, Ordering::SeqCst);
            let data = match lock_data(&dispatch_state) {
                Ok(data) => data.clone(),
                Err(_) => return,
            };
            match build_menu(&dispatch_app, &data) {
                Ok(menu) => {
                    if let Some(tray) = dispatch_app.tray_by_id(TRAY_ID) {
                        if let Err(error) = tray.set_menu(Some(menu)) {
                            log::warn!("Could not update tray menu: {error}");
                        }
                        let _ = tray.set_tooltip(Some(tray_tooltip(&data)));
                    }
                }
                Err(error) => {
                    log::warn!("Could not rebuild tray dashboard: {}", error.user_message())
                }
            }
        }) {
            shared.tray_refresh_pending.store(false, Ordering::SeqCst);
            log::warn!("Could not schedule tray dashboard: {error}");
        }
    });
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

fn provider_client_name(provider: AccountProvider) -> &'static str {
    match provider {
        AccountProvider::Codex => "Codex",
        AccountProvider::Gemini => "Antigravity",
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

fn switch_suggestion(data: &AppData, account: &Account) -> Option<String> {
    let (best, recommendation) = recommended_account_for_provider(data, account.provider)?;
    (best.id != account.id).then(|| {
        format!(
            " Best reserve: {} ({}). Switch from the tray menu.",
            account_label(best, data.app_settings.privacy_mode),
            recommended_quota_text(&recommendation)
        )
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct QuotaAlert {
    title: &'static str,
    body: String,
}

/// Decides which quota notifications to show and records the new levels.
///
/// - The first level seen for an account is recorded silently, so starting the
///   app never replays alerts for accounts that were already low.
/// - Running low is only reported for the account each client is using now;
///   inactive accounts running low need no action.
/// - When the active account is critical, an inactive account getting its quota
///   back is reported because it is now a way out.
fn quota_alerts(data: &AppData, levels: &mut HashMap<String, u8>) -> Vec<QuotaAlert> {
    let privacy_mode = data.app_settings.privacy_mode;
    let mut current_levels = HashMap::new();
    for account in &data.accounts {
        let focus = quota_focus(&data.app_settings, account.provider);
        if let Some(remaining) = remaining_percent(account, focus) {
            current_levels.insert(account.id.clone(), (alert_level(remaining), remaining));
        }
    }
    let active_level = |provider: AccountProvider| {
        data.accounts
            .iter()
            .filter(|account| account.provider == provider)
            .find(|account| crate::auto_refresh::is_account_active_in_data(account, data))
            .and_then(|account| current_levels.get(&account.id))
            .map(|(level, _)| *level)
    };

    let mut alerts = Vec::new();
    for account in &data.accounts {
        let Some(&(next_level, remaining)) = current_levels.get(&account.id) else {
            continue;
        };
        let Some(previous) = levels.insert(account.id.clone(), next_level) else {
            continue;
        };
        let is_active = crate::auto_refresh::is_account_active_in_data(account, data);
        let client = provider_client_name(account.provider);
        if is_active && next_level > previous {
            let mut body = alert_body(account, next_level, remaining, privacy_mode);
            if next_level >= 2
                && let Some(suggestion) = switch_suggestion(data, account)
            {
                body.push_str(&suggestion);
            }
            alerts.push(QuotaAlert {
                title: if next_level == 3 {
                    "Quota exhausted"
                } else {
                    "Quota alert"
                },
                body,
            });
        } else if previous > 0 && next_level == 0 {
            if is_active {
                alerts.push(QuotaAlert {
                    title: "Quota recovered",
                    body: alert_body(account, 0, remaining, privacy_mode),
                });
            } else if active_level(account.provider).is_some_and(|level| level >= 2) {
                alerts.push(QuotaAlert {
                    title: "Quota available",
                    body: format!(
                        "{} has quota again ({remaining:.0}% left). Switch {client} to it from the tray menu.",
                        account_label(account, privacy_mode)
                    ),
                });
            }
        }
    }
    levels.retain(|account_id, _| current_levels.contains_key(account_id));
    alerts
}

pub fn refresh_dashboard_and_alerts(state: &Arc<SharedState>) {
    refresh_dashboard(state);
    let Some(app) = state.app_handle.get() else {
        return;
    };
    let data = match lock_data(state) {
        Ok(data) => data,
        Err(_) => return,
    };
    let mut levels = match state.quota_alert_levels.lock() {
        Ok(levels) => levels,
        Err(_) => return,
    };

    for alert in quota_alerts(&data, &mut levels) {
        if let Err(error) = show_notification(app, alert.title, alert.body) {
            log::warn!("Could not show quota notification: {error}");
        }
    }
}

pub fn notify_account_selected(
    state: &Arc<SharedState>,
    account: &Account,
    restart_warning: Option<&str>,
    client_notice: Option<&str>,
) {
    let Some(app) = state.app_handle.get() else {
        return;
    };
    let privacy_mode = lock_data(state)
        .map(|data| data.app_settings.privacy_mode)
        .unwrap_or(false);
    let client = provider_client_name(account.provider);
    let mut body = match restart_warning {
        Some(warning) => format!(
            "{} selected. {client} restart warning: {warning}",
            account_label(account, privacy_mode)
        ),
        None => format!(
            "{} selected. Configured {client} restart completed.",
            account_label(account, privacy_mode)
        ),
    };
    if let Some(notice) = client_notice {
        body.push(' ');
        body.push_str(notice);
    }
    let _ = show_notification(app, "Account selected", body);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{QuotaInfo, QuotaWindow, TokenHealth};

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
                claude_and_gpt: None,
                fetched_at: crate::models::now_ts(),
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
        assert_eq!(remaining_percent(&account, QuotaFocus::Primary), Some(75.0));

        let no_quota = make_account("2", "user2@test.com", AccountProvider::Codex, None);
        assert_eq!(remaining_percent(&no_quota, QuotaFocus::Primary), None);
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
        assert_eq!(best_codex.map(|(a, _)| a.id.as_str()), Some("c2"));

        let best_gemini = recommended_account_for_provider(&data, AccountProvider::Gemini);
        assert_eq!(best_gemini.map(|(a, _)| a.id.as_str()), Some("g1"));

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
        assert_eq!(quota_suffix(&healthy, QuotaFocus::Primary), " · 90% left");

        let no_quota = make_account("4", "u@test.com", AccountProvider::Codex, None);
        assert_eq!(
            quota_suffix(&no_quota, QuotaFocus::Primary),
            " · no data yet"
        );

        let mut relogin = make_account("5", "u@test.com", AccountProvider::Codex, Some(10.0));
        relogin.token_health.status = TokenHealthStatus::NeedsRelogin;
        assert_eq!(quota_suffix(&relogin, QuotaFocus::Primary), " · signed out");
    }

    #[test]
    fn test_account_menu_label() {
        let account = make_account("1", "user@test.com", AccountProvider::Codex, Some(10.0)); // 90% left
        assert_eq!(
            account_menu_label(&account, QuotaFocus::Primary, false, false),
            "user@test.com · 90% left"
        );
        assert_eq!(
            account_menu_label(&account, QuotaFocus::Primary, true, false),
            "user@test.com · 90% left · best right now"
        );

        let mut relogin = make_account("2", "relogin@test.com", AccountProvider::Codex, Some(10.0));
        relogin.token_health.status = TokenHealthStatus::NeedsRelogin;
        assert_eq!(
            account_menu_label(&relogin, QuotaFocus::Primary, false, false),
            "relogin@test.com · signed out"
        );

        // The menu's own check mark shows the account in use; no symbols.
        for label in [
            account_menu_label(&account, QuotaFocus::Primary, true, false),
            account_menu_label(&relogin, QuotaFocus::Primary, true, false),
        ] {
            assert!(!label.contains(['✓', '★', '⚠', '•', '⚡']), "{label}");
        }
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
        let sorted = sort_accounts_for_provider(&accounts, Some("c1"), QuotaFocus::Primary);
        let ids: Vec<&str> = sorted.into_iter().map(|a| a.id.as_str()).collect();
        assert_eq!(ids, vec!["c1", "c2", "c4", "c3"]);

        // Case 2: no active account -> c2 (90%), c4 (50%), c1 (20%), c3 (relogin)
        let sorted2 = sort_accounts_for_provider(&accounts, None, QuotaFocus::Primary);
        let ids2: Vec<&str> = sorted2.into_iter().map(|a| a.id.as_str()).collect();
        assert_eq!(ids2, vec!["c2", "c4", "c1", "c3"]);
    }

    #[test]
    fn alerts_are_silent_on_first_sight_and_only_escalate_for_the_active_account() {
        let mut data = AppData::default();
        data.accounts.push(make_account(
            "active",
            "active@test.com",
            AccountProvider::Codex,
            Some(50.0),
        ));
        data.accounts.push(make_account(
            "other",
            "other@test.com",
            AccountProvider::Codex,
            Some(95.0),
        ));
        data.active_account_id = Some("active".into());
        let mut levels = HashMap::new();

        // Startup: an already exhausted inactive account must not notify.
        assert!(quota_alerts(&data, &mut levels).is_empty());

        // An inactive account dropping further stays silent.
        data.accounts[1]
            .quota
            .as_mut()
            .unwrap()
            .primary
            .used_percent = Some(100.0);
        assert!(quota_alerts(&data, &mut levels).is_empty());

        // The active account going critical notifies once.
        data.accounts[0]
            .quota
            .as_mut()
            .unwrap()
            .primary
            .used_percent = Some(92.0);
        let alerts = quota_alerts(&data, &mut levels);
        assert_eq!(alerts.len(), 1);
        assert_eq!(alerts[0].title, "Quota alert");
        assert!(alerts[0].body.contains("active@test.com"));
        assert!(quota_alerts(&data, &mut levels).is_empty());
    }

    #[test]
    fn critical_alert_suggests_the_best_reserve_and_reports_a_way_out() {
        let mut data = AppData::default();
        data.accounts.push(make_account(
            "active",
            "active@test.com",
            AccountProvider::Codex,
            Some(50.0),
        ));
        data.accounts.push(make_account(
            "spare",
            "spare@test.com",
            AccountProvider::Codex,
            Some(100.0),
        ));
        data.active_account_id = Some("active".into());
        let mut levels = HashMap::new();
        quota_alerts(&data, &mut levels);

        // Spare recovers while the active account is still fine: no alert.
        data.accounts[1]
            .quota
            .as_mut()
            .unwrap()
            .primary
            .used_percent = Some(10.0);
        assert!(quota_alerts(&data, &mut levels).is_empty());

        data.accounts[0]
            .quota
            .as_mut()
            .unwrap()
            .primary
            .used_percent = Some(100.0);
        let alerts = quota_alerts(&data, &mut levels);
        assert_eq!(alerts.len(), 1);
        assert_eq!(alerts[0].title, "Quota exhausted");
        assert!(
            alerts[0]
                .body
                .contains("Best reserve: spare@test.com (90% left)")
        );

        // Spare runs out and comes back while the active one is exhausted.
        data.accounts[1]
            .quota
            .as_mut()
            .unwrap()
            .primary
            .used_percent = Some(100.0);
        assert!(quota_alerts(&data, &mut levels).is_empty());
        data.accounts[1]
            .quota
            .as_mut()
            .unwrap()
            .primary
            .used_percent = Some(0.0);
        let alerts = quota_alerts(&data, &mut levels);
        assert_eq!(alerts.len(), 1);
        assert_eq!(alerts[0].title, "Quota available");
        assert!(
            alerts[0]
                .body
                .contains("spare@test.com has quota again (100% left)")
        );
    }
}
