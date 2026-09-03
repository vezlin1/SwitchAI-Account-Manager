use std::sync::Arc;
use tauri::State;

use crate::app_state::{SharedState, lock_data, lock_flows};
use crate::commands::command_result;
use crate::errors::{AppError, IpcErrorDto};
use crate::models::now_ts;
use crate::oauth::{
    build_oauth_flow, cancel_oauth_flow as cancel_oauth_flow_state, ensure_callback_server,
    flow_to_response, prune_oauth_flows,
};

#[tauri::command]
pub fn start_oauth_flow(
    provider: Option<String>,
    target_account_id: Option<String>,
    state: State<'_, Arc<SharedState>>,
) -> Result<crate::oauth::OauthStartResponse, IpcErrorDto> {
    command_result((|| {
        let account_provider = match provider.as_deref() {
            Some("gemini") => crate::models::AccountProvider::Gemini,
            _ => crate::models::AccountProvider::Codex,
        };
        ensure_callback_server(state.inner())?;
        prune_oauth_flows(state.inner(), now_ts());
        let login_hint = if let Some(target_account_id) = target_account_id.as_deref() {
            let data = lock_data(state.inner())?;
            let account = data
                .accounts
                .iter()
                .find(|account| account.id == target_account_id)
                .ok_or_else(|| AppError::msg("Re-login target account not found"))?;
            if account.provider != account_provider {
                return Err(AppError::msg(
                    "Re-login provider does not match the selected account",
                ));
            }
            account.email.clone()
        } else {
            None
        };

        let (flow, response) = build_oauth_flow(
            account_provider,
            target_account_id.clone(),
            login_hint.as_deref(),
        )?;
        let mut flows = lock_flows(state.inner())?;
        if target_account_id.is_some()
            && flows.values().any(|existing| {
                existing.target_account_id == target_account_id
                    && matches!(
                        existing.status,
                        crate::oauth::OauthFlowStatus::WaitingCallback
                            | crate::oauth::OauthFlowStatus::Exchanging
                    )
            })
        {
            return Err(AppError::msg(
                "Re-login is already running for this account",
            ));
        }
        flows.insert(response.flow_id.clone(), flow);
        Ok(response)
    })())
}

#[tauri::command]
pub fn get_oauth_flow_status(
    flow_id: String,
    state: State<'_, Arc<SharedState>>,
) -> Result<crate::oauth::OauthFlowResponse, IpcErrorDto> {
    command_result((|| {
        prune_oauth_flows(state.inner(), now_ts());
        let flow = {
            let flows = lock_flows(state.inner())?;
            flows
                .get(&flow_id)
                .cloned()
                .ok_or_else(|| AppError::msg("OAuth flow not found"))?
        };
        let data = lock_data(state.inner())?;
        Ok(flow_to_response(&flow, &data))
    })())
}

#[tauri::command]
pub fn cancel_oauth_flow(
    flow_id: String,
    state: State<'_, Arc<SharedState>>,
) -> Result<(), IpcErrorDto> {
    command_result(cancel_oauth_flow_state(state.inner(), &flow_id))
}
