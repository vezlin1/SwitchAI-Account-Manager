use std::sync::Arc;
use tauri::State;

use crate::app_state::{SharedState, lock_data};
use crate::auto_refresh;
use crate::commands::{command_result, warning};
use crate::dto::{AccountDto, AccountRefreshResultDto, AppDataDto, StateResultDto};
use crate::errors::{AppError, IpcErrorDto};
use crate::models::{AccountProvider, TokenHealthStatus};
use crate::refresh_service::{QuotaRefreshWarning, RefreshService};

#[tauri::command]
pub async fn refresh_account_subscription(
    account_id: String,
    state: State<'_, Arc<SharedState>>,
) -> Result<AccountRefreshResultDto, IpcErrorDto> {
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
                crate::tray_dashboard::refresh_dashboard_and_alerts(state.inner());
                crate::tray_dashboard::emit_state_changed(
                    state.inner(),
                    "account",
                    vec![account_id.clone()],
                );
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
                let account = snapshot
                    .accounts
                    .iter()
                    .find(|account| account.id == account_id)
                    .map(AccountDto::from)
                    .ok_or_else(|| AppError::msg("Account not found"))?;
                return Ok(AccountRefreshResultDto { account, warnings });
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
            crate::tray_dashboard::emit_state_changed(
                state.inner(),
                "account",
                vec![account_id.clone()],
            );
            let account = result
                .state
                .accounts
                .iter()
                .find(|account| account.id == account_id)
                .map(AccountDto::from)
                .ok_or_else(|| AppError::msg("Account not found"))?;
            Ok(AccountRefreshResultDto { account, warnings })
        }
        .await,
    )
}

#[tauri::command]
pub async fn refresh_account_quota(
    account_id: String,
    state: State<'_, Arc<SharedState>>,
) -> Result<AccountRefreshResultDto, IpcErrorDto> {
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
                crate::tray_dashboard::emit_state_changed(
                    state.inner(),
                    "account",
                    vec![account_id.clone()],
                );
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
                let account = next_state
                    .accounts
                    .iter()
                    .find(|account| account.id == account_id)
                    .map(AccountDto::from)
                    .ok_or_else(|| AppError::msg("Account not found"))?;
                return Ok(AccountRefreshResultDto { account, warnings });
            }

            let outcome = RefreshService::new(Arc::clone(state.inner()))
                .refresh_account_quota(&account_id)
                .await?;
            let refresh_succeeded = outcome.succeeded;
            auto_refresh::update_account_schedule(state.inner(), &account_id, refresh_succeeded)?;
            let next_state = lock_data(state.inner())?.clone();
            crate::tray_dashboard::refresh_dashboard_and_alerts(state.inner());
            crate::tray_dashboard::emit_state_changed(
                state.inner(),
                "account",
                vec![account_id.clone()],
            );

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

            let account = next_state
                .accounts
                .iter()
                .find(|account| account.id == account_id)
                .map(AccountDto::from)
                .ok_or_else(|| AppError::msg("Account not found"))?;

            Ok(AccountRefreshResultDto { account, warnings })
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
