use std::sync::Arc;
use tauri::State;

use crate::app_state::{SharedState, lock_data, lock_startup_error};
use crate::auto_refresh;
use crate::commands::command_result;
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
        });
    }

    Ok(StartupStatusDto {
        mode: "ready".to_string(),
        state: Some(AppDataDto::from(&*lock_data(state)?)),
        warnings,
        recovery: None,
    })
}

#[tauri::command]
pub fn get_app_state(state: State<'_, Arc<SharedState>>) -> Result<AppDataDto, IpcErrorDto> {
    command_result((|| {
        if let Some(error) = lock_startup_error(state.inner())?.as_ref() {
            return Err(AppError::msg(error.clone()));
        }
        let data = lock_data(state.inner())?;
        Ok(AppDataDto::from(&*data))
    })())
}

#[tauri::command]
pub fn get_startup_status(
    state: State<'_, Arc<SharedState>>,
) -> Result<StartupStatusDto, IpcErrorDto> {
    command_result(startup_status(state.inner()))
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
        let next = {
            let data = lock_data(state.inner())?;
            settings.hidden_account_ids.retain(|account_id| {
                data.accounts
                    .iter()
                    .any(|account| &account.id == account_id)
            });
            let mut next = data.clone();
            next.app_settings = settings;
            next
        };

        let committed = commit_state_data(state.inner(), next)?;
        let _ = auto_refresh::restart(state.inner())?;
        Ok(AppDataDto::from(&committed))
    })())
}

#[tauri::command]
pub fn open_external_url(url: String) -> Result<(), IpcErrorDto> {
    command_result(shell::open_external_url(&url))
}
