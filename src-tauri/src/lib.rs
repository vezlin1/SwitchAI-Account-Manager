pub mod providers;
pub use providers::chatgpt::oauth;
pub use providers::chatgpt::quota;
pub use providers::chatgpt::sync as codex;
pub use providers::gemini::oauth as oauth_gemini;
pub use providers::gemini::quota as gemini_quota;
pub use providers::gemini::sync as gemini;

mod app_state;
mod atomic_file;
mod auto_refresh;
mod commands;
mod dto;
mod errors;
mod geo;
mod models;
mod persisted;
pub mod portable_updater;
mod refresh_service;
mod secret_store;
mod shell;
mod storage;
mod subscription;
mod token_utils;
mod tray_dashboard;

use std::sync::Arc;
use std::sync::atomic::Ordering;

use app_state::SharedState;
use models::AppData;
use storage::load_app_data;
use tauri::{
    Manager, WindowEvent,
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
};

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    crate::portable_updater::handle_after_update_wait();
    let (mut initial_data, startup_error) = match load_app_data() {
        Ok(data) => (data, None),
        Err(err) => {
            let message = format!(
                "Failed to load saved accounts: {err}. The existing state files were preserved for recovery."
            );
            log::error!("{message}");
            (AppData::default(), Some(message))
        }
    };
    let mut startup_warnings = Vec::new();
    if startup_error.is_none() {
        if let Err(error) = codex::reconcile_codex_auth_at_startup(&mut initial_data) {
            startup_warnings.push(format!(
                "Could not reconcile the external Codex account: {}",
                error.user_message()
            ));
            log::warn!(
                "Failed to reconcile external Codex selection: {}",
                error.user_message()
            );
        }
        match gemini::reconcile_antigravity_auth_at_startup(&mut initial_data) {
            Ok(Some(warning)) => {
                log::warn!("{warning}");
                startup_warnings.push(warning);
            }
            Ok(None) => {}
            Err(error) => {
                let warning = format!(
                    "Could not reconcile the external Antigravity account: {}",
                    error.user_message()
                );
                log::warn!("{warning}");
                startup_warnings.push(warning);
            }
        }
    }
    let shared_state = Arc::new(
        SharedState::new_with_startup_error(initial_data, startup_error)
            .expect("failed to initialize shared application state"),
    );
    if let Ok(mut warnings) = shared_state.startup_warnings.lock() {
        *warnings = startup_warnings;
    }
    let setup_state = Arc::clone(&shared_state);
    let close_state = Arc::clone(&shared_state);

    let builder = tauri::Builder::default()
        .manage(Arc::clone(&shared_state))
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            show_main_window(app);
        }))
        .plugin(
            tauri_plugin_log::Builder::default()
                .level(log::LevelFilter::Info)
                .build(),
        );
    #[cfg(not(test))]
    let builder = builder.plugin(tauri_plugin_notification::init());

    builder
        .setup(move |app| {
            let _ = setup_state.app_handle.set(app.handle().clone());
            let initial = app_state::lock_data(&setup_state)?.clone();
            let menu = tray_dashboard::build_menu(app.handle(), &initial)
                .map_err(|error| error.user_message())?;

            let tray_state = Arc::clone(&setup_state);
            let tray_builder = TrayIconBuilder::with_id(tray_dashboard::TRAY_ID)
                .menu(&menu)
                .show_menu_on_left_click(false)
                .on_menu_event(move |app, event| {
                    let id = event.id().as_ref();
                    match id {
                        "show" => show_main_window(app),
                        "open_update" => {
                            show_main_window(app);
                            use tauri::Emitter;
                            let _ = app.emit("open-update-modal", ());
                        }
                        "refresh" => auto_refresh::request_refresh_now(&tray_state),
                        "quit" => {
                            tray_state.is_quitting.store(true, Ordering::SeqCst);
                            app.exit(0);
                        }
                        _ => {
                            if let Some(account_id) =
                                id.strip_prefix(tray_dashboard::SWITCH_CODEX_ACCOUNT_PREFIX)
                            {
                                match commands::set_active_account_data(account_id, &tray_state) {
                                    Ok(data) => {
                                        tray_dashboard::emit_state_changed(
                                            &tray_state,
                                            "accounts",
                                            vec![account_id.to_string()],
                                        );
                                        if let Some(account) = data
                                            .accounts
                                            .iter()
                                            .find(|account| account.id == account_id)
                                        {
                                            tray_dashboard::notify_account_selected(
                                                &tray_state,
                                                account,
                                            );
                                        }
                                        tray_dashboard::refresh_dashboard(&tray_state);
                                    }
                                    Err(error) => log::warn!(
                                        "Tray Codex account selection failed: {}",
                                        error.user_message()
                                    ),
                                }
                            } else if let Some(account_id) =
                                id.strip_prefix(tray_dashboard::SWITCH_GEMINI_ACCOUNT_PREFIX)
                            {
                                let selection_state = Arc::clone(&tray_state);
                                let account_id = account_id.to_string();
                                tauri::async_runtime::spawn(async move {
                                    match commands::set_active_gemini_account_data(
                                        &account_id,
                                        &selection_state,
                                    )
                                    .await
                                    {
                                        Ok(data) => {
                                            tray_dashboard::emit_state_changed(
                                                &selection_state,
                                                "accounts",
                                                vec![account_id.clone()],
                                            );
                                            if let Some(account) = data
                                                .accounts
                                                .iter()
                                                .find(|account| account.id == account_id)
                                            {
                                                tray_dashboard::notify_account_selected(
                                                    &selection_state,
                                                    account,
                                                );
                                            }
                                            tray_dashboard::refresh_dashboard(&selection_state);
                                        }
                                        Err(error) => log::warn!(
                                            "Tray Antigravity account selection failed: {}",
                                            error.user_message()
                                        ),
                                    }
                                });
                            }
                        }
                    }
                })
                .on_tray_icon_event(|tray, event| {
                    if let TrayIconEvent::Click {
                        button: MouseButton::Left,
                        button_state: MouseButtonState::Up,
                        ..
                    } = event
                    {
                        show_main_window(tray.app_handle());
                    }
                });

            let tray_builder = if let Some(icon) = app.default_window_icon() {
                let b = tray_builder.icon(icon.clone());
                #[cfg(target_os = "macos")]
                let b = b.icon_as_template(true);
                b
            } else {
                tray_builder
            };
            tray_builder.build(app)?;

            #[cfg(target_os = "macos")]
            {
                if let Ok(app_menu) = tauri::menu::Menu::default(app.handle()) {
                    let _ = app.set_menu(app_menu);
                }
            }

            #[cfg(target_os = "windows")]
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.set_decorations(false);
            }
            if app_state::lock_startup_error(&setup_state)?.is_none() {
                let _ = auto_refresh::start(&setup_state);

                let bg_state = Arc::clone(&setup_state);
                tauri::async_runtime::spawn(async move {
                    tokio::time::sleep(std::time::Duration::from_secs(15)).await;
                    let should_check = crate::app_state::lock_data(&bg_state)
                        .map(|d| d.app_settings.auto_check_updates)
                        .unwrap_or(true);
                    if should_check {
                        log::info!("Starting background update check...");
                        if let Ok(res) =
                            crate::commands::check_for_updates_internal(&bg_state, false).await
                            && res.update_available
                        {
                            log::info!("Update available: v{}", res.version);
                            if let Some(app) = bg_state.app_handle.get() {
                                use tauri::Emitter;
                                let _ = app.emit("update-available", &res);

                                #[cfg(not(test))]
                                {
                                    use tauri_plugin_notification::NotificationExt;
                                    let _ = app
                                        .notification()
                                        .builder()
                                        .title("SwitchAI Update Available")
                                        .body(format!(
                                            "Version v{} is available for download.",
                                            res.version
                                        ))
                                        .show();
                                }
                            }
                        }
                    }
                });
            }
            show_main_window(app.handle());

            Ok(())
        })
        .on_window_event(move |window, event| {
            if window.label() != "main" {
                return;
            }

            if let WindowEvent::CloseRequested { api, .. } = event {
                if close_state.is_quitting.load(Ordering::SeqCst) {
                    return;
                }

                let close_to_tray = app_state::lock_data(&close_state)
                    .map(|data| data.app_settings.close_to_tray)
                    .unwrap_or(false);

                if close_to_tray {
                    api.prevent_close();
                    let _ = window.hide();
                }
            }
        })
        .invoke_handler(tauri::generate_handler![
            commands::get_startup_status,
            commands::restore_state_backup,
            commands::start_fresh,
            commands::open_recovery_data_directory,
            commands::get_app_state,
            commands::get_account,
            commands::get_auto_refresh_status,
            commands::set_app_settings,
            commands::start_oauth_flow,
            commands::get_oauth_flow_status,
            commands::cancel_oauth_flow,
            commands::open_external_url,
            commands::remove_account,
            commands::switch_active_account_and_restart_codex,
            commands::switch_active_gemini_account_and_restart_antigravity,
            commands::get_antigravity_surfaces,
            commands::import_antigravity_account,
            commands::import_codex_account,
            commands::set_account_order,
            commands::refresh_account_subscription,
            commands::refresh_account_quota,
            commands::refresh_all_quotas,
            commands::check_for_updates,
            commands::download_and_stage_update,
            commands::install_update_and_restart,
            commands::dismiss_update_version
        ])
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run({
            let run_state = Arc::clone(&shared_state);
            move |_app_handle, event| {
                if let tauri::RunEvent::ExitRequested { .. } = event {
                    run_state.is_quitting.store(true, Ordering::SeqCst);
                }
            }
        });
}

fn show_main_window(app: &tauri::AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.unminimize();
        let _ = window.show();
        let _ = window.set_focus();
    }
}
