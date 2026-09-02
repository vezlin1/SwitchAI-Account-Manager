use std::cmp::Ordering;
use std::sync::Arc;

use serde::Serialize;
use tauri::menu::{Menu, MenuBuilder, MenuItemBuilder};
use tauri::{Emitter, Runtime};
#[cfg(not(test))]
use tauri_plugin_notification::NotificationExt;

use crate::app_state::{SharedState, lock_data};
use crate::errors::{AppError, AppResult};
use crate::models::{Account, AccountProvider, AppData, TokenHealthStatus};

pub const TRAY_ID: &str = "main-tray";
pub const SWITCH_CODEX_ACCOUNT_PREFIX: &str = "switch-codex-account:";
pub const SWITCH_GEMINI_ACCOUNT_PREFIX: &str = "switch-antigravity-account:";

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

fn account_label(account: &Account) -> String {
    account
        .email
        .as_deref()
        .filter(|value| !value.trim().is_empty())
        .or(account.account_id.as_deref())
        .unwrap_or("Unnamed account")
        .to_string()
}

pub fn remaining_percent(account: &Account) -> Option<f64> {
    let quota = account.quota.as_ref()?;
    [&quota.primary, &quota.secondary]
        .into_iter()
        .filter_map(|window| window.used_percent)
        .map(|used| (100.0 - used).clamp(0.0, 100.0))
        .min_by(|left, right| left.partial_cmp(right).unwrap_or(Ordering::Equal))
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

#[allow(dead_code)]
pub fn recommended_account(data: &AppData) -> Option<&Account> {
    recommended_account_for_provider(data, AccountProvider::Codex)
        .or_else(|| recommended_account_for_provider(data, AccountProvider::Gemini))
}

pub fn build_menu<R: Runtime>(app: &tauri::AppHandle<R>, data: &AppData) -> AppResult<Menu<R>> {
    let rec_codex = recommended_account_for_provider(data, AccountProvider::Codex);
    let rec_gemini = recommended_account_for_provider(data, AccountProvider::Gemini);

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

    let mut builder = MenuBuilder::new(app);
    let has_codex = codex_enabled
        && data
            .accounts
            .iter()
            .any(|a| a.provider == AccountProvider::Codex);
    let has_gemini = gemini_enabled
        && data
            .accounts
            .iter()
            .any(|a| a.provider == AccountProvider::Gemini);

    if has_codex || has_gemini {
        if has_codex {
            let summary_codex = rec_codex
                .map(|account| {
                    format!(
                        "[Codex] Recommended: {} · {:.0}% left",
                        account_label(account),
                        remaining_percent(account).unwrap_or_default()
                    )
                })
                .unwrap_or_else(|| "[Codex] No quota recommendation available".to_string());
            let summary_item = MenuItemBuilder::with_id("quota-summary-codex", summary_codex)
                .enabled(false)
                .build(app)
                .map_err(|error| AppError::msg(format!("Failed to build tray summary: {error}")))?;
            builder = builder.item(&summary_item);
        }
        if has_gemini {
            let summary_gemini = rec_gemini
                .map(|account| {
                    format!(
                        "[Antigravity] Recommended: {} · {:.0}% left",
                        account_label(account),
                        remaining_percent(account).unwrap_or_default()
                    )
                })
                .unwrap_or_else(|| "[Antigravity] No quota recommendation available".to_string());
            let summary_item = MenuItemBuilder::with_id("quota-summary-gemini", summary_gemini)
                .enabled(false)
                .build(app)
                .map_err(|error| AppError::msg(format!("Failed to build tray summary: {error}")))?;
            builder = builder.item(&summary_item);
        }
    } else {
        let summary_item = MenuItemBuilder::with_id("quota-summary", "No accounts added")
            .enabled(false)
            .build(app)
            .map_err(|error| AppError::msg(format!("Failed to build tray summary: {error}")))?;
        builder = builder.item(&summary_item);
    }
    builder = builder.separator();

    for account in data
        .accounts
        .iter()
        .filter(|a| !data.app_settings.hidden_account_ids.contains(&a.id))
        .filter(|a| match a.provider {
            AccountProvider::Codex => codex_enabled,
            AccountProvider::Gemini => gemini_enabled,
        })
    {
        let (provider_label, prefix, active_id) = match account.provider {
            AccountProvider::Codex => (
                "Codex",
                SWITCH_CODEX_ACCOUNT_PREFIX,
                data.active_account_id.as_deref(),
            ),
            AccountProvider::Gemini => (
                "Antigravity",
                SWITCH_GEMINI_ACCOUNT_PREFIX,
                data.active_gemini_account_id.as_deref(),
            ),
        };
        let is_active = active_id == Some(account.id.as_str());
        let needs_relogin = account.token_health.status == TokenHealthStatus::NeedsRelogin;
        let active_marker = if is_active { " (active)" } else { "" };
        let relogin_marker = if needs_relogin {
            " (re-login required)"
        } else {
            ""
        };
        let quota = if needs_relogin {
            String::new()
        } else {
            remaining_percent(account)
                .map(|remaining| format!(" · {remaining:.0}% left"))
                .unwrap_or_else(|| " · quota unavailable".to_string())
        };
        let is_recommended = !needs_relogin
            && match account.provider {
                AccountProvider::Codex => rec_codex.is_some_and(|item| item.id == account.id),
                AccountProvider::Gemini => rec_gemini.is_some_and(|item| item.id == account.id),
            };
        let recommended_marker = if is_recommended { " (recommended)" } else { "" };
        let item = MenuItemBuilder::with_id(
            format!("{prefix}{}", account.id),
            format!(
                "[{provider_label}] {}{active_marker}{relogin_marker}{quota}{recommended_marker}",
                account_label(account),
            ),
        )
        .enabled(!is_active && !needs_relogin)
        .build(app)
        .map_err(|error| AppError::msg(format!("Failed to build tray account item: {error}")))?;
        builder = builder.item(&item);
    }

    let menu = builder
        .separator()
        .text("show", "Show account manager")
        .text("refresh", "Refresh quotas now")
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
    match (rec_codex, rec_gemini) {
        (Some(c), Some(g)) => format!(
            "SwitchAI — Codex: {} ({:.0}%) · Antigravity: {} ({:.0}%)",
            account_label(c),
            remaining_percent(c).unwrap_or_default(),
            account_label(g),
            remaining_percent(g).unwrap_or_default()
        ),
        (Some(c), None) => format!(
            "SwitchAI — [Codex] {} ({:.0}% left)",
            account_label(c),
            remaining_percent(c).unwrap_or_default()
        ),
        (None, Some(g)) => format!(
            "SwitchAI — [Antigravity] {} ({:.0}% left)",
            account_label(g),
            remaining_percent(g).unwrap_or_default()
        ),
        (None, None) => "SwitchAI".to_string(),
    }
}

fn refresh_dashboard_with_app_and_data(app: &tauri::AppHandle, data: &AppData) {
    let dispatch_app = app.clone();
    let data_clone = data.clone();
    let _ = app.run_on_main_thread(move || match build_menu(&dispatch_app, &data_clone) {
        Ok(menu) => {
            if let Some(tray) = dispatch_app.tray_by_id(TRAY_ID) {
                if let Err(error) = tray.set_menu(Some(menu)) {
                    log::warn!("Could not update tray menu: {error}");
                }
                let tooltip = tray_tooltip(&data_clone);
                let _ = tray.set_tooltip(Some(tooltip));
            }
        }
        Err(error) => log::warn!("Could not rebuild tray dashboard: {}", error.user_message()),
    });
}

pub fn refresh_dashboard(state: &Arc<SharedState>) {
    let Some(app) = state.app_handle.get() else {
        return;
    };
    let data = match lock_data(state) {
        Ok(data) => data,
        Err(error) => {
            log::warn!("Could not refresh tray dashboard: {}", error.user_message());
            return;
        }
    };
    refresh_dashboard_with_app_and_data(app, &data);
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

fn alert_body(account: &Account, level: u8, remaining: f64) -> String {
    match level {
        3 => format!(
            "{} has exhausted its available quota.",
            account_label(account)
        ),
        2 => format!(
            "{} is critical: only {remaining:.0}% quota remains.",
            account_label(account)
        ),
        1 => format!(
            "{} is running low: {remaining:.0}% quota remains.",
            account_label(account)
        ),
        _ => format!("{} quota has recovered.", account_label(account)),
    }
}

pub fn refresh_dashboard_and_alerts(state: &Arc<SharedState>) {
    let Some(app) = state.app_handle.get() else {
        return;
    };
    let data = match lock_data(state) {
        Ok(data) => data,
        Err(_) => return,
    };
    refresh_dashboard_with_app_and_data(app, &data);
    let mut levels = match state.quota_alert_levels.lock() {
        Ok(levels) => levels,
        Err(_) => return,
    };

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
        if should_notify
            && let Err(error) = show_notification(
                app,
                if next_level == 0 {
                    "Quota recovered"
                } else {
                    "Quota alert"
                },
                alert_body(account, next_level, remaining),
            )
        {
            log::warn!("Could not show quota notification: {error}");
        }
    }
    levels.retain(|account_id, _| {
        data.accounts
            .iter()
            .any(|account| &account.id == account_id)
    });
}

pub fn notify_account_selected(state: &Arc<SharedState>, account: &Account) {
    let Some(app) = state.app_handle.get() else {
        return;
    };
    let client = match account.provider {
        AccountProvider::Codex => "Codex",
        AccountProvider::Gemini => "Antigravity",
    };
    let _ = show_notification(
        app,
        "Account selected",
        format!(
            "{} will be used on the next {client} launch. The running client was not restarted.",
            account_label(account)
        ),
    );
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
}
