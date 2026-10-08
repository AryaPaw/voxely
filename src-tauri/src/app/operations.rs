use parking_lot::MutexGuard;
use std::collections::HashSet;

use crate::app::machine::{is_cancellable, SessionState};
use crate::app::session::AppContext;

pub fn lock_admission(ctx: &AppContext) -> MutexGuard<'_, ()> {
    ctx.admission.lock()
}

pub(crate) fn with_admission_released<T>(
    admission: MutexGuard<'_, ()>,
    action: impl FnOnce() -> T,
) -> T {
    drop(admission);
    action()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SessionLease {
    pub generation: u64,
    pub recording_id: Option<&'static str>,
}

pub fn lease_matches(expected: u64, current: u64) -> bool {
    expected == current && expected != 0
}

pub fn escape_cancels(ctx: &AppContext) -> bool {
    is_cancellable(&ctx.state.lock())
        || !ctx.in_flight.lock().is_empty()
        || ctx
            .insert_in_flight
            .load(std::sync::atomic::Ordering::SeqCst)
}

pub fn system_busy(ctx: &AppContext) -> bool {
    is_cancellable(&ctx.state.lock())
        || ctx.capture.lock().is_some()
        || ctx.preview_capture.lock().is_some()
        || ctx.compare_capture.lock().is_some()
        || ctx.compare_state.lock().recording
        || ctx
            .filter_recording
            .load(std::sync::atomic::Ordering::SeqCst)
        || ctx
            .compare_running
            .load(std::sync::atomic::Ordering::SeqCst)
        || ctx.meter_starting.load(std::sync::atomic::Ordering::SeqCst)
        || ctx
            .capture_starting
            .load(std::sync::atomic::Ordering::SeqCst)
        || ctx
            .pending_native_capture_starts
            .load(std::sync::atomic::Ordering::SeqCst)
            > 0
        || ctx
            .capture_stopping
            .load(std::sync::atomic::Ordering::SeqCst)
        || ctx
            .shutdown_requested
            .load(std::sync::atomic::Ordering::SeqCst)
        || ctx
            .insert_in_flight
            .load(std::sync::atomic::Ordering::SeqCst)
        || !ctx.in_flight.lock().is_empty()
        || ctx
            .update_installing
            .load(std::sync::atomic::Ordering::SeqCst)
}

pub fn protected_recording_ids(ctx: &AppContext) -> HashSet<String> {
    let mut ids = ctx.in_flight.lock().clone();
    if let Some(id) = ctx.session_recording_id.lock().clone() {
        ids.insert(id);
    }
    ids
}

pub fn history_recording_is_protected(ctx: &AppContext, recording_id: &str) -> bool {
    protected_recording_ids(ctx).contains(recording_id)
}

pub fn protected_audio_names(ctx: &AppContext) -> HashSet<String> {
    let mut names = HashSet::new();
    let in_flight = ctx.in_flight.lock().clone();
    for id in &in_flight {
        names.extend([
            format!("{id}.raw.wav"),
            format!("{id}.processed.wav"),
            format!("{id}.raw.stt.wav"),
            format!("{id}.processed.stt.wav"),
            format!("{id}.raw.wav.tmp"),
            format!("{id}.processed.wav.tmp"),
            format!("{id}.raw.stt.wav.tmp"),
            format!("{id}.processed.stt.wav.tmp"),
        ]);
    }
    add_active_recording_artifacts(
        &mut names,
        &crate::history::repository::audio_dir(&ctx.data_dir),
        &in_flight,
    );
    if let Some(id) = ctx.session_recording_id.lock().clone() {
        names.insert(format!("{id}.raw.wav"));
        names.insert(format!("{id}.processed.wav"));
        names.insert(format!("{id}.raw.wav.tmp"));
        names.insert(format!("{id}.wav.tmp"));
    }
    names.insert("filter-sample.wav".into());
    names.insert("filter-sample.wav.tmp".into());
    names.insert("filter-preview.wav".into());
    names.insert("filter-preview.wav.tmp".into());
    names.insert("filter-preview-original.wav".into());
    names.insert("filter-preview-original.wav.tmp".into());
    if ctx
        .filter_recording
        .load(std::sync::atomic::Ordering::SeqCst)
    {
        if let Ok(entries) = std::fs::read_dir(crate::history::repository::audio_dir(&ctx.data_dir))
        {
            names.extend(entries.filter_map(Result::ok).filter_map(|entry| {
                let name = entry.file_name().to_string_lossy().into_owned();
                name.starts_with("filter-preview-").then_some(name)
            }));
        }
    }
    let compare = ctx.compare_state.lock();
    if let Some(path) = compare.listen_path.as_ref() {
        if let Some(name) = std::path::Path::new(path).file_name() {
            names.insert(name.to_string_lossy().into_owned());
        }
    }
    if let Some(path) = compare.stt_path.as_ref() {
        if let Some(name) = std::path::Path::new(path).file_name() {
            names.insert(name.to_string_lossy().into_owned());
        }
    }
    if compare.nonce > 0 {
        names.insert(crate::app::compare::compare_listen_name(compare.nonce));
        names.insert(crate::app::compare::compare_stt_name(compare.nonce));
        names.insert(crate::app::compare::compare_raw_name(compare.nonce));
    }
    if let Some(run_id) = compare.run_id.as_ref() {
        names.insert(format!("model-compare.{run_id}.stt.wav"));
    }
    names
}

fn add_active_recording_artifacts(
    names: &mut HashSet<String>,
    audio_root: &std::path::Path,
    active_ids: &HashSet<String>,
) {
    if active_ids.is_empty() {
        return;
    }
    let Ok(entries) = std::fs::read_dir(audio_root) else {
        return;
    };
    for entry in entries.filter_map(Result::ok) {
        let name = entry.file_name().to_string_lossy().into_owned();
        if active_ids
            .iter()
            .any(|id| name.starts_with(&format!("{id}.")))
        {
            names.insert(name);
        }
    }
}

pub fn hide_overlay_allowed(_state: &SessionState, expected: u64, current: u64) -> bool {
    expected == current
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::machine::SessionState;

    #[test]
    fn stale_generation_does_not_match() {
        assert!(lease_matches(3, 3));
        assert!(!lease_matches(3, 4));
        assert!(!lease_matches(0, 0));
    }

    #[test]
    fn hide_blocked_while_new_session_live() {
        assert!(hide_overlay_allowed(&SessionState::Recording, 2, 2));
        assert!(hide_overlay_allowed(&SessionState::Idle, 2, 2));
        assert!(hide_overlay_allowed(&SessionState::Idle, 0, 0));
        assert!(!hide_overlay_allowed(&SessionState::Idle, 2, 3));
    }

    #[test]
    fn capture_stop_reservation_keeps_session_busy_until_result_arrives() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = AppContext::initialize(dir.path().to_path_buf()).unwrap();
        assert!(!system_busy(&ctx));

        ctx.capture_stopping
            .store(true, std::sync::atomic::Ordering::SeqCst);
        assert!(system_busy(&ctx));

        ctx.capture_stopping
            .store(false, std::sync::atomic::Ordering::SeqCst);
        assert!(!system_busy(&ctx));
    }

    #[test]
    fn active_recording_protects_unique_reprocess_artifacts_from_retention() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("recording.processed-retry-1.wav"),
            b"active",
        )
        .unwrap();
        std::fs::write(
            dir.path().join("recording.processed-retry-1.stt.wav"),
            b"upload",
        )
        .unwrap();
        std::fs::write(dir.path().join("other.processed-retry-1.wav"), b"other").unwrap();
        let mut names = HashSet::new();

        add_active_recording_artifacts(
            &mut names,
            dir.path(),
            &HashSet::from(["recording".to_string()]),
        );

        assert!(names.contains("recording.processed-retry-1.wav"));
        assert!(names.contains("recording.processed-retry-1.stt.wav"));
        assert!(!names.contains("other.processed-retry-1.wav"));
    }

    #[test]
    fn retry_reservation_protects_completed_recording_before_processing_status() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = AppContext::initialize(dir.path().to_path_buf()).unwrap();
        let recording_id = "completed-recording";

        ctx.in_flight.lock().insert(recording_id.to_string());

        assert!(history_recording_is_protected(&ctx, recording_id));
        assert!(!history_recording_is_protected(&ctx, "other-recording"));
    }

    #[test]
    fn admission_is_released_before_waiting_for_session_lifecycle() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = std::sync::Arc::new(AppContext::initialize(dir.path().to_path_buf()).unwrap());
        let admission = lock_admission(&ctx);
        let waiter_ctx = std::sync::Arc::clone(&ctx);
        let (lifecycle_locked, lifecycle_wait) = std::sync::mpsc::channel();
        let waiter = std::thread::spawn(move || {
            let _lifecycle = waiter_ctx.session_lifecycle.lock();
            lifecycle_locked.send(()).unwrap();
            let _admission = waiter_ctx.admission.lock();
        });
        lifecycle_wait
            .recv_timeout(std::time::Duration::from_secs(2))
            .expect("shutdown did not acquire the lifecycle lock");

        let action_ctx = std::sync::Arc::clone(&ctx);
        with_admission_released(admission, || {
            let _lifecycle = action_ctx.session_lifecycle.lock();
        });

        waiter.join().unwrap();
    }
}
