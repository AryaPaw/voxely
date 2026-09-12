pub mod policy;

use std::sync::Arc;
use std::time::Duration;

use tauri::{AppHandle, Manager};
use tauri_plugin_updater::UpdaterExt;

use crate::app::machine::is_cancellable;
use crate::app::session::AppContext;

use policy::{install_allowed, is_newer_stable};

pub fn current_version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UpdateCode {
    None,
    Installed,
    Busy,
    Deferred,
    Failed,
}

impl UpdateCode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Installed => "installed",
            Self::Busy => "busy",
            Self::Deferred => "deferred",
            Self::Failed => "failed",
        }
    }
}

pub async fn check_and_maybe_install(app: &AppHandle, force: bool) -> UpdateCode {
    let ctx = app.state::<Arc<AppContext>>();
    let mut gate = ctx.update_gate.lock().await;
    if *gate {
        return UpdateCode::Busy;
    }
    *gate = true;
    drop(gate);
    let outcome = run_check(app, force).await;
    *ctx.update_gate.lock().await = false;
    if outcome == UpdateCode::Installed {
        app.restart();
    }
    outcome
}

async fn run_check(app: &AppHandle, force: bool) -> UpdateCode {
    let ctx = app.state::<Arc<AppContext>>();
    let settings = ctx.settings.lock().clone();
    if !force && !settings.auto_update_enabled {
        return UpdateCode::None;
    }
    let busy = is_cancellable(&ctx.state.lock()) || crate::app::compare::compare_busy(&ctx);
    if !install_allowed(busy) {
        return UpdateCode::Deferred;
    }
    let updater = match app.updater() {
        Ok(updater) => updater,
        Err(err) => {
            tracing::error!(error = %err, "updater unavailable");
            return UpdateCode::Failed;
        }
    };
    match updater.check().await {
        Ok(Some(update)) => {
            let prerelease = update.version.contains('-');
            if !is_newer_stable(current_version(), &update.version, prerelease) {
                return UpdateCode::None;
            }
            match update.download_and_install(|_, _| {}, || {}).await {
                Ok(()) => UpdateCode::Installed,
                Err(err) => {
                    tracing::error!(error = %err, "update install failed");
                    UpdateCode::Failed
                }
            }
        }
        Ok(None) => UpdateCode::None,
        Err(err) => {
            tracing::error!(error = %err, "update check failed");
            UpdateCode::Failed
        }
    }
}

pub fn spawn_background_loop(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(Duration::from_secs(15)).await;
        loop {
            let enabled = {
                let ctx = app.state::<Arc<AppContext>>();
                let enabled = ctx.settings.lock().auto_update_enabled;
                enabled
            };
            if enabled {
                let outcome = check_and_maybe_install(&app, false).await;
                match outcome {
                    UpdateCode::Installed => return,
                    UpdateCode::Deferred | UpdateCode::Busy | UpdateCode::Failed => {
                        tokio::time::sleep(Duration::from_secs(120)).await;
                    }
                    UpdateCode::None => {
                        tokio::time::sleep(Duration::from_secs(6 * 60 * 60)).await;
                    }
                }
            } else {
                tokio::time::sleep(Duration::from_secs(120)).await;
            }
        }
    });
}
