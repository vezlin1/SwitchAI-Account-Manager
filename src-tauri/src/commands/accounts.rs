use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use serde::Serialize;
use tauri::State;

use crate::app_state::{SharedState, lock_data};
use crate::auto_refresh;
use crate::codex::{reconcile_codex_auth, restart_codex_process};
use crate::commands::{command_result, warning};
use crate::dto::{
    AccountDto, AccountSnapshotDto, AntigravitySurfaceDto, AppDataDto, StateResultDto,
};
use crate::errors::{AppError, AppResult, IpcErrorDto};
use crate::models::{Account, AccountProvider, AppData, TokenHealth, now_ts};
use crate::refresh_service::{RefreshService, write_account_auth};
use crate::storage::{commit_state_data, persist_app_data};

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SwitchAccountRestartResponse {
    pub state: AppDataDto,
    pub restart_warning: Option<String>,
}

fn account_for_selection(data: &AppData, account_id: Option<&str>) -> Option<Account> {
    let account_id = account_id?;
    data.accounts
        .iter()
        .find(|account| account.id == account_id)
        .cloned()
}

fn commit_active_selection_changes(
    state: &Arc<SharedState>,
    mut current: std::sync::MutexGuard<'_, AppData>,
    next: AppData,
    codex_changed: bool,
    gemini_changed: bool,
) -> AppResult<AppData> {
    let previous_codex = account_for_selection(&current, current.active_account_id.as_deref());
    let next_codex = account_for_selection(&next, next.active_account_id.as_deref());
    let previous_antigravity = if gemini_changed {
        crate::gemini::read_antigravity_auth()?
    } else {
        None
    };
    let next_gemini = account_for_selection(&next, next.active_gemini_account_id.as_deref());

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

    if let Err(save_error) = persist_app_data(&current, &next) {
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

    let mut next = next;
    next.revision = current.revision.saturating_add(1);
    *current = next.clone();
    drop(current);

    crate::tray_dashboard::refresh_dashboard_and_alerts(state);
    Ok(next)
}

pub(crate) fn remove_account_from_data(
    data: &mut AppData,
    account_id: &str,
) -> AppResult<(bool, bool)> {
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
            .find(|account| account.provider == AccountProvider::Codex)
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
pub fn remove_account(
    account_id: String,
    state: State<'_, Arc<SharedState>>,
) -> Result<AppDataDto, IpcErrorDto> {
    command_result((|| {
        let (data, codex_changed, gemini_changed, next) = {
            let data = lock_data(state.inner())?;
            let mut next = data.clone();
            let _ = reconcile_codex_auth(&mut next)?;
            let (codex_changed, gemini_changed) = remove_account_from_data(&mut next, &account_id)?;
            (data, codex_changed, gemini_changed, next)
        };

        let result = if codex_changed || gemini_changed {
            commit_active_selection_changes(
                state.inner(),
                data,
                next,
                codex_changed,
                gemini_changed,
            )?
        } else {
            commit_state_data(state.inner(), data, next)?
        };

        Ok(AppDataDto::from(&result))
    })())
}

pub(crate) fn set_active_account_data(
    account_id: &str,
    state: &Arc<SharedState>,
) -> AppResult<AppData> {
    let (data, next) =
        {
            let data = lock_data(state)?;
            let mut next = data.clone();
            let _ = reconcile_codex_auth(&mut next)?;
            if !next.accounts.iter().any(|account| {
                account.id == account_id && account.provider == AccountProvider::Codex
            }) {
                return Err(AppError::msg("Codex account not found"));
            }

            next.active_account_id = Some(account_id.to_string());
            (data, next)
        };

    let result = commit_active_selection_changes(state, data, next, true, false)?;
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

    let (data, next) = {
        let data = lock_data(state)?;
        let current = data
            .accounts
            .iter()
            .find(|account| account.id == account_id)
            .ok_or_else(|| AppError::msg("Antigravity account disappeared during switch"))?;
        if current.tokens != snapshot.tokens
            || current.tokens_updated_at != snapshot.tokens_updated_at
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
        (data, next)
    };
    let result = commit_active_selection_changes(state, data, next, false, true)?;
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
            let (email, google_account_id) = crate::providers::gemini::oauth::fetch_google_user_info(
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

            let mut warnings = Vec::new();
            let (data, imported_id, next) = {
                let data = lock_data(state.inner())?;
                let mut next = data.clone();
                let account = crate::providers::gemini::oauth::save_authenticated_gemini_account(
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
                (data, account.id, next)
            };

            let result = commit_active_selection_changes(state.inner(), data, next, false, true)?;
            crate::tray_dashboard::emit_state_changed(
                state.inner(),
                "accounts",
                vec![imported_id],
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

            let (data, saved_id, next) = {
                let data = lock_data(state.inner())?;
                let mut next = data.clone();
                let account = crate::oauth::save_authenticated_account(
                    &mut next,
                    tokens,
                    email,
                    account_id,
                    None,
                )?;

                next.active_account_id = Some(account.id.clone());
                (data, account.id, next)
            };
            commit_active_selection_changes(state.inner(), data, next, true, false)?;

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
        let (data, next) = {
            let data = lock_data(state.inner())?;

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

            (data, next)
        };

        let result = commit_state_data(state.inner(), data, next)?;
        Ok(AppDataDto::from(&result))
    })())
}
