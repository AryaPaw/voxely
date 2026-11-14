#[path = "build_support/build_date.rs"]
mod build_date;

fn main() {
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        // The test harness explicitly links the same Tauri resource library as
        // the application, including icons, version and the activation manifest.
        let out_dir = std::env::var_os("OUT_DIR").expect("Cargo sets OUT_DIR for build scripts");
        println!(
            "cargo:rustc-link-search=native={}",
            std::path::Path::new(&out_dir).display()
        );
    }

    println!("cargo:rerun-if-env-changed=VOXELY_BUILD_DATE");
    println!("cargo:rerun-if-env-changed=SOURCE_DATE_EPOCH");
    println!("cargo:rerun-if-env-changed=VOXELY_LOCAL_BUILD");
    println!("cargo:rerun-if-changed=windows.manifest.xml");
    println!(
        "cargo:rustc-env=VOXELY_LOCAL_BUILD={}",
        match std::env::var("VOXELY_LOCAL_BUILD") {
            Ok(value) if matches!(value.as_str(), "1" | "true" | "TRUE") => "1",
            _ => "0",
        }
    );
    let repo = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
    build_date::emit_git_dependencies(&repo);
    println!(
        "cargo:rustc-env=VOXELY_BUILD_DATE={}",
        build_date::resolve_build_date(&repo)
    );
    let overlay = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../dist/overlay.html");
    if std::env::var("TAURI_ENV_PLATFORM").is_ok() {
        let html = std::fs::read_to_string(&overlay)
            .unwrap_or_else(|_| panic!("overlay.html missing from dist"));
        if !html.contains("overlay-") && !html.contains("/src/overlay.tsx") {
            panic!("dist/overlay.html is not the overlay entry");
        }
    } else if overlay.exists() {
        let html = std::fs::read_to_string(&overlay).unwrap_or_default();
        if !html.contains("overlay-") && !html.contains("/src/overlay.tsx") {
            panic!("dist/overlay.html is not the overlay entry");
        }
    }
    tauri_build::try_build(
        tauri_build::Attributes::new()
            .windows_attributes(
                tauri_build::WindowsAttributes::new()
                    .app_manifest(include_str!("windows.manifest.xml")),
            )
            .app_manifest(tauri_build::AppManifest::new().commands(&[
                "get_session_state",
                "get_overlay_snapshot",
                "overlay_pointer_matches",
                "get_settings",
                "save_settings",
                "export_settings_file",
                "read_settings_file",
                "import_settings",
                "acknowledge_first_run_disclosure",
                "preview_retention_settings",
                "apply_retention_settings",
                "manual_reprocess",
                "list_history",
                "list_history_summaries",
                "get_usage_statistics",
                "get_recording",
                "search_history",
                "delete_history_item",
                "delete_all_history",
                "list_microphones",
                "get_meter",
                "start_input_meter",
                "stop_input_meter",
                "api_key_configured",
                "store_api_key",
                "test_openrouter",
                "discover_models",
                "toggle_dictation",
                "retry_recording",
                "cancel_history_retry",
                "open_logs",
                "open_settings_dir",
                "reset_settings",
                "open_audio_dir",
                "copy_transcript",
                "run_retention",
                "overlay_timing",
                "overlay_timeline",
                "overlay_mark_frame",
                "preview_dsp",
                "start_filter_sample",
                "stop_filter_sample",
                "check_for_updates",
                "install_update",
                "show_system_notification",
                "recording_audio_url",
                "recording_api_audio",
                "cancel_dictation",
                "cancel_overlay_dictation",
                "set_hotkey_capture",
                "open_github",
                "open_openrouter_models",
                "reset_main_window",
                "start_model_compare",
                "stop_model_compare",
                "run_model_compare",
                "get_model_compare",
                "clear_model_compare",
                "cancel_model_compare",
                "get_runtime_info",
                "play_cue",
                "preview_error_notification",
            ])),
    )
    .expect("tauri build");
}
