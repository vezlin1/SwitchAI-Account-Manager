use std::sync::Arc;
use tauri::State;

use crate::app_state::{SharedState, lock_data, lock_startup_error};
use crate::auto_refresh;
use crate::commands::{command_result, run_blocking};
use crate::dto::{
    AppDataDto, AppSettingsDto, AutoRefreshStatusDto, RecoveryStatusDto, StartupStatusDto,
};
use crate::errors::{AppError, AppResult, IpcErrorDto};
use crate::models::AppData;
use crate::shell;
use crate::storage::commit_state_data;

fn startup_status(state: &Arc<SharedState>) -> AppResult<StartupStatusDto> {
    let warnings = state
        .startup_warnings
        .lock()
        .map_err(|_| AppError::msg("State lock poisoned (startup warnings)"))?
        .clone();
    if let Some(error) = lock_startup_error(state)?.clone() {
        let recovery = crate::storage::recovery_status()?;
        return Ok(StartupStatusDto {
            mode: "recovery_required".to_string(),
            state: None,
            warnings,
            recovery: Some(RecoveryStatusDto {
                error,
                data_directory: recovery.data_directory.display().to_string(),
                state_path: recovery.state_path.display().to_string(),
                backup_available: recovery.backup_available,
            }),
            auto_refresh: None,
        });
    }

    let app_data = {
        let data = lock_data(state)?;
        AppDataDto::from(&*data)
    };
    let auto_refresh = AutoRefreshStatusDto::from(&auto_refresh::snapshot(state)?);
    Ok(StartupStatusDto {
        mode: "ready".to_string(),
        state: Some(app_data),
        warnings,
        recovery: None,
        auto_refresh: Some(auto_refresh),
    })
}

#[tauri::command]
pub async fn get_app_state(
    state: State<'_, Arc<SharedState>>,
    reconcile_local_auth: Option<bool>,
) -> Result<AppDataDto, IpcErrorDto> {
    let state = Arc::clone(state.inner());
    command_result(
        run_blocking(move || {
            if let Some(error) = lock_startup_error(&state)?.as_ref() {
                return Err(AppError::msg(error.clone()));
            }
            let mut data = lock_data(&state)?;
            if reconcile_local_auth.unwrap_or(false) {
                if let Err(error) = crate::codex::reconcile_codex_auth_transactionally(&mut data) {
                    log::warn!(
                        "Local Codex session could not be reconciled: {}",
                        error.user_message()
                    );
                }
                if let Err(error) = crate::gemini::reconcile_antigravity_auth_at_startup(&mut data)
                {
                    log::warn!(
                        "Local Antigravity session could not be reconciled: {}",
                        error.user_message()
                    );
                }
            }
            let result = AppDataDto::from(&*data);
            drop(data);
            if reconcile_local_auth.unwrap_or(false) {
                auto_refresh::sync_schedule_runtime_status(&state)?;
                crate::tray_dashboard::refresh_dashboard(&state);
            }
            Ok(result)
        })
        .await,
    )
}

#[tauri::command]
pub async fn get_startup_status(
    state: State<'_, Arc<SharedState>>,
) -> Result<StartupStatusDto, IpcErrorDto> {
    let state = Arc::clone(state.inner());
    command_result(run_blocking(move || startup_status(&state)).await)
}

fn install_recovered_state(state: &Arc<SharedState>, mut recovered: AppData) -> AppResult<()> {
    let mut current = lock_data(state)?;
    recovered.revision = current.revision.saturating_add(1);
    *current = recovered;
    drop(current);
    *lock_startup_error(state)? = None;
    let _ = auto_refresh::restart(state)?;
    crate::tray_dashboard::refresh_dashboard(state);
    crate::tray_dashboard::emit_state_changed(state, "accounts", Vec::new());
    Ok(())
}

fn reconcile_recovered_auth(state: &Arc<SharedState>, recovered: &mut AppData) {
    if let Err(error) = crate::codex::reconcile_codex_auth_at_startup(recovered) {
        let message = format!(
            "Application state recovered, but auth.json could not be reconciled: {}",
            error.user_message()
        );
        log::warn!("{message}");
        if let Ok(mut warnings) = state.startup_warnings.lock() {
            warnings.push(message);
        }
    }
}

#[tauri::command]
pub fn restore_state_backup(
    state: State<'_, Arc<SharedState>>,
) -> Result<StartupStatusDto, IpcErrorDto> {
    command_result((|| {
        let mut recovered = crate::storage::restore_app_data_backup()?;
        reconcile_recovered_auth(state.inner(), &mut recovered);
        install_recovered_state(state.inner(), recovered)?;
        startup_status(state.inner())
    })())
}

#[tauri::command]
pub fn start_fresh(state: State<'_, Arc<SharedState>>) -> Result<StartupStatusDto, IpcErrorDto> {
    command_result((|| {
        let mut recovered = crate::storage::start_fresh_app_data()?;
        reconcile_recovered_auth(state.inner(), &mut recovered);
        install_recovered_state(state.inner(), recovered)?;
        startup_status(state.inner())
    })())
}

#[tauri::command]
pub fn open_recovery_data_directory() -> Result<(), IpcErrorDto> {
    command_result((|| {
        let path = crate::storage::app_storage_dir()?;
        let path = path
            .to_str()
            .ok_or_else(|| AppError::msg("Application data path is not valid Unicode"))?;
        shell::open_target(path, "Failed to open application data directory")
    })())
}

#[tauri::command]
pub fn migrate_privacy_mode(
    legacy_enabled: bool,
    state: State<'_, Arc<SharedState>>,
) -> Result<AppDataDto, IpcErrorDto> {
    command_result((|| {
        let data = lock_data(state.inner())?;
        if !data.app_settings.privacy_migration_pending {
            return Ok(AppDataDto::from(&*data));
        }
        let mut next = data.clone();
        next.app_settings.migrate_privacy(legacy_enabled);
        let committed = commit_state_data(state.inner(), data, next)?;
        crate::tray_dashboard::refresh_dashboard(state.inner());
        crate::tray_dashboard::emit_state_changed(state.inner(), "settings", Vec::new());
        Ok(AppDataDto::from(&committed))
    })())
}

#[tauri::command]
pub fn get_auto_refresh_status(
    state: State<'_, Arc<SharedState>>,
) -> Result<AutoRefreshStatusDto, IpcErrorDto> {
    command_result(
        auto_refresh::snapshot(state.inner()).map(|status| AutoRefreshStatusDto::from(&status)),
    )
}

#[tauri::command]
pub fn set_app_settings(
    settings: AppSettingsDto,
    state: State<'_, Arc<SharedState>>,
) -> Result<AppDataDto, IpcErrorDto> {
    command_result((|| {
        let mut settings = crate::models::AppSettings::from(settings).normalized();
        let (data, next) = {
            let data = lock_data(state.inner())?;
            settings.privacy_migration_pending = data.app_settings.privacy_migration_pending;
            settings.hidden_account_ids.retain(|account_id| {
                data.accounts
                    .iter()
                    .any(|account| &account.id == account_id)
            });
            let mut next = data.clone();
            next.app_settings = settings;
            (data, next)
        };

        let committed = commit_state_data(state.inner(), data, next)?;
        let _ = auto_refresh::restart(state.inner())?;
        Ok(AppDataDto::from(&committed))
    })())
}

#[tauri::command]
pub fn open_external_url(url: String) -> Result<(), IpcErrorDto> {
    command_result(shell::open_external_url(&url))
}

#[cfg(test)]
mod tests {
    use std::sync::mpsc;
    use std::time::Duration;

    use super::*;

    #[test]
    fn startup_returns_accounts_and_refresh_status() {
        let state = Arc::new(
            SharedState::new_with_startup_error(AppData::default(), None).expect("create state"),
        );
        let (sender, receiver) = mpsc::channel();
        std::thread::spawn(move || {
            let _ = sender.send(startup_status(&state));
        });
        let response = receiver
            .recv_timeout(Duration::from_secs(2))
            .expect("startup must release account data before taking the refresh snapshot")
            .expect("build startup response");
        assert_eq!(response.mode, "ready");
        assert!(response.state.is_some());
        assert!(response.auto_refresh.is_some());
    }
}
