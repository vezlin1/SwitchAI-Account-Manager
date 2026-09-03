use std::collections::HashMap;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex, MutexGuard, OnceLock, Weak};
use std::time::Duration;

use reqwest::Client;

use crate::auto_refresh::{AutoRefreshRuntime, RefreshAllResult};
use crate::errors::{AppError, AppResult};
use crate::models::AppData;
use crate::oauth::OauthFlow;

pub struct SharedState {
    pub data: Mutex<AppData>,
    pub startup_error: Mutex<Option<String>>,
    pub startup_warnings: Mutex<Vec<String>>,
    pub flows: Mutex<HashMap<String, OauthFlow>>,
    pub auto_refresh: Mutex<AutoRefreshRuntime>,
    pub last_refresh_result: Mutex<Option<RefreshAllResult>>,
    pub callback_server_started: AtomicBool,
    pub account_update_gates: Mutex<HashMap<String, Weak<tauri::async_runtime::Mutex<()>>>>,
    pub refresh_all_gate: tauri::async_runtime::Mutex<()>,
    pub app_handle: OnceLock<tauri::AppHandle>,
    pub quota_alert_levels: Mutex<HashMap<String, u8>>,
    pub is_quitting: AtomicBool,
    pub http_client: Client,
    pub available_update: Mutex<Option<crate::portable_updater::UpdateManifest>>,
    pub commit_gate: Mutex<()>,
}

impl SharedState {
    pub fn new_with_startup_error(
        initial: AppData,
        startup_error: Option<String>,
    ) -> AppResult<Self> {
        let http_client = Client::builder()
            .timeout(Duration::from_secs(45))
            .build()
            .map_err(|source| AppError::Http {
                context: "Failed to create HTTP client",
                source,
            })?;

        Ok(Self {
            auto_refresh: Mutex::new(AutoRefreshRuntime::new(&initial.app_settings)),
            last_refresh_result: Mutex::new(None),
            data: Mutex::new(initial),
            startup_error: Mutex::new(startup_error),
            startup_warnings: Mutex::new(Vec::new()),
            flows: Mutex::new(HashMap::new()),
            callback_server_started: AtomicBool::new(false),
            account_update_gates: Mutex::new(HashMap::new()),
            refresh_all_gate: tauri::async_runtime::Mutex::new(()),
            app_handle: OnceLock::new(),
            quota_alert_levels: Mutex::new(HashMap::new()),
            is_quitting: AtomicBool::new(false),
            http_client,
            available_update: Mutex::new(None),
            commit_gate: Mutex::new(()),
        })
    }
}

pub fn account_update_gate(
    state: &Arc<SharedState>,
    key: impl Into<String>,
) -> AppResult<Arc<tauri::async_runtime::Mutex<()>>> {
    let key = key.into();
    let mut gates = state
        .account_update_gates
        .lock()
        .map_err(|_| AppError::msg("State lock poisoned (account update gates)"))?;
    gates.retain(|_, gate| gate.strong_count() > 0);
    if let Some(gate) = gates.get(&key).and_then(Weak::upgrade) {
        return Ok(gate);
    }

    let gate = Arc::new(tauri::async_runtime::Mutex::new(()));
    gates.insert(key, Arc::downgrade(&gate));
    Ok(gate)
}

pub fn lock_auto_refresh(
    state: &Arc<SharedState>,
) -> AppResult<MutexGuard<'_, AutoRefreshRuntime>> {
    state
        .auto_refresh
        .lock()
        .map_err(|_| AppError::msg("State lock poisoned (auto refresh)"))
}

pub fn lock_last_refresh_result(
    state: &Arc<SharedState>,
) -> AppResult<MutexGuard<'_, Option<RefreshAllResult>>> {
    state
        .last_refresh_result
        .lock()
        .map_err(|_| AppError::msg("State lock poisoned (last refresh result)"))
}

pub fn lock_data(state: &Arc<SharedState>) -> AppResult<MutexGuard<'_, AppData>> {
    state
        .data
        .lock()
        .map_err(|_| AppError::msg("State lock poisoned (data)"))
}

pub fn lock_flows(
    state: &Arc<SharedState>,
) -> AppResult<MutexGuard<'_, HashMap<String, OauthFlow>>> {
    state
        .flows
        .lock()
        .map_err(|_| AppError::msg("State lock poisoned (oauth flows)"))
}

pub fn lock_startup_error(state: &Arc<SharedState>) -> AppResult<MutexGuard<'_, Option<String>>> {
    state
        .startup_error
        .lock()
        .map_err(|_| AppError::msg("State lock poisoned (startup recovery)"))
}

pub fn lock_available_update(
    state: &Arc<SharedState>,
) -> AppResult<MutexGuard<'_, Option<crate::portable_updater::UpdateManifest>>> {
    state
        .available_update
        .lock()
        .map_err(|_| AppError::msg("State lock poisoned (available update)"))
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use crate::models::AppData;

    use super::SharedState;

    #[test]
    fn account_update_gate_serializes_only_the_same_account() {
        let state = Arc::new(
            SharedState::new_with_startup_error(AppData::default(), None).expect("create state"),
        );
        let first_gate = super::account_update_gate(&state, "first").expect("create first gate");
        let first = first_gate.try_lock().expect("acquire first update guard");

        assert!(first_gate.try_lock().is_err());
        let other_gate = super::account_update_gate(&state, "other").expect("create other gate");
        assert!(other_gate.try_lock().is_ok());
        drop(first);
        assert!(first_gate.try_lock().is_ok());
    }
}
