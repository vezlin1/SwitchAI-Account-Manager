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
            staged_ready: false,
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
                staged_ready: false,
            });
        }

        let download_size = manifest.platforms.get(platform_key).and_then(|p| p.size);
        let version = manifest.version.clone();
        let release_date = manifest.release_date.clone();
        let release_notes = manifest.notes.clone();
        // Never inspect/clean a partially written stage while a download or
        // installation owns it. The download reports readiness when finished.
        let staged_ready = if let Ok(_update_gate) = state.update_install_gate.try_lock() {
            crate::portable_updater::staged_update_matches_manifest(
                crate::portable_updater::read_staged_update_metadata()?.as_ref(),
                &manifest,
            )
        } else {
            false
        };

        *crate::app_state::lock_available_update(state)? = Some(manifest);
        crate::tray_dashboard::refresh_dashboard(state);

        Ok(UpdateCheckResultDto {
            update_available: true,
            version,
            current_version: current_version.to_string(),
            release_date,
            release_notes,
            download_size,
            staged_ready,
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
            staged_ready: false,
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
    expected_version: String,
) -> Result<bool, IpcErrorDto> {
    #[cfg(target_os = "macos")]
    {
        let _ = (app, state, expected_version);
        return Err(IpcErrorDto::from(crate::errors::AppError::msg(
            "Automatic in-place updates are not supported on macOS to prevent code signature invalidation. Please download the latest DMG from GitHub Releases.",
        )));
    }
    #[cfg(not(target_os = "macos"))]
    command_result(
        async {
            let _update_gate = state.update_install_gate.lock().await;
            if state.is_quitting.load(std::sync::atomic::Ordering::SeqCst) {
                return Err(crate::errors::AppError::msg(
                    "SwitchAI is already restarting.",
                ));
            }
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

            crate::portable_updater::validate_update_version(&manifest, &expected_version)?;

            let platform_key = crate::portable_updater::current_platform_key();
            let platform = manifest.platforms.get(platform_key).ok_or_else(|| {
                crate::errors::AppError::msg(format!(
                    "Current platform ({platform_key}) not found in update manifest"
                ))
            })?;

            let pubkey = crate::portable_updater::get_update_public_key();

            let request = crate::portable_updater::UpdateDownloadRequest {
                download_url: &platform.url,
                expected_size: platform.size,
                signature: &platform.signature,
                expected_sha256: platform.sha256.as_deref(),
                public_key: pubkey,
                version: &manifest.version,
            };
            crate::portable_updater::download_and_stage_update(&app, &state.http_client, &request)
                .await?;

            let available = crate::app_state::lock_available_update(state.inner())?;
            let current = available.as_ref().ok_or_else(|| {
                crate::errors::AppError::msg(
                    "The update selection changed during download. Check for updates again.",
                )
            })?;
            crate::portable_updater::validate_update_version(current, &expected_version)?;
            if !crate::portable_updater::staged_update_matches_manifest(
                crate::portable_updater::read_staged_update_metadata()?.as_ref(),
                current,
            ) {
                return Err(crate::errors::AppError::msg(
                    "The available release changed during download. Download the update again.",
                ));
            }

            Ok(true)
        }
        .await,
    )
}

#[tauri::command]
pub async fn install_update_and_restart(
    app: tauri::AppHandle,
    state: State<'_, Arc<SharedState>>,
    expected_version: String,
) -> Result<(), IpcErrorDto> {
    #[cfg(target_os = "macos")]
    {
        let _ = (app, state, expected_version);
        return Err(IpcErrorDto::from(crate::errors::AppError::msg(
            "Automatic in-place updates are not supported on macOS to prevent code signature invalidation. Please download the latest DMG from GitHub Releases.",
        )));
    }
    #[cfg(not(target_os = "macos"))]
    command_result(
        crate::portable_updater::perform_atomic_swap_and_restart(
            &app,
            state.inner(),
            &expected_version,
        )
        .await,
    )
}

pub fn dismiss_update_version_internal(
    state: &Arc<SharedState>,
    version: &str,
) -> AppResult<AppDataDto> {
    let (current, next) = {
        let current = lock_data(state)?;
        let mut next = current.clone();
        next.app_settings.ignored_update_version = Some(version.to_string());
        (current, next)
    };
    *crate::app_state::lock_available_update(state)? = None;
    let committed = commit_state_data(state, current, next)?;

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
