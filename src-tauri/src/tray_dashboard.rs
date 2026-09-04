use std::cmp::Ordering;
use std::sync::Arc;

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
    let main_active = app
        .get_webview_window("main")
        .is_some_and(|w| w.is_visible().unwrap_or(false) && !w.is_minimized().unwrap_or(false));
    let flyout_active = app
        .get_webview_window("tray-flyout")
        .is_some_and(|w| w.is_visible().unwrap_or(false) && !w.is_minimized().unwrap_or(false));
    if !main_active && !flyout_active {
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

fn account_label(account: &Account, privacy_mode: bool) -> String {
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

pub fn build_menu<R: Runtime>(app: &tauri::AppHandle<R>, _data: &AppData) -> AppResult<Menu<R>> {
    use tauri::Manager;
    let available_update_version = app.try_state::<Arc<SharedState>>().and_then(|state| {
        state
            .available_update
            .lock()
            .ok()
            .and_then(|guard| guard.as_ref().map(|manifest| manifest.version.clone()))
    });

    let mut builder = MenuBuilder::new(app);
    if let Some(version) = available_update_version {
        let update_item = MenuItemBuilder::with_id(
            "open_update",
            format!("★ SwitchAI Update Available (v{version})"),
        )
        .build(app)
        .map_err(|error| AppError::msg(format!("Failed to build tray update item: {error}")))?;
        builder = builder.item(&update_item);
        builder = builder.separator();
    }

    let menu = builder
        .text("show", "Open SwitchAI")
        .text("refresh", "Refresh quotas now")
        .separator()
        .text("quit", "Quit")
        .build()
        .map_err(|error| AppError::msg(format!("Failed to build tray menu: {error}")))?;
    Ok(menu)
}

pub fn toggle_tray_flyout(
    app: &tauri::AppHandle,
    state: &Arc<SharedState>,
    click_pos: tauri::PhysicalPosition<f64>,
    tray_rect: tauri::Rect,
) {
    let Some(flyout) = app.get_webview_window("tray-flyout") else {
        log::warn!("tray-flyout window not found");
        return;
    };

    let now = std::time::Instant::now();
    if let Ok(last_blurred) = state.flyout_last_blurred.lock() {
        if let Some(t) = *last_blurred {
            if now.duration_since(t) < std::time::Duration::from_millis(250) {
                return;
            }
        }
    }

    let is_visible = flyout.is_visible().unwrap_or(false);
    if is_visible {
        let _ = flyout.hide();
        return;
    }

    let monitor = app
        .monitor_from_point(click_pos.x, click_pos.y)
        .ok()
        .flatten()
        .or_else(|| flyout.current_monitor().ok().flatten())
        .or_else(|| app.primary_monitor().ok().flatten());

    let (mon_x, mon_y, mon_w, mon_h, scale) = if let Some(m) = monitor {
        let p = m.position();
        let s = m.size();
        (
            p.x as f64,
            p.y as f64,
            s.width as f64,
            s.height as f64,
            m.scale_factor(),
        )
    } else {
        (0.0, 0.0, 1920.0, 1080.0, 1.0)
    };

    let (rect_x, rect_y, rect_w, rect_h) = match (tray_rect.position, tray_rect.size) {
        (tauri::Position::Physical(p), tauri::Size::Physical(s)) => {
            (p.x as f64, p.y as f64, s.width as f64, s.height as f64)
        }
        (tauri::Position::Logical(l), tauri::Size::Logical(s)) => (
            l.x * scale,
            l.y * scale,
            s.width * scale,
            s.height * scale,
        ),
        (tauri::Position::Physical(p), tauri::Size::Logical(s)) => (
            p.x as f64,
            p.y as f64,
            s.width * scale,
            s.height * scale,
        ),
        (tauri::Position::Logical(l), tauri::Size::Physical(s)) => (
            l.x * scale,
            l.y * scale,
            s.width as f64,
            s.height as f64,
        ),
    };

    let mut anchor_x = if rect_w > 0.0 {
        rect_x + rect_w / 2.0
    } else {
        click_pos.x
    };
    let mut anchor_y = if rect_h > 0.0 {
        rect_y + rect_h / 2.0
    } else {
        click_pos.y
    };

    let logical_w = 380.0;
    let logical_h = 520.0;
    let flyout_w = logical_w * scale;
    let flyout_h = logical_h * scale;
    let margin = 12.0 * scale;

    // Fallback if OS gave no click or rect coords: default to bottom right near tray
    if anchor_x <= mon_x && anchor_y <= mon_y {
        anchor_x = mon_x + mon_w - flyout_w / 2.0 - margin;
        anchor_y = mon_y + mon_h - margin;
    }

    let is_bottom = anchor_y > (mon_y + mon_h / 2.0);
    let target_y = if is_bottom {
        let top_edge = if rect_h > 0.0 { rect_y } else { anchor_y };
        top_edge - flyout_h - margin
    } else {
        let bottom_edge = if rect_h > 0.0 { rect_y + rect_h } else { anchor_y };
        bottom_edge + margin
    };

    let target_x = anchor_x - flyout_w / 2.0;

    let clamped_x = target_x.clamp(mon_x + margin, mon_x + mon_w - flyout_w - margin);
    let clamped_y = target_y.clamp(mon_y + margin, mon_y + mon_h - flyout_h - margin);

    let _ = flyout.set_size(tauri::Size::Logical(tauri::LogicalSize::new(
        logical_w, logical_h,
    )));
    let _ = flyout.set_position(tauri::Position::Physical(tauri::PhysicalPosition::new(
        clamped_x.round() as i32,
        clamped_y.round() as i32,
    )));
    let _ = flyout.show();
    let _ = flyout.unminimize();
    let _ = flyout.set_focus();

    if let Ok(mut last) = state.flyout_last_blurred.lock() {
        *last = None;
    }

    emit_state_changed_forced(state, "all", Vec::new());
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
    let data = match lock_data(state) {
        Ok(data) => data,
        Err(_) => return,
    };
    refresh_dashboard_with_app_and_data(app, &data);
    let mut levels = match state.quota_alert_levels.lock() {
        Ok(levels) => levels,
        Err(_) => return,
    };

    let privacy_mode = data.app_settings.privacy_mode;
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
                alert_body(account, next_level, remaining, privacy_mode),
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
}
