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
mod obs;
mod settings;
mod transcription;
mod updates;
mod windows_int;

use std::path::Path;
use std::sync::Arc;

use tauri::Manager;
use tauri_plugin_autostart::MacosLauncher;
use tracing_subscriber::EnvFilter;

use crate::app::lifecycle::{
    apply_launch_visibility, attach_context, configure_tray, hide_main_to_tray, reregister_hotkey,
    should_hide_on_launch, show_main, sync_autostart,
};
use crate::app::session::AppContext;
use crate::commands::*;

fn init_logging(debug: bool, log_dir: Option<&Path>) {
    let filter = if debug {
        "info,voxely_lib=debug"
    } else {
        "warn,voxely_lib=info"
    };
    let subscriber = tracing_subscriber::fmt().with_env_filter(EnvFilter::new(filter));
    if let Some(dir) = log_dir {
        let _ = std::fs::create_dir_all(dir);
        let file_appender = match crate::logging::file_appender(dir) {
            Ok(appender) => appender,
            Err(_) => {
                let _ = subscriber.try_init();
                return;
            }
        };
        let _ = subscriber
            .with_ansi(false)
            .with_writer(file_appender)
            .try_init();
    } else {
        let _ = subscriber.try_init();
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, args, _cwd| {
            if should_hide_on_launch(&args) {
                return;
            }
            show_main(app, "/");
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
            init_logging(debug, Some(&logs));
            app.manage(ctx);
            configure_tray(app.handle())?;
            sync_autostart(app.handle(), start_with_windows);
            crate::updates::spawn_background_loop(app.handle().clone());
            if let Err(err) = reregister_hotkey(app.handle(), &hotkey) {
                tracing::error!(error = %err, "hotkey failed");
                crate::notify::show_error(app.handle(), &err);
            }
            apply_launch_visibility(app.handle(), std::env::args());
            if let Some(window) = app.get_webview_window("main") {
                let handle = app.handle().clone();
                window.on_window_event(move |event| {
                    if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                        let ctx = handle.state::<Arc<AppContext>>();
                        if ctx.settings.lock().close_to_tray {
                            api.prevent_close();
                            hide_main_to_tray(&handle);
                        }
                    }
                });
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_session_state,
            get_settings,
            save_settings,
            list_history,
            list_history_summaries,
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
            open_logs,
            open_settings_dir,
            reset_settings,
            open_audio_dir,
            preview_obs_import,
            import_obs_preset,
            parse_obs_json,
            copy_transcript,
            insert_transcript,
            run_retention,
            overlay_timing,
            overlay_timeline,
            overlay_mark_frame,
            preview_dsp,
            start_filter_sample,
            stop_filter_sample,
            check_for_updates,
            recording_audio_url,
            cancel_dictation,
            set_hotkey_capture,
            open_github,
            start_model_compare,
            stop_model_compare,
            run_model_compare,
            get_model_compare,
            clear_model_compare,
            get_runtime_info,
            play_cue,
            preview_error_notification
        ])
        .run(tauri::generate_context!())
        .unwrap_or_else(|err| {
            tracing::error!(error = %err, "failed to start");
            std::process::exit(1);
        });
}
