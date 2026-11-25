use std::collections::HashMap;
use std::ops::{Deref, DerefMut};
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex, MutexGuard, OnceLock, Weak};
use std::time::Duration;

use reqwest::Client;

use crate::auto_refresh::{AutoRefreshRuntime, RefreshAllResult};
use crate::errors::{AppError, AppResult};
use crate::models::AppData;
use crate::oauth::OauthFlow;

pub struct SharedState {
    // Writers serialize their complete transactions here. Readers use the last
    // published snapshot, so slow credential/disk writes never hold a read lock.
    data: Mutex<AppData>,
    published_data: Mutex<AppData>,
    pub startup_error: Mutex<Option<String>>,
    pub startup_warnings: Mutex<Vec<String>>,
    pub flows: Mutex<HashMap<String, OauthFlow>>,
    pub auto_refresh: Mutex<AutoRefreshRuntime>,
    last_refresh_result: Mutex<Option<Arc<RefreshAllResult>>>,
    pub callback_server_started: AtomicBool,
    pub account_update_gates: Mutex<HashMap<String, Weak<tauri::async_runtime::Mutex<()>>>>,
    pub refresh_all_gate: tauri::async_runtime::Mutex<()>,
    pub reachability_gate: tauri::async_runtime::Mutex<()>,
    pub app_handle: OnceLock<tauri::AppHandle>,
    pub quota_alert_levels: Mutex<HashMap<String, u8>>,
    pub is_quitting: AtomicBool,
    pub http_client: Client,
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
            published_data: Mutex::new(initial.clone()),
            data: Mutex::new(initial),
            startup_error: Mutex::new(startup_error),
            startup_warnings: Mutex::new(Vec::new()),
            flows: Mutex::new(HashMap::new()),
            callback_server_started: AtomicBool::new(false),
            account_update_gates: Mutex::new(HashMap::new()),
            refresh_all_gate: tauri::async_runtime::Mutex::new(()),
            reachability_gate: tauri::async_runtime::Mutex::new(()),
            app_handle: OnceLock::new(),
            quota_alert_levels: Mutex::new(HashMap::new()),
            is_quitting: AtomicBool::new(false),
            http_client,
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
    if let Some(gate) = gates.get(&key).and_then(Weak::upgrade) {
        return Ok(gate);
    }
    // Reap abandoned keys as the map grows instead of scanning live gates for
    // every account registered by the same refresh run.
    if gates.len().is_power_of_two() {
        gates.retain(|_, gate| gate.strong_count() > 0);
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
) -> AppResult<MutexGuard<'_, Option<Arc<RefreshAllResult>>>> {
    state
        .last_refresh_result
        .lock()
        .map_err(|_| AppError::msg("State lock poisoned (last refresh result)"))
}

/// Read access is immutable; all mutations must use a serialized transaction.
pub struct DataReadGuard<'a>(MutexGuard<'a, AppData>);

impl Deref for DataReadGuard<'_> {
    type Target = AppData;

    fn deref(&self) -> &AppData {
        &self.0
    }
}

pub fn lock_data(state: &Arc<SharedState>) -> AppResult<DataReadGuard<'_>> {
    state
        .published_data
        .lock()
        .map(DataReadGuard)
        .map_err(|_| AppError::msg("State lock poisoned (data)"))
}

/// A serialized state transaction. Publication follows the existing revision
/// contract: failed persistence leaves the revision and readable state intact.
pub struct DataUpdateGuard<'a> {
    current: MutexGuard<'a, AppData>,
    published: &'a Mutex<AppData>,
    original_revision: u64,
}

impl Deref for DataUpdateGuard<'_> {
    type Target = AppData;

    fn deref(&self) -> &AppData {
        &self.current
    }
}

impl DerefMut for DataUpdateGuard<'_> {
    fn deref_mut(&mut self) -> &mut AppData {
        &mut self.current
    }
}

impl Drop for DataUpdateGuard<'_> {
    fn drop(&mut self) {
        if self.current.revision == self.original_revision {
            return;
        }
        // Clone before taking the snapshot mutex. Only the pointer-rich state
        // replacement happens while readers are excluded.
        let snapshot = self.current.clone();
        match self.published.lock() {
            Ok(mut published) => *published = snapshot,
            Err(error) => {
                // Recover a poisoned read snapshot rather than leave a durable
                // write invisible. No mutation logic runs inside this mutex.
                *error.into_inner() = snapshot;
                self.published.clear_poison();
                log::warn!("Recovered a poisoned published state snapshot");
            }
        }
    }
}

pub fn lock_data_for_update(state: &Arc<SharedState>) -> AppResult<DataUpdateGuard<'_>> {
    let current = state
        .data
        .lock()
        .map_err(|_| AppError::msg("State lock poisoned (data transaction)"))?;
    let original_revision = current.revision;
    Ok(DataUpdateGuard {
        current,
        published: &state.published_data,
        original_revision,
    })
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

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::sync::mpsc;
    use std::time::Duration;

    use crate::models::AppData;

    use super::SharedState;

    #[test]
    fn ui_readers_stay_responsive_until_a_transaction_is_published() {
        let state = Arc::new(
            SharedState::new_with_startup_error(AppData::default(), None).expect("create state"),
        );
        let mut writer = super::lock_data_for_update(&state).expect("start write transaction");
        writer.app_settings.privacy_mode = true;
        writer.revision += 1;
        let (snapshot_tx, snapshot_rx) = mpsc::channel();
        let reader_state = state.clone();
        let reader = std::thread::spawn(move || {
            let snapshot = super::lock_data(&reader_state)
                .expect("read published snapshot")
                .clone();
            snapshot_tx.send(snapshot).expect("send snapshot");
        });

        // A slow writer must not make the UI wait for persistence. It observes
        // the previous complete transaction until the writer releases its guard.
        let observed = snapshot_rx.recv_timeout(Duration::from_secs(2));
        drop(writer);
        reader.join().expect("reader completed");
        let observed = observed.expect("reader must complete while transaction is held");
        assert!(!observed.app_settings.privacy_mode);
        assert_eq!(observed.revision, 0);
        let committed = super::lock_data(&state).expect("read committed snapshot");
        assert!(committed.app_settings.privacy_mode);
        assert_eq!(committed.revision, 1);
    }

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
