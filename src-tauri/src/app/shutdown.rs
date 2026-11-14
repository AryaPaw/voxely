//! One asynchronous owner for ordinary exit and restart requests.
use std::sync::atomic::Ordering;
use std::sync::Arc;
use tauri::{AppHandle, Manager};

use super::session::AppContext;
use crate::runtime_diagnostics::{self, Event};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Action {
    Exit(i32),
    Restart,
}

pub(super) fn claim(ctx: &AppContext, action: Action) -> bool {
    // This guard protects only a small value. Never call session/window APIs
    // while holding it. First accepted request owns the final action.
    let mut current = ctx.shutdown_action.lock();
    if current.is_some() {
        return false;
    }
    *current = Some(action);
    true
}

fn release_failed_spawn(ctx: &AppContext) {
    *ctx.shutdown_action.lock() = None;
    runtime_diagnostics::record(Event::ShutdownWorkerUnavailable);
}

pub(crate) fn request(app: &AppHandle, action: Action) {
    let ctx = app.state::<Arc<AppContext>>();
    if !claim(&ctx, action) {
        return;
    }
    let worker_app = app.clone();
    if std::thread::Builder::new()
        .name("session-shutdown".into())
        .spawn(move || run(&worker_app))
        .is_err()
    {
        release_failed_spawn(&ctx);
    }
}

/// Already on the idle-only control worker: no fallible spawn after reservation.
pub(crate) fn complete_local(app: &AppHandle) {
    // Idle reservation already claimed the action before setting shutdown_requested.
    run(app);
}

fn run(app: &AppHandle) {
    super::session::shutdown_session(app);
    let ctx = app.state::<Arc<AppContext>>();
    let started = std::time::Instant::now();
    wait_for_cleanup(
        || super::session::schedule_deferred_shutdown_exit(app, &ctx),
        || super::session::schedule_forced_shutdown_exit(app, &ctx),
        || super::session::shutdown_capture_cleanup_expired(started, std::time::Instant::now()),
    );
}

fn wait_for_cleanup(
    mut schedule_ready: impl FnMut() -> bool,
    schedule_forced: impl FnOnce(),
    mut deadline_expired: impl FnMut() -> bool,
) {
    // Always own finalization, even when cleanup initially looked ready. A UI
    // completion callback may skip its best-effort schedule under contention.
    loop {
        if schedule_ready() {
            return;
        }
        if deadline_expired() {
            schedule_forced();
            return;
        }
        // Storage recovery belongs to the periodic worker. Its mutex or SQLite
        // waits must never delay this owner's independent shutdown deadline.
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
}

pub(crate) fn exit_permitted(ctx: &AppContext) -> bool {
    ctx.shutdown_cleanup_started.load(Ordering::SeqCst)
        && ctx.shutdown_exit_scheduled.load(Ordering::SeqCst)
}

pub(crate) fn dispatch(app: &AppHandle, ctx: &AppContext) {
    let action = *ctx.shutdown_action.lock();
    match action {
        Some(Action::Restart) => app.request_restart(),
        Some(Action::Exit(code)) => app.exit(code),
        None => runtime_diagnostics::record(Event::ShutdownWorkerUnavailable),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shutdown_worker_drains_a_full_healthy_usage_backlog_before_forced_exit() {
        let directory = tempfile::tempdir().unwrap();
        let ctx = Arc::new(AppContext::initialize(directory.path().to_path_buf()).unwrap());
        let injection =
            rusqlite::Connection::open(directory.path().join("history.sqlite")).unwrap();
        injection.execute_batch("CREATE TRIGGER reject_shutdown_usage BEFORE INSERT ON usage_attempts BEGIN SELECT RAISE(ABORT,'fixture shutdown persistence failure'); END;").unwrap();
        let generation = ctx.history.lock().compare_usage_generation().unwrap();
        for index in 0..super::super::session::MAX_PENDING_ATTEMPTS {
            assert!(super::super::session::persist_compare_attempt(
                &ctx,
                crate::history::repository::ApiAttemptUsage {
                    operation_id: "compare:shutdown".into(),
                    attempt_id: format!("compare:shutdown:{index}"),
                    occurred_at: chrono::Utc::now(),
                    model: "model".into(),
                    cost_usd: Some(0.01),
                },
                &generation
            )
            .is_err());
        }
        assert!(!super::super::session::shutdown_session_exit_ready(&ctx));
        injection
            .execute_batch("DROP TRIGGER reject_shutdown_usage;")
            .unwrap();
        ctx.shutdown_requested.store(true, Ordering::SeqCst);
        ctx.shutdown_cleanup_started.store(true, Ordering::SeqCst);
        let started = std::time::Instant::now();
        let forced = std::sync::atomic::AtomicBool::new(false);
        let recovery_ctx = Arc::clone(&ctx);
        let recovery = std::thread::spawn(move || {
            // The same recovery entry point is called by recovery_interval.
            super::super::session::flush_pending_attempts(&recovery_ctx)
        });
        wait_for_cleanup(
            || {
                // Storage recovery must not retain either UI gate.
                assert!(ctx.session_lifecycle.try_lock().is_some());
                assert!(ctx.admission.try_lock().is_some());
                super::super::session::shutdown_session_exit_ready(&ctx)
            },
            || forced.store(true, Ordering::SeqCst),
            || {
                super::super::session::shutdown_capture_cleanup_expired(
                    started,
                    std::time::Instant::now(),
                )
            },
        );
        assert!(recovery.join().unwrap());
        assert!(!forced.load(Ordering::SeqCst));
        assert!(!super::super::session::shutdown_capture_cleanup_expired(
            started,
            std::time::Instant::now()
        ));
        assert!(!super::super::session::compare_usage_is_pending(
            &ctx,
            "compare:shutdown"
        ));
        let now = chrono::Utc::now();
        let statistics = ctx
            .history
            .lock()
            .get_usage_statistics(
                now - chrono::Duration::days(1),
                now + chrono::Duration::days(1),
            )
            .unwrap();
        assert_eq!(
            statistics.api_requests,
            super::super::session::MAX_PENDING_ATTEMPTS as i64
        );
        assert_eq!(statistics.unpriced_attempts, 0);
        assert!(
            (statistics.reported_cost_usd
                - super::super::session::MAX_PENDING_ATTEMPTS as f64 * 0.01)
                .abs()
                < 1e-9
        );
    }

    #[test]
    fn shutdown_deadline_forces_exit_while_usage_recovery_waits_for_history() {
        let directory = tempfile::tempdir().unwrap();
        let ctx = Arc::new(AppContext::initialize(directory.path().to_path_buf()).unwrap());
        let injection =
            rusqlite::Connection::open(directory.path().join("history.sqlite")).unwrap();
        injection.execute_batch("CREATE TRIGGER reject_blocked_shutdown_usage BEFORE INSERT ON usage_attempts BEGIN SELECT RAISE(ABORT,'fixture shutdown persistence failure'); END;").unwrap();
        let generation = ctx.history.lock().compare_usage_generation().unwrap();
        assert!(super::super::session::persist_compare_attempt(
            &ctx,
            crate::history::repository::ApiAttemptUsage {
                operation_id: "compare:blocked-shutdown".into(),
                attempt_id: "compare:blocked-shutdown:1".into(),
                occurred_at: chrono::Utc::now(),
                model: "model".into(),
                cost_usd: Some(0.01),
            },
            &generation,
        )
        .is_err());
        injection
            .execute_batch("DROP TRIGGER reject_blocked_shutdown_usage;")
            .unwrap();
        ctx.shutdown_requested.store(true, Ordering::SeqCst);
        ctx.shutdown_cleanup_started.store(true, Ordering::SeqCst);

        // Keep storage unavailable until after observing the forced callback.
        // The recovery worker and the old watchdog's inline drain both need
        // this mutex, whereas readiness/deadline polling must not acquire it.
        let history = ctx.history.lock();
        let (recovery_started, recovery_entered) = std::sync::mpsc::channel();
        let recovery_ctx = Arc::clone(&ctx);
        let recovery = std::thread::spawn(move || {
            recovery_started.send(()).unwrap();
            super::super::session::flush_pending_attempts(&recovery_ctx)
        });
        recovery_entered
            .recv_timeout(std::time::Duration::from_secs(2))
            .unwrap();
        let (forced, observed_forced) = std::sync::mpsc::channel();
        let watchdog_ctx = Arc::clone(&ctx);
        let watchdog = std::thread::spawn(move || {
            let mut polls = 0;
            wait_for_cleanup(
                || super::super::session::shutdown_session_exit_ready(&watchdog_ctx),
                || forced.send(()).unwrap(),
                || {
                    // The first poll precedes expiration, so an inline storage
                    // drain would block before the second deadline check.
                    polls += 1;
                    polls >= 2
                },
            );
        });
        let forced_while_storage_blocked =
            observed_forced.recv_timeout(std::time::Duration::from_secs(2));
        // Release and join even if the regression is reintroduced, so this
        // test fails cleanly without stranding a worker on the storage mutex.
        drop(history);
        assert!(recovery.join().unwrap());
        watchdog.join().unwrap();
        assert!(
            forced_while_storage_blocked.is_ok(),
            "shutdown deadline waited for storage recovery"
        );
    }

    #[test]
    fn first_request_owns_action_and_spawn_failure_allows_retry_without_shutdown() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = AppContext::initialize(dir.path().to_path_buf()).unwrap();
        assert!(claim(&ctx, Action::Exit(7)));
        assert!(!claim(&ctx, Action::Restart));
        assert_eq!(*ctx.shutdown_action.lock(), Some(Action::Exit(7)));
        release_failed_spawn(&ctx);
        assert!(!ctx.shutdown_requested.load(Ordering::SeqCst));
        assert!(claim(&ctx, Action::Restart));
        assert_eq!(*ctx.shutdown_action.lock(), Some(Action::Restart));
    }

    #[test]
    fn exit_gate_is_lock_free_and_requires_cleanup_and_final_schedule() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = AppContext::initialize(dir.path().to_path_buf()).unwrap();
        let _lifecycle = ctx.session_lifecycle.lock();
        let _admission = ctx.admission.lock();
        assert!(!exit_permitted(&ctx));
        ctx.shutdown_requested.store(true, Ordering::SeqCst);
        assert!(!exit_permitted(&ctx));
        ctx.shutdown_cleanup_started.store(true, Ordering::SeqCst);
        assert!(!exit_permitted(&ctx));
        ctx.shutdown_exit_scheduled.store(true, Ordering::SeqCst);
        assert!(exit_permitted(&ctx));
    }

    #[test]
    fn pending_owner_blocks_local_reservation_before_a_failed_spawn() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = AppContext::initialize(dir.path().to_path_buf()).unwrap();
        assert!(claim(&ctx, Action::Exit(7)));
        assert!(!super::super::local_rebuild::reserve_idle_shutdown(&ctx));
        assert!(!ctx.shutdown_requested.load(Ordering::SeqCst));
        release_failed_spawn(&ctx);
        assert_eq!(*ctx.shutdown_action.lock(), None);
        assert!(!ctx.shutdown_requested.load(Ordering::SeqCst));
        assert!(super::super::local_rebuild::reserve_idle_shutdown(&ctx));
        assert_eq!(*ctx.shutdown_action.lock(), Some(Action::Exit(0)));
        assert!(ctx.shutdown_requested.load(Ordering::SeqCst));
    }
}
