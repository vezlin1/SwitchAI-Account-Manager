use std::sync::Arc;
use tauri::State;

use crate::app_state::{SharedState, lock_data};
use crate::auto_refresh;
use crate::commands::{command_result, warning};
use crate::dto::{
    AccountDto, AccountRefreshResultDto, AppDataDto, CommandWarningDto, StateResultDto,
};
use crate::errors::{AppError, AppResult, IpcErrorDto};
use crate::models::{AccountProvider, TokenHealthStatus};
use crate::refresh_service::{QuotaRefreshWarning, RefreshService};
use crate::tray_dashboard::{refresh_dashboard, refresh_dashboard_and_alerts};

#[derive(Clone, Copy, PartialEq, Eq)]
enum AccountRefreshKind {
    Quota,
    Subscription,
}

fn account_warning(
    account_id: &str,
    code: &str,
    message: String,
    retryable: bool,
) -> CommandWarningDto {
    warning(
        code,
        "account",
        message,
        Some(account_id.to_owned()),
        retryable,
    )
}

async fn refresh_account(
    state: &Arc<SharedState>,
    account_id: String,
    kind: AccountRefreshKind,
) -> AppResult<AccountRefreshResultDto> {
    let gate = auto_refresh::account_gate_for_id(state, &account_id)?;
    let _guard = gate.lock().await;
    let provider = lock_data(state)?
        .accounts
        .iter()
        .find(|account| account.id == account_id)
        .map(|account| account.provider)
        .ok_or_else(|| AppError::msg("Account not found"))?;

    if provider == AccountProvider::Codex && kind == AccountRefreshKind::Subscription {
        let result = RefreshService::new(Arc::clone(state))
            .refresh_account_subscription(&account_id)
            .await?;
        let warnings = result
            .warning
            .into_iter()
            .map(|message| {
                account_warning(&account_id, "subscription_refresh_warning", message, true)
            })
            .collect();
        let account = result
            .state
            .accounts
            .iter()
            .find(|account| account.id == account_id)
            .map(AccountDto::from)
            .ok_or_else(|| AppError::msg("Account not found"))?;
        refresh_dashboard(state);
        auto_refresh::emit_account_snapshot(state, &account);
        return Ok(AccountRefreshResultDto { account, warnings });
    }

    let (outcome, token_warning, warning_code, failure_code, failure_message) = match provider {
        AccountProvider::Gemini => (
            crate::gemini_quota::refresh_gemini_account(
                state,
                &account_id,
                kind == AccountRefreshKind::Subscription,
            )
            .await?,
            None,
            "antigravity_refresh_warning",
            "antigravity_quota_warning",
            "Antigravity quota refresh did not complete",
        ),
        AccountProvider::Codex => {
            let result = RefreshService::new(Arc::clone(state))
                .refresh_account_quota(&account_id)
                .await?;
            (
                result.refresh,
                result.warning,
                "auth_reconcile_warning",
                "quota_refresh_warning",
                "Quota refresh did not complete",
            )
        }
    };
    auto_refresh::update_account_schedule(
        state,
        &account_id,
        outcome.succeeded,
        outcome.retry_after_seconds,
    )
    .await?;

    let mut warnings = outcome
        .warnings
        .into_iter()
        .map(|message| account_warning(&account_id, warning_code, message, true))
        .collect::<Vec<_>>();
    if outcome.succeeded && token_warning == Some(QuotaRefreshWarning::TokenUpdateSkipped) {
        warnings.push(account_warning(
            &account_id,
            "token_update_skipped",
            "Token update skipped: credentials changed externally during refresh".to_string(),
            false,
        ));
    }
    let account = {
        let data = lock_data(state)?;
        data.accounts
            .iter()
            .find(|account| account.id == account_id)
            .map(|account| {
                if kind == AccountRefreshKind::Quota
                    && !outcome.succeeded
                    && account.token_health.status != TokenHealthStatus::NeedsRelogin
                {
                    warnings.push(account_warning(
                        &account_id,
                        failure_code,
                        account
                            .last_error
                            .as_deref()
                            .unwrap_or(failure_message)
                            .to_owned(),
                        true,
                    ));
                }
                AccountDto::from(account)
            })
    }
    .ok_or_else(|| AppError::msg("Account not found"))?;
    refresh_dashboard_and_alerts(state);
    auto_refresh::emit_account_snapshot(state, &account);
    Ok(AccountRefreshResultDto { account, warnings })
}

#[tauri::command]
pub async fn refresh_account_subscription(
    account_id: String,
    state: State<'_, Arc<SharedState>>,
) -> Result<AccountRefreshResultDto, IpcErrorDto> {
    command_result(
        refresh_account(state.inner(), account_id, AccountRefreshKind::Subscription).await,
    )
}

#[tauri::command]
pub async fn refresh_account_quota(
    account_id: String,
    state: State<'_, Arc<SharedState>>,
) -> Result<AccountRefreshResultDto, IpcErrorDto> {
    command_result(refresh_account(state.inner(), account_id, AccountRefreshKind::Quota).await)
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
                .iter()
                .cloned()
                .map(|message| warning("refresh_warning", "account", message, None, false))
                .collect::<Vec<_>>();
            for account_id in &result.failed_account_ids {
                let is_needs_relogin = result
                    .state
                    .accounts
                    .iter()
                    .find(|account| account.id == *account_id)
                    .is_some_and(|account| {
                        account.token_health.status == TokenHealthStatus::NeedsRelogin
                    });
                if !is_needs_relogin {
                    warnings.push(warning(
                        "refresh_failed",
                        "account",
                        "Quota refresh failed for this account".to_string(),
                        Some(account_id.clone()),
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
