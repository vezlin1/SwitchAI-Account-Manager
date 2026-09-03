use std::sync::Arc;
use tauri::State;

use crate::app_state::{SharedState, lock_data};
use crate::commands::command_result;
use crate::dto::{AppDataDto, UpdateCheckResultDto};
use crate::errors::{AppResult, IpcErrorDto};
use crate::storage::commit_state_data;

pub async fn check_for_updates_internal(
    state: &Arc<SharedState>,
    force: bool,
) -> AppResult<UpdateCheckResultDto> {
    let current_version = env!("CARGO_PKG_VERSION");
    let (auto_check, ignored_version) = {
        let data = lock_data(state)?;
        (
            data.app_settings.auto_check_updates,
            data.app_settings.ignored_update_version.clone(),
        )
    };

    if !force && !auto_check {
        return Ok(UpdateCheckResultDto {
            update_available: false,
            version: current_version.to_string(),
            current_version: current_version.to_string(),
            release_date: None,
            release_notes: None,
            download_size: None,
        });
    }

    let manifest_opt =
        crate::portable_updater::check_for_updates(&state.http_client, current_version).await?;

    if let Some(manifest) = manifest_opt {
        let platform_key = crate::portable_updater::current_platform_key();
        if !force && ignored_version.as_deref() == Some(&manifest.version) {
            *crate::app_state::lock_available_update(state)? = None;
            crate::tray_dashboard::refresh_dashboard(state);
            return Ok(UpdateCheckResultDto {
                update_available: false,
                version: manifest.version,
                current_version: current_version.to_string(),
                release_date: manifest.release_date,
                release_notes: manifest.notes,
                download_size: manifest.platforms.get(platform_key).and_then(|p| p.size),
            });
        }

        let download_size = manifest.platforms.get(platform_key).and_then(|p| p.size);
        let version = manifest.version.clone();
        let release_date = manifest.release_date.clone();
        let release_notes = manifest.notes.clone();

        *crate::app_state::lock_available_update(state)? = Some(manifest);
        crate::tray_dashboard::refresh_dashboard(state);

        Ok(UpdateCheckResultDto {
            update_available: true,
            version,
            current_version: current_version.to_string(),
            release_date,
            release_notes,
            download_size,
        })
    } else {
        *crate::app_state::lock_available_update(state)? = None;
        crate::tray_dashboard::refresh_dashboard(state);

        Ok(UpdateCheckResultDto {
            update_available: false,
            version: current_version.to_string(),
            current_version: current_version.to_string(),
            release_date: None,
            release_notes: None,
            download_size: None,
        })
    }
}

#[tauri::command]
pub async fn check_for_updates(
    state: State<'_, Arc<SharedState>>,
    force: Option<bool>,
) -> Result<UpdateCheckResultDto, IpcErrorDto> {
    command_result(check_for_updates_internal(state.inner(), force.unwrap_or(false)).await)
}

#[tauri::command]
pub async fn download_and_stage_update(
    app: tauri::AppHandle,
    state: State<'_, Arc<SharedState>>,
) -> Result<bool, IpcErrorDto> {
    #[cfg(target_os = "macos")]
    {
        let _ = (app, state);
        return Err(IpcErrorDto::from(crate::errors::AppError::msg(
            "Automatic in-place updates are not supported on macOS to prevent code signature invalidation. Please download the latest DMG from GitHub Releases.",
        )));
    }
    #[cfg(not(target_os = "macos"))]
    command_result(
        async {
            let manifest = {
                let update_opt = crate::app_state::lock_available_update(state.inner())?.clone();
                match update_opt {
                    Some(m) => m,
                    None => {
                        let res = check_for_updates_internal(state.inner(), true).await?;
                        if !res.update_available {
                            return Err(crate::errors::AppError::msg(
                                "No updates are available to download",
                            ));
                        }
                        crate::app_state::lock_available_update(state.inner())?
                            .clone()
                            .ok_or_else(|| {
                                crate::errors::AppError::msg(
                                    "Update manifest not found after check",
                                )
                            })?
                    }
                }
            };

            let platform_key = crate::portable_updater::current_platform_key();
            let platform = manifest.platforms.get(platform_key).ok_or_else(|| {
                crate::errors::AppError::msg(format!(
                    "Current platform ({platform_key}) not found in update manifest"
                ))
            })?;

            let pubkey = crate::portable_updater::get_update_public_key();

            crate::portable_updater::download_and_stage_update(
                &app,
                &state.http_client,
                &platform.url,
                platform.size,
                &platform.signature,
                platform.sha256.as_deref(),
                pubkey,
            )
            .await?;

            Ok(true)
        }
        .await,
    )
}

#[tauri::command]
pub async fn install_update_and_restart(
    app: tauri::AppHandle,
    state: State<'_, Arc<SharedState>>,
) -> Result<(), IpcErrorDto> {
    #[cfg(target_os = "macos")]
    {
        let _ = (app, state);
        return Err(IpcErrorDto::from(crate::errors::AppError::msg(
            "Automatic in-place updates are not supported on macOS to prevent code signature invalidation. Please download the latest DMG from GitHub Releases.",
        )));
    }
    #[cfg(not(target_os = "macos"))]
    command_result(
        crate::portable_updater::perform_atomic_swap_and_restart(&app, state.inner()).await,
    )
}

pub fn dismiss_update_version_internal(
    state: &Arc<SharedState>,
    version: &str,
) -> AppResult<AppDataDto> {
    let next = {
        let current = lock_data(state)?;
        let mut next = current.clone();
        next.app_settings.ignored_update_version = Some(version.to_string());
        next
    };
    let committed = commit_state_data(state, next)?;

    *crate::app_state::lock_available_update(state)? = None;
    crate::tray_dashboard::emit_state_changed(state, "settings", Vec::new());

    Ok(AppDataDto::from(&committed))
}

#[tauri::command]
pub fn dismiss_update_version(
    version: String,
    state: State<'_, Arc<SharedState>>,
) -> Result<AppDataDto, IpcErrorDto> {
    command_result(dismiss_update_version_internal(state.inner(), &version))
}
