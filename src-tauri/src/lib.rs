#![allow(dead_code)]
#![allow(clippy::too_many_arguments)]
#![allow(clippy::field_reassign_with_default)]

mod app;
mod audio;
mod commands;
pub mod dsp;
mod error;
mod history;
mod logging;
mod notify;
mod settings;
mod transcription;
mod updates;
mod windows_int;

use std::sync::Arc;

use tauri::Manager;
use tauri_plugin_autostart::MacosLauncher;

use crate::app::lifecycle::{
    apply_launch_visibility, attach_context, configure_tray, hide_main_to_tray, reregister_hotkey,
    should_hide_on_launch, show_main, sync_autostart,
};
use crate::app::session::{prepare_overlay_window, AppContext};
use crate::commands::*;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    crate::notify::apply_windows_app_identity();
    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, args, _cwd| {
            if should_hide_on_launch(&args) {
                return;
            }
            show_main(app, "history");
        }))
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_autostart::init(
            MacosLauncher::LaunchAgent,
            Some(vec!["--autostart"]),
        ))
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_process::init())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .setup(|app| {
            let ctx = attach_context(app.handle())?;
            let hotkey = ctx.settings.lock().hotkey.clone();
            let debug = ctx.settings.lock().debug_logging;
            let start_with_windows = ctx.settings.lock().start_with_windows;
            let logs = ctx.data_dir.join("logs");
            crate::logging::init(debug, Some(&logs));
            app.manage(ctx);
            configure_tray(app.handle())?;
            if let Err(err) = sync_autostart(app.handle(), start_with_windows) {
                tracing::error!(error = %err, "autostart failed");
            }
            crate::updates::spawn_background_loop(app.handle().clone());
            if let Err(err) = reregister_hotkey(app.handle(), &hotkey) {
                tracing::error!(error = %err, "hotkey failed");
                crate::notify::show_error(app.handle(), &err);
            }
            apply_launch_visibility(app.handle(), std::env::args());
            prepare_overlay_window(app.handle());
            if let Some(window) = app.get_webview_window("main") {
                let handle = app.handle().clone();
                window.on_window_event(move |event| {
                    if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                        let ctx = handle.state::<Arc<AppContext>>();
                        api.prevent_close();
                        if ctx.settings.lock().close_to_tray {
                            hide_main_to_tray(&handle);
                        } else {
                            crate::app::session::shutdown_session(&handle);
                            handle.exit(0);
                        }
                    }
                });
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_session_state,
            get_overlay_snapshot,
            get_settings,
            save_settings,
            list_history,
            list_history_summaries,
            search_history,
            get_recording,
            delete_history_item,
            delete_all_history,
            list_microphones,
            get_meter,
            start_input_meter,
            stop_input_meter,
            api_key_configured,
            store_api_key,
            test_openrouter,
            discover_models,
            toggle_dictation,
            retry_recording,
            cancel_history_retry,
            open_logs,
            open_settings_dir,
            reset_settings,
            open_audio_dir,
            copy_transcript,
            run_retention,
            overlay_timing,
            overlay_timeline,
            overlay_mark_frame,
            preview_dsp,
            start_filter_sample,
            stop_filter_sample,
            check_for_updates,
            install_update,
            show_system_notification,
            recording_audio_url,
            cancel_dictation,
            set_hotkey_capture,
            open_github,
            open_openrouter_models,
            reset_main_window,
            start_model_compare,
            stop_model_compare,
            run_model_compare,
            get_model_compare,
            clear_model_compare,
            cancel_model_compare,
            get_runtime_info,
            play_cue,
            preview_error_notification
        ])
        .build(tauri::generate_context!())
        .unwrap_or_else(|err| {
            tracing::error!(error = %err, "failed to start");
            std::process::exit(1);
        })
        .run(|app, event| {
            if let tauri::RunEvent::ExitRequested { .. } = event {
                crate::app::session::shutdown_session(app);
            }
        });
}
