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
    // Always own finalization, even when cleanup initially looked ready. A UI
    // completion callback may skip its best-effort schedule under contention.
    loop {
        if super::session::schedule_deferred_shutdown_exit(app, &ctx) {
            return;
        }
        if super::session::shutdown_capture_cleanup_expired(started, std::time::Instant::now()) {
            super::session::schedule_forced_shutdown_exit(app, &ctx);
            return;
        }
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
