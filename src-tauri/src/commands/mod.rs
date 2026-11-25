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
use crate::errors::{AppError, AppResult, IpcErrorDto, to_command_error};

/// Runs filesystem, credential-store and process operations outside the window
/// thread and the async executor. Closures own their state, so no data guard can
/// accidentally cross an await point.
pub(crate) async fn run_blocking<T, F>(work: F) -> AppResult<T>
where
    T: Send + 'static,
    F: FnOnce() -> AppResult<T> + Send + 'static,
{
    tauri::async_runtime::spawn_blocking(work)
        .await
        .map_err(|error| AppError::msg(format!("Background operation failed: {error}")))?
}

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
    use crate::errors::AppError;
    use crate::models::{Account, AppData, TokenHealth, Tokens};
    use crate::refresh_service::apply_subscription_result;

    use super::accounts::remove_account_from_data;

    #[test]
    fn blocking_operations_leave_the_async_executor_responsive() {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .build()
            .unwrap();
        let (entered_tx, entered_rx) = tokio::sync::oneshot::channel();
        let (release_tx, release_rx) = std::sync::mpsc::channel();
        runtime.block_on(async {
            let progress = tokio::spawn(async move {
                entered_rx.await.unwrap();
                release_tx.send(()).unwrap();
            });
            let work = super::run_blocking(move || {
                entered_tx.send(()).unwrap();
                release_rx
                    .recv_timeout(std::time::Duration::from_secs(2))
                    .map_err(|_| AppError::msg("async executor stalled"))?;
                Ok(())
            })
            .await;
            work.expect("worker should not stall the executor");
            progress.await.unwrap();
        });
    }

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
}
