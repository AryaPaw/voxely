fn main() {
    println!("cargo:rerun-if-env-changed=VOXELY_BUILD_DATE");
    println!("cargo:rerun-if-env-changed=SOURCE_DATE_EPOCH");
    println!("cargo:rerun-if-env-changed=VOXELY_LOCAL_BUILD");
    println!(
        "cargo:rustc-env=VOXELY_LOCAL_BUILD={}",
        match std::env::var("VOXELY_LOCAL_BUILD") {
            Ok(value) if matches!(value.as_str(), "1" | "true" | "TRUE") => "1",
            _ => "0",
        }
    );
    let git_head = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../.git/HEAD");
    if git_head.exists() {
        println!("cargo:rerun-if-changed={}", git_head.display());
    }
    println!("cargo:rustc-env=VOXELY_BUILD_DATE={}", resolve_build_date());
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
    tauri_build::try_build(tauri_build::Attributes::new().app_manifest(
        tauri_build::AppManifest::new().commands(&[
            "get_session_state",
            "get_overlay_snapshot",
            "get_settings",
            "save_settings",
            "list_history",
            "list_history_summaries",
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
            "cancel_dictation",
            "set_hotkey_capture",
            "open_github",
            "start_model_compare",
            "stop_model_compare",
            "run_model_compare",
            "get_model_compare",
            "clear_model_compare",
            "cancel_model_compare",
            "get_runtime_info",
            "play_cue",
            "preview_error_notification",
        ]),
    ))
    .expect("tauri build");
}

fn resolve_build_date() -> String {
    if let Ok(value) = std::env::var("VOXELY_BUILD_DATE") {
        if is_iso_day(&value) {
            return value;
        }
    }
    if let Ok(epoch) = std::env::var("SOURCE_DATE_EPOCH") {
        if let Ok(secs) = epoch.parse::<i64>() {
            if let Some(day) = unix_day(secs) {
                return day;
            }
        }
    }
    let repo = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
    if let Ok(output) = std::process::Command::new("git")
        .args([
            "-C",
            repo.to_str().unwrap_or("."),
            "log",
            "-1",
            "--format=%cs",
        ])
        .output()
    {
        if output.status.success() {
            let day = String::from_utf8_lossy(&output.stdout).trim().to_string();
            if is_iso_day(&day) {
                return day;
            }
        }
    }
    unix_day(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0),
    )
    .unwrap_or_else(|| "1970-01-01".into())
}

fn is_iso_day(value: &str) -> bool {
    let bytes = value.as_bytes();
    bytes.len() == 10
        && bytes[4] == b'-'
        && bytes[7] == b'-'
        && bytes.iter().enumerate().all(|(i, b)| {
            if i == 4 || i == 7 {
                true
            } else {
                b.is_ascii_digit()
            }
        })
}

fn unix_day(secs: i64) -> Option<String> {
    let days = secs.div_euclid(86_400);
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097) as u32;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    Some(format!("{y:04}-{m:02}-{d:02}"))
}
