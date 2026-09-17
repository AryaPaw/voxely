use std::collections::HashSet;

use crate::app::machine::{is_cancellable, SessionState};
use crate::app::session::AppContext;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SessionLease {
    pub generation: u64,
    pub recording_id: Option<&'static str>,
}

pub fn lease_matches(expected: u64, current: u64) -> bool {
    expected == current && expected != 0
}

pub fn system_busy(ctx: &AppContext) -> bool {
    is_cancellable(&ctx.state.lock())
        || ctx.capture.lock().is_some()
        || ctx.preview_capture.lock().is_some()
        || ctx.compare_capture.lock().is_some()
        || ctx
            .compare_running
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

pub fn protected_audio_names(ctx: &AppContext) -> HashSet<String> {
    let mut names = HashSet::new();
    if let Some(id) = ctx.session_recording_id.lock().clone() {
        names.insert(format!("{id}.raw.wav"));
        names.insert(format!("{id}.processed.wav"));
        names.insert(format!("{id}.raw.wav.tmp"));
        names.insert(format!("{id}.wav.tmp"));
    }
    names.insert("filter-sample.wav".into());
    names.insert("filter-preview.wav".into());
    names.insert("filter-preview-original.wav".into());
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

pub fn hide_overlay_allowed(_state: &SessionState, expected: u64, current: u64) -> bool {
    lease_matches(expected, current)
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
        assert!(!hide_overlay_allowed(&SessionState::Idle, 2, 3));
    }
}
