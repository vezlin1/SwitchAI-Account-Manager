pub mod accounts;
pub mod oauth;
pub mod refresh;
pub mod system;
pub mod updater;

pub use accounts::*;
pub use oauth::*;
pub use refresh::*;
pub use system::*;
pub use updater::*;

use crate::dto::CommandWarningDto;
use crate::errors::{AppResult, IpcErrorDto, to_command_error};

pub(crate) fn warning(
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

pub(crate) fn command_result<T>(result: AppResult<T>) -> Result<T, IpcErrorDto> {
    result.map_err(to_command_error)
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use crate::app_state::SharedState;
    use crate::errors::AppError;
    use crate::models::{Account, AppData, TokenHealth, Tokens};
    use crate::refresh_service::apply_subscription_result;

    use super::accounts::remove_account_from_data;

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
            active_account_id: Some(only.id.clone()),
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

    #[test]
    fn check_for_updates_respects_auto_check_disabled() {
        let mut data = AppData::default();
        data.app_settings.auto_check_updates = false;
        let state = Arc::new(SharedState::new_with_startup_error(data, None).unwrap());

        let res = tauri::async_runtime::block_on(super::updater::check_for_updates_internal(
            &state, false,
        ))
        .expect("check should succeed");
        assert!(!res.update_available);
    }

    #[test]
    fn dismissing_update_version_persists_and_clears_available_update() {
        let data = AppData::default();
        let state = Arc::new(SharedState::new_with_startup_error(data, None).unwrap());

        let manifest = crate::portable_updater::UpdateManifest {
            version: "9.9.9".to_string(),
            notes: Some("Notes".to_string()),
            release_date: None,
            platforms: std::collections::HashMap::new(),
        };
        *crate::app_state::lock_available_update(&state).unwrap() = Some(manifest);

        let res = super::updater::dismiss_update_version_internal(&state, "9.9.9")
            .expect("dismiss should succeed");

        assert_eq!(
            res.app_settings.ignored_update_version.as_deref(),
            Some("9.9.9")
        );
        assert!(
            crate::app_state::lock_available_update(&state)
                .unwrap()
                .is_none()
        );
    }
}
