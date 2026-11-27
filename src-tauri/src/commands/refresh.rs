use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use tauri::State;

use crate::app_state::{SharedState, lock_data};
use crate::auto_refresh;
use crate::commands::{command_result, run_blocking, warning};
use crate::dto::{
    AccountDto, AccountRefreshResultDto, AppDataDto, CommandWarningDto, StateResultDto,
};
use crate::errors::{AppError, AppResult, IpcErrorDto};
use crate::models::{AccountProvider, TokenHealthStatus};
use crate::refresh_service::{QuotaRefreshWarning, RefreshService};
use crate::tray_dashboard::{emit_state_changed, refresh_dashboard, refresh_dashboard_and_alerts};

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

fn report_selection_change(
    state: &Arc<SharedState>,
    account_id: &str,
    previous: &(Option<String>, Option<String>),
) -> AppResult<()> {
    let selection_changed = {
        let data = lock_data(state)?;
        data.active_account_id != previous.0 || data.active_gemini_account_id != previous.1
    };
    if selection_changed {
        emit_state_changed(state, "accounts", vec![account_id.to_owned()]);
    }
    Ok(())
}

async fn refresh_account(
    state: &Arc<SharedState>,
    account_id: String,
    kind: AccountRefreshKind,
) -> AppResult<AccountRefreshResultDto> {
    let gate = auto_refresh::account_gate_for_id(state, &account_id)?;
    let _guard = gate.lock().await;
    let (provider, selection) = {
        let data = lock_data(state)?;
        let provider = data
            .accounts
            .iter()
            .find(|account| account.id == account_id)
            .map(|account| account.provider)
            .ok_or_else(|| AppError::msg("Account not found"))?;
        (
            provider,
            (
                data.active_account_id.clone(),
                data.active_gemini_account_id.clone(),
            ),
        )
    };

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
        report_selection_change(state, &account_id, &selection)?;
        return Ok(AccountRefreshResultDto { account, warnings });
    }

    let (outcome, token_warning, warning_code, failure_code, failure_message) = match provider {
        AccountProvider::Gemini => (
            crate::gemini_quota::refresh_gemini_account_with_options(
                state,
                &account_id,
                kind == AccountRefreshKind::Subscription,
                false,
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
                crate::refresh_service::AccountRefreshOutcome {
                    succeeded: result.succeeded,
                    retry_after_seconds: result.retry_after_seconds,
                    warnings: result.warnings,
                },
                result.warning,
                "auth_reconcile_warning",
                "quota_refresh_warning",
                "Quota refresh did not complete",
            )
        }
    };
    let schedule_state = Arc::clone(state);
    let schedule_id = account_id.clone();
    run_blocking(move || {
        auto_refresh::update_account_schedule_with_retry(
            &schedule_state,
            &schedule_id,
            outcome.succeeded,
            outcome.retry_after_seconds,
        )
    })
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
    report_selection_change(state, &account_id, &selection)?;
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
            let warned_accounts: HashSet<_> = result
                .warnings
                .iter()
                .filter_map(|notice| notice.account_id.as_deref())
                .collect();
            let failed_accounts: HashSet<_> = result
                .failed_account_ids
                .iter()
                .map(String::as_str)
                .collect();
            let mut warnings = result
                .warnings
                .iter()
                .map(|notice| {
                    let code = if notice
                        .account_id
                        .as_deref()
                        .is_some_and(|id| failed_accounts.contains(id))
                    {
                        "refresh_failed"
                    } else {
                        "refresh_warning"
                    };
                    warning(
                        code,
                        "account",
                        notice.message.clone(),
                        notice.account_id.clone(),
                        false,
                    )
                })
                .collect::<Vec<_>>();
            if !result.failed_account_ids.is_empty() {
                let accounts: HashMap<_, _> = result
                    .state
                    .accounts
                    .iter()
                    .map(|account| (account.id.as_str(), account))
                    .collect();
                for account_id in &result.failed_account_ids {
                    let id = account_id.as_str();
                    if warned_accounts.contains(id)
                        || !accounts.get(id).is_some_and(|account| {
                            account.token_health.status != TokenHealthStatus::NeedsRelogin
                        })
                    {
                        continue;
                    }
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
