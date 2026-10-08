pub mod policy;

use std::sync::Arc;
use std::time::Duration;

use tauri::{AppHandle, Manager};
use tauri_plugin_updater::UpdaterExt;

use crate::app::lifecycle::is_local_build;
use crate::app::session::AppContext;
use crate::runtime_diagnostics::{record, Event as DiagnosticEvent, ExitReason, UpdateOutcome};

use policy::{install_allowed, is_newer_stable, restart_after_install_allowed};

pub fn current_version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

const UPDATE_CHECK_ATTEMPTS: u32 = 3;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UpdateCode {
    None,
    Available,
    Installed,
    Busy,
    Deferred,
    Failed,
}

impl UpdateCode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Available => "available",
            Self::Installed => "installed",
            Self::Busy => "busy",
            Self::Deferred => "deferred",
            Self::Failed => "failed",
        }
    }
}

pub fn should_poll_updates(local_build: bool, auto_enabled: bool) -> bool {
    auto_enabled && !local_build
}

pub fn install_on_discovery(manual_check: bool) -> bool {
    !manual_check
}

pub fn should_defer_update(install: bool, busy: bool) -> bool {
    install && !install_allowed(busy)
}

pub fn update_error_retryable(message: &str) -> bool {
    let lower = message.to_ascii_lowercase();
    lower.contains("error sending request")
        || lower.contains("timed out")
        || lower.contains("timeout")
        || lower.contains("connection")
        || lower.contains("dns")
        || lower.contains("tls")
        || lower.contains("status code")
        || lower.contains("failed to check")
}

pub async fn check_updates(app: &AppHandle, force: bool) -> UpdateCode {
    run_update(app, force, false).await
}

pub async fn install_available_update(app: &AppHandle) -> UpdateCode {
    let outcome = run_update(app, true, true).await;
    if outcome == UpdateCode::Installed {
        record(DiagnosticEvent::ExitIntent {
            reason: ExitReason::UpdateRestart,
        });
        crate::app::shutdown::request(app, crate::app::shutdown::Action::Restart);
    }
    outcome
}

pub async fn check_and_maybe_install(app: &AppHandle, force: bool) -> UpdateCode {
    install_available_update_if_enabled(app, force).await
}

async fn install_available_update_if_enabled(app: &AppHandle, force: bool) -> UpdateCode {
    let outcome = run_update(app, force, true).await;
    if outcome == UpdateCode::Installed {
        record(DiagnosticEvent::ExitIntent {
            reason: ExitReason::UpdateRestart,
        });
        crate::app::shutdown::request(app, crate::app::shutdown::Action::Restart);
    }
    outcome
}

async fn run_update(app: &AppHandle, force: bool, install: bool) -> UpdateCode {
    let ctx = app.state::<Arc<AppContext>>();
    let mut gate = ctx.update_gate.lock().await;
    if *gate {
        return UpdateCode::Busy;
    }
    *gate = true;
    drop(gate);
    record(DiagnosticEvent::UpdateCheckStarted);
    let outcome = run_check(app, force, install).await;
    record(DiagnosticEvent::UpdateCheckFinished {
        outcome: match outcome {
            UpdateCode::None => UpdateOutcome::None,
            UpdateCode::Available => UpdateOutcome::Available,
            UpdateCode::Installed => UpdateOutcome::Installed,
            UpdateCode::Busy => UpdateOutcome::Busy,
            UpdateCode::Deferred => UpdateOutcome::Deferred,
            UpdateCode::Failed => UpdateOutcome::Failed,
        },
    });
    *ctx.update_gate.lock().await = false;
    outcome
}

async fn run_check(app: &AppHandle, force: bool, install: bool) -> UpdateCode {
    let ctx = app.state::<Arc<AppContext>>();
    let settings = ctx.settings.lock().clone();
    if !force && !settings.auto_update_enabled {
        return UpdateCode::None;
    }
    let busy = crate::app::operations::system_busy(&ctx);
    if should_defer_update(install, busy) {
        return UpdateCode::Deferred;
    }
    let mut delay = Duration::from_millis(400);
    let mut last_err = String::new();
    for attempt in 1..=UPDATE_CHECK_ATTEMPTS {
        match try_check(app, install).await {
            Ok(code) => return code,
            Err(err) => {
                last_err = err;
                if attempt < UPDATE_CHECK_ATTEMPTS && update_error_retryable(&last_err) {
                    tracing::warn!(
                        attempt,
                        error = %last_err,
                        "update check retry"
                    );
                    tokio::time::sleep(delay).await;
                    delay = delay.saturating_mul(2);
                    continue;
                }
                break;
            }
        }
    }
    tracing::error!(error = %last_err, "update check failed");
    UpdateCode::Failed
}

async fn try_check(app: &AppHandle, install: bool) -> Result<UpdateCode, String> {
    let updater = match build_updater(app) {
        Ok(updater) => updater,
        Err(err) => return Err(err),
    };
    match updater.check().await {
        Ok(Some(update)) => {
            let prerelease = update.version.contains('-');
            if !is_newer_stable(current_version(), &update.version, prerelease) {
                return Ok(UpdateCode::None);
            }
            if !install {
                return Ok(UpdateCode::Available);
            }
            let ctx = app.state::<Arc<AppContext>>();
            record(DiagnosticEvent::UpdateDownloadStarted);
            let bytes = update
                .download(|_, _| {}, || {})
                .await
                .map_err(|err| err.to_string())?;
            record(DiagnosticEvent::UpdateDownloadCompleted);
            if !reserve_update_install(&ctx) {
                return Ok(UpdateCode::Deferred);
            }
            record(DiagnosticEvent::UpdateInstallStarted);
            let result = update.install(bytes);
            record(DiagnosticEvent::UpdateInstallReturned {
                succeeded: result.is_ok(),
            });
            ctx.update_installing
                .store(false, std::sync::atomic::Ordering::SeqCst);
            match result {
                Ok(()) => {
                    if !restart_after_install_allowed(crate::app::operations::system_busy(&ctx)) {
                        return Ok(UpdateCode::Deferred);
                    }
                    Ok(UpdateCode::Installed)
                }
                Err(err) => {
                    tracing::error!(error = %err, "update install failed");
                    Ok(UpdateCode::Failed)
                }
            }
        }
        Ok(None) => Ok(UpdateCode::None),
        Err(err) => Err(err.to_string()),
    }
}

fn reserve_update_install(ctx: &AppContext) -> bool {
    let _admission = crate::app::operations::lock_admission(ctx);
    if crate::app::operations::system_busy(ctx) {
        return false;
    }
    ctx.update_installing
        .store(true, std::sync::atomic::Ordering::SeqCst);
    true
}

fn build_updater(app: &AppHandle) -> Result<tauri_plugin_updater::Updater, String> {
    let ua = format!(
        "Voxely/{} (+https://github.com/AryaPaw/voxely)",
        current_version()
    );
    app.updater_builder()
        .timeout(Duration::from_secs(20))
        .on_before_exit(|| {
            // The Windows installer exits inside the plugin without the Tauri event loop.
            // This hook precedes installer launch, so it is intent, not exit completion.
            record(DiagnosticEvent::ExitIntent {
                reason: ExitReason::UpdateInstaller,
            });
        })
        .header("User-Agent", ua)
        .map_err(|err| err.to_string())?
        .build()
        .map_err(|err| err.to_string())
}

pub fn spawn_background_loop(app: AppHandle) {
    if is_local_build() {
        tracing::info!("skipping auto-update polling on local debug build");
        return;
    }
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(Duration::from_secs(15)).await;
        loop {
            let enabled = {
                let ctx = app.state::<Arc<AppContext>>();
                let enabled = ctx.settings.lock().auto_update_enabled;
                enabled
            };
            if should_poll_updates(false, enabled) {
                let outcome = check_and_maybe_install(&app, false).await;
                match outcome {
                    UpdateCode::Installed => return,
                    UpdateCode::Deferred
                    | UpdateCode::Busy
                    | UpdateCode::Failed
                    | UpdateCode::Available => {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn install_reservation_refuses_active_audio_and_blocks_new_reservations() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = AppContext::initialize(dir.path().to_path_buf()).unwrap();
        ctx.filter_recording
            .store(true, std::sync::atomic::Ordering::SeqCst);
        assert!(!reserve_update_install(&ctx));
        assert!(!ctx
            .update_installing
            .load(std::sync::atomic::Ordering::SeqCst));
        ctx.filter_recording
            .store(false, std::sync::atomic::Ordering::SeqCst);
        assert!(reserve_update_install(&ctx));
        assert!(crate::app::operations::system_busy(&ctx));
        assert!(!reserve_update_install(&ctx));
    }

    #[test]
    fn local_debug_does_not_poll_updates() {
        assert!(!should_poll_updates(true, true));
        assert!(should_poll_updates(false, true));
        assert!(!should_poll_updates(false, false));
    }

    #[test]
    fn manual_check_does_not_auto_install() {
        assert!(!install_on_discovery(true));
        assert!(install_on_discovery(false));
        assert_eq!(UpdateCode::Available.as_str(), "available");
        assert!(!should_defer_update(false, true));
        assert!(should_defer_update(true, true));
        assert!(!should_defer_update(true, false));
    }

    #[test]
    fn github_network_blip_is_retryable() {
        assert!(update_error_retryable(
            "error sending request for url (https://github.com/AryaPaw/voxely/releases/latest/download/latest.json)"
        ));
        assert!(update_error_retryable(
            "update endpoint did not respond with a successful status code"
        ));
        assert!(!update_error_retryable("invalid signature"));
    }
}
