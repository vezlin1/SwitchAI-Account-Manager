use std::collections::HashSet;
use std::sync::Arc;
use tauri::State;

use crate::app_state::{SharedState, lock_data, lock_data_for_update, lock_startup_error};
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
            auto_refresh: None,
            warnings,
            recovery: Some(RecoveryStatusDto {
                error,
                data_directory: recovery.data_directory.display().to_string(),
                state_path: recovery.state_path.display().to_string(),
                backup_available: recovery.backup_available,
            }),
        });
    }

    let auto_refresh = AutoRefreshStatusDto::from(&auto_refresh::snapshot(state)?);
    Ok(StartupStatusDto {
        mode: "ready".to_string(),
        state: Some(AppDataDto::from(&*lock_data(state)?)),
        auto_refresh: Some(auto_refresh),
        warnings,
        recovery: None,
    })
}

#[tauri::command]
pub async fn get_app_state(state: State<'_, Arc<SharedState>>) -> Result<AppDataDto, IpcErrorDto> {
    let state = Arc::clone(state.inner());
    command_result(
        run_blocking(move || {
            if let Some(error) = lock_startup_error(&state)?.as_ref() {
                return Err(AppError::msg(error.clone()));
            }
            let data = lock_data(&state)?;
            Ok(AppDataDto::from(&*data))
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
    let mut current = lock_data_for_update(state)?;
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
pub async fn restore_state_backup(
    state: State<'_, Arc<SharedState>>,
) -> Result<StartupStatusDto, IpcErrorDto> {
    let state = Arc::clone(state.inner());
    command_result(
        run_blocking(move || {
            let mut recovered = crate::storage::restore_app_data_backup()?;
            reconcile_recovered_auth(&state, &mut recovered);
            install_recovered_state(&state, recovered)?;
            startup_status(&state)
        })
        .await,
    )
}

#[tauri::command]
pub async fn start_fresh(
    state: State<'_, Arc<SharedState>>,
) -> Result<StartupStatusDto, IpcErrorDto> {
    let state = Arc::clone(state.inner());
    command_result(
        run_blocking(move || {
            let mut recovered = crate::storage::start_fresh_app_data()?;
            reconcile_recovered_auth(&state, &mut recovered);
            install_recovered_state(&state, recovered)?;
            startup_status(&state)
        })
        .await,
    )
}

#[tauri::command]
pub async fn open_recovery_data_directory() -> Result<(), IpcErrorDto> {
    command_result(
        run_blocking(move || {
            let path = crate::storage::app_storage_dir()?;
            let path = path
                .to_str()
                .ok_or_else(|| AppError::msg("Application data path is not valid Unicode"))?;
            shell::open_target(path, "Failed to open application data directory")
        })
        .await,
    )
}

#[tauri::command]
pub async fn get_auto_refresh_status(
    state: State<'_, Arc<SharedState>>,
) -> Result<AutoRefreshStatusDto, IpcErrorDto> {
    let state = Arc::clone(state.inner());
    command_result(
        run_blocking(move || {
            auto_refresh::snapshot(&state).map(|status| AutoRefreshStatusDto::from(&status))
        })
        .await,
    )
}

#[tauri::command]
pub async fn set_app_settings(
    settings: AppSettingsDto,
    state: State<'_, Arc<SharedState>>,
) -> Result<AppDataDto, IpcErrorDto> {
    let state = Arc::clone(state.inner());
    command_result(
        run_blocking(move || {
            let mut settings = crate::models::AppSettings::from(settings).normalized();
            let (data, next, restart_scheduler) = {
                let data = lock_data_for_update(&state)?;
                if !settings.hidden_account_ids.is_empty() {
                    let account_ids: HashSet<_> = data
                        .accounts
                        .iter()
                        .map(|account| account.id.as_str())
                        .collect();
                    settings
                        .hidden_account_ids
                        .retain(|account_id| account_ids.contains(account_id.as_str()));
                }
                if settings == data.app_settings {
                    return Ok(AppDataDto::from(&*data));
                }
                let restart_scheduler = settings.auto_refresh_enabled
                    != data.app_settings.auto_refresh_enabled
                    || settings.auto_refresh_interval_minutes
                        != data.app_settings.auto_refresh_interval_minutes
                    || settings.skip_unsupported_region_refresh
                        != data.app_settings.skip_unsupported_region_refresh;
                let mut next = data.clone();
                next.app_settings = settings;
                (data, next, restart_scheduler)
            };

            let committed = commit_state_data(&state, data, next)?;
            if restart_scheduler {
                let status = auto_refresh::restart(&state)?;
                auto_refresh::emit_status(&state, &status);
            }
            Ok(AppDataDto::from(&committed))
        })
        .await,
    )
}

#[tauri::command]
pub async fn open_external_url(url: String) -> Result<(), IpcErrorDto> {
    command_result(run_blocking(move || shell::open_external_url(&url)).await)
}
