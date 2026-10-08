//! Explicit, idle-only shutdown for the local developer rebuild workflow.
//! It never edits the persisted close-to-tray preference.
use std::sync::{atomic::Ordering, Arc};
use tauri::{AppHandle, Manager};

use super::{lifecycle::is_local_build, operations, session::AppContext};
use crate::runtime_diagnostics::{self, Event, ExitReason, LocalRebuildRefusal};

const FLAG: &str = "--quit-for-local-rebuild";

pub(crate) fn request(args: &[String]) -> Option<Option<u32>> {
    let controls: Vec<_> = args
        .iter()
        .skip(1)
        .filter(|arg| arg.as_str() == FLAG || arg.starts_with(&format!("{FLAG}=")))
        .collect();
    if controls.is_empty() {
        return None;
    }
    let target = (controls.len() == 1)
        .then(|| controls[0].strip_prefix(&format!("{FLAG}=")))
        .flatten()
        .and_then(|value| {
            if value.is_empty() || !value.bytes().all(|b| b.is_ascii_digit()) {
                return None;
            }
            value.parse::<u32>().ok().filter(|pid| *pid != 0)
        });
    Some(target)
}

fn target_allowed(
    local: bool,
    target: Option<u32>,
    current: u32,
) -> Result<(), LocalRebuildRefusal> {
    if !local {
        return Err(LocalRebuildRefusal::NotLocal);
    }
    if target != Some(current) {
        return Err(LocalRebuildRefusal::InvalidTarget);
    }
    Ok(())
}

pub(super) fn reserve_idle_shutdown(ctx: &AppContext) -> bool {
    // Match shutdown_session's lock order. Block fresh admissions before releasing
    // either guard so a dictation cannot start between the idle check and shutdown.
    let Some(_lifecycle) = ctx.session_lifecycle.try_lock() else {
        return false;
    };
    let Some(_admission) = ctx.admission.try_lock() else {
        return false;
    };
    if operations::system_busy(ctx) {
        return false;
    }
    if !super::shutdown::claim(ctx, super::shutdown::Action::Exit(0)) {
        return false;
    }
    runtime_diagnostics::record(Event::ExitIntent {
        reason: ExitReason::LocalRebuild,
    });
    runtime_diagnostics::record(Event::ShutdownStarted);
    ctx.shutdown_requested.store(true, Ordering::SeqCst);
    true
}

pub(crate) fn handle_request(app: &AppHandle, args: &[String]) -> bool {
    let Some(target) = request(args) else {
        return false;
    };
    if let Err(reason) = target_allowed(is_local_build(), target, std::process::id()) {
        runtime_diagnostics::record(Event::LocalRebuildRefused { reason });
        return true;
    }
    let Some(ctx) = app
        .try_state::<Arc<AppContext>>()
        .map(|ctx| Arc::clone(&ctx))
    else {
        runtime_diagnostics::record(Event::LocalRebuildRefused {
            reason: LocalRebuildRefusal::NotReady,
        });
        return true;
    };
    let app = app.clone();
    // WM_COPYDATA invokes the single-instance callback synchronously on the UI
    // thread. A worker can hold lifecycle while waiting for a UI window getter;
    // shutdown must never wait for that worker from this callback.
    if std::thread::Builder::new()
        .name("local-rebuild-exit".into())
        .spawn(move || {
            if !reserve_idle_shutdown(&ctx) {
                runtime_diagnostics::record(Event::LocalRebuildRefused {
                    reason: LocalRebuildRefusal::Busy,
                });
            } else {
                super::shutdown::complete_local(&app);
            }
        })
        .is_err()
    {
        runtime_diagnostics::record(Event::LocalRebuildRefused {
            reason: LocalRebuildRefusal::NotReady,
        });
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    fn args(values: &[&str]) -> Vec<String> {
        values.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn control_requires_exact_flag_and_positive_pid() {
        assert_eq!(request(&args(&["voxely", "--autostart"])), None);
        assert_eq!(
            request(&args(&["voxely", "--quit-for-local-rebuild=42"])),
            Some(Some(42))
        );
        for value in [
            "--quit-for-local-rebuild",
            "--quit-for-local-rebuild=0",
            "--quit-for-local-rebuild=-1",
            "--quit-for-local-rebuild=4294967296",
            "--quit-for-local-rebuild=+42",
        ] {
            assert_eq!(request(&args(&["voxely", value])), Some(None));
        }
        assert_eq!(
            request(&args(&[
                "voxely",
                "--quit-for-local-rebuild=42",
                "--quit-for-local-rebuild=42"
            ])),
            Some(None)
        );
        assert_eq!(
            target_allowed(false, Some(42), 42),
            Err(LocalRebuildRefusal::NotLocal)
        );
        assert_eq!(
            target_allowed(true, Some(41), 42),
            Err(LocalRebuildRefusal::InvalidTarget)
        );
        assert!(target_allowed(true, Some(42), 42).is_ok());
    }

    #[test]
    fn active_work_is_preserved_and_idle_shutdown_blocks_new_admissions() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = AppContext::initialize(dir.path().to_path_buf()).unwrap();
        ctx.capture_starting.store(true, Ordering::SeqCst);
        assert!(!reserve_idle_shutdown(&ctx));
        assert!(!ctx.shutdown_requested.load(Ordering::SeqCst));
        ctx.capture_starting.store(false, Ordering::SeqCst);
        ctx.in_flight.lock().insert("existing-retry".into());
        assert!(!reserve_idle_shutdown(&ctx));
        assert!(ctx.in_flight.lock().contains("existing-retry"));
        ctx.in_flight.lock().clear();
        ctx.settings.lock().save(&ctx.settings_path).unwrap();
        let before = std::fs::read(&ctx.settings_path).unwrap();
        assert!(reserve_idle_shutdown(&ctx));
        assert!(operations::system_busy(&ctx));
        assert!(!reserve_idle_shutdown(&ctx));
        assert_eq!(std::fs::read(&ctx.settings_path).unwrap(), before);
    }

    #[test]
    fn contended_lifecycle_or_admission_is_refused_without_waiting() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = Arc::new(AppContext::initialize(dir.path().to_path_buf()).unwrap());
        for gate in [&ctx.session_lifecycle, &ctx.admission] {
            let held = gate.lock();
            let worker_ctx = Arc::clone(&ctx);
            let (done_tx, done_rx) = std::sync::mpsc::channel();
            let worker = std::thread::spawn(move || {
                done_tx.send(reserve_idle_shutdown(&worker_ctx)).unwrap()
            });
            let result = done_rx.recv_timeout(std::time::Duration::from_secs(2));
            // Release before asserting so even a broken blocking implementation
            // cannot leave a deadlocked worker behind after this test fails.
            drop(held);
            worker.join().unwrap();
            assert!(!result.expect("idle-only exit waited for a held session gate"));
        }
        assert!(!ctx.shutdown_requested.load(Ordering::SeqCst));
    }
}
