pub mod coordinator;
pub mod feed;
pub mod launcher;
pub mod policy;

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use tauri::{AppHandle, Manager};

use crate::app::session::AppContext;
use crate::settings::AppSettings;

use coordinator::{manual_copy, run_installed_pass, SilentUpdateOutcome};
use feed::{build_client, is_reachable};

pub fn current_version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

pub fn application_directory() -> Option<PathBuf> {
    std::env::current_exe()
        .ok()
        .and_then(|path| path.parent().map(PathBuf::from))
}

pub fn resolved_ui_locale(settings: &AppSettings) -> String {
    match settings.ui_language.as_str() {
        "en" => "en".into(),
        "ru" => "ru".into(),
        _ => "ru".into(),
    }
}

pub async fn check_updates(app: &AppHandle, force: bool) -> SilentUpdateOutcome {
    let ctx = app.state::<Arc<AppContext>>();
    let mut gate = ctx.update_gate.lock().await;
    if *gate {
        return SilentUpdateOutcome::Busy;
    }
    *gate = true;
    drop(gate);
    let outcome = run_check(app, force).await;
    *ctx.update_gate.lock().await = false;
    if outcome == SilentUpdateOutcome::Applied {
        app.exit(0);
    }
    outcome
}

async fn run_check(app: &AppHandle, force: bool) -> SilentUpdateOutcome {
    let ctx = app.state::<Arc<AppContext>>();
    let settings = ctx.settings.lock().clone();
    let auto = force || settings.auto_update_enabled;
    let Some(app_dir) = application_directory() else {
        return SilentUpdateOutcome::Skipped;
    };
    let process_name = std::env::current_exe()
        .ok()
        .and_then(|path| {
            path.file_stem()
                .map(|name| name.to_string_lossy().into_owned())
        })
        .unwrap_or_else(|| "voxely".into());
    let Ok(client) = build_client(Duration::from_secs(20), true) else {
        return SilentUpdateOutcome::Failed;
    };
    let download_directory = std::env::temp_dir()
        .join("Voxely")
        .join("updates")
        .join(uuid::Uuid::new_v4().to_string());
    let online = is_reachable(&client).await;
    run_installed_pass(
        &client,
        auto,
        current_version(),
        &process_name,
        &app_dir,
        &download_directory,
        online,
    )
    .await
}

pub fn spawn_background_loop(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        loop {
            tokio::time::sleep(Duration::from_secs(15)).await;
            let enabled = {
                let ctx = app.state::<Arc<AppContext>>();
                let enabled = ctx.settings.lock().auto_update_enabled;
                enabled
            };
            if !enabled {
                tokio::time::sleep(Duration::from_secs(120)).await;
                continue;
            }
            let outcome = check_updates(&app, false).await;
            match outcome {
                SilentUpdateOutcome::Applied | SilentUpdateOutcome::NoUpdate => return,
                SilentUpdateOutcome::Skipped => return,
                _ => tokio::time::sleep(coordinator::delay_after(outcome)).await,
            }
        }
    });
}

pub fn outcome_message(outcome: SilentUpdateOutcome, ui: &str) -> String {
    if matches!(outcome, SilentUpdateOutcome::Busy) && ui != "en" {
        return "Проверка уже идёт.".into();
    }
    if matches!(outcome, SilentUpdateOutcome::Busy) {
        return "A check is already running.".into();
    }
    manual_copy(outcome, ui)
}
