use serde::Serialize;

use crate::app::machine::SessionState;

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct OverlaySnapshot {
    pub revision: u64,
    pub visible: bool,
    pub state: SessionState,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct OverlayLifecycle {
    pub revision: u64,
    pub epoch: u64,
    pub visible: bool,
}

impl OverlayLifecycle {
    pub fn publish(&mut self) -> u64 {
        self.revision = next_overlay_revision(self.revision);
        self.revision
    }

    pub fn show(&mut self) -> u64 {
        self.epoch = next_overlay_revision(self.epoch);
        self.visible = true;
        self.publish()
    }

    pub fn hide(&mut self) -> u64 {
        self.epoch = next_overlay_revision(self.epoch);
        self.visible = false;
        self.publish()
    }

    pub fn snapshot(&self, state: SessionState) -> OverlaySnapshot {
        overlay_snapshot(self.revision, self.visible, state)
    }
}

pub fn next_overlay_revision(current: u64) -> u64 {
    current.saturating_add(1)
}

pub fn accept_overlay_revision(current: u64, incoming: u64) -> bool {
    incoming > current
}

pub fn overlay_snapshot(revision: u64, visible: bool, state: SessionState) -> OverlaySnapshot {
    OverlaySnapshot {
        revision,
        visible,
        state,
    }
}

pub fn hide_should_dismiss(state: &SessionState) -> bool {
    matches!(state, SessionState::Failed { .. } | SessionState::Completed)
}

pub fn delayed_hide_is_stale(expected_epoch: u64, current_epoch: u64) -> bool {
    expected_epoch != current_epoch
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn event_newer_than_snapshot_is_accepted() {
        assert!(accept_overlay_revision(1, 2));
        assert!(!accept_overlay_revision(4, 4));
        assert!(!accept_overlay_revision(5, 4));
        assert_eq!(next_overlay_revision(4), 5);
    }

    #[test]
    fn strict_mode_double_subscribe_keeps_monotonic_revision() {
        let mut revision = 0u64;
        for incoming in [3u64, 3, 2, 4] {
            if accept_overlay_revision(revision, incoming) {
                revision = incoming;
            }
        }
        assert_eq!(revision, 4);
        let hidden = overlay_snapshot(revision, false, SessionState::Idle);
        assert!(!hidden.visible);
        assert_eq!(hidden.state, SessionState::Idle);
    }

    #[test]
    fn stale_delayed_hide_is_ignored() {
        assert!(delayed_hide_is_stale(3, 4));
        assert!(!delayed_hide_is_stale(4, 4));
    }

    #[test]
    fn start_during_error_timeout_invalidates_hide_epoch() {
        let error_epoch = 7u64;
        let start_epoch = next_overlay_revision(error_epoch);
        assert!(delayed_hide_is_stale(error_epoch, start_epoch));
        assert!(!hide_should_dismiss(&SessionState::Recording));
        assert!(hide_should_dismiss(&SessionState::Failed {
            message: "x".into(),
            code: "InvalidApiKey".into(),
        }));
    }

    #[test]
    fn hud_status_publish_advances_revision_without_touching_epoch() {
        let mut life = OverlayLifecycle::default();
        let shown = life.show();
        assert_eq!(shown, 1);
        assert_eq!(life.epoch, 1);
        assert!(life.visible);

        let recording = life.snapshot(SessionState::Recording);
        let transcribing_rev = life.publish();
        let transcribing = life.snapshot(SessionState::Transcribing { attempt: 1 });

        assert!(accept_overlay_revision(
            recording.revision,
            transcribing.revision
        ));
        assert_eq!(transcribing_rev, 2);
        assert_eq!(life.epoch, 1);
        assert_eq!(
            transcribing.state,
            SessionState::Transcribing { attempt: 1 }
        );
        assert!(transcribing.visible);
    }

    #[test]
    fn error_hide_epoch_survives_failed_status_publish() {
        let mut life = OverlayLifecycle::default();
        life.show();
        let expected = life.epoch;
        life.publish();
        life.snapshot(SessionState::Failed {
            message: "x".into(),
            code: "InvalidApiKey".into(),
        });
        assert!(!delayed_hide_is_stale(expected, life.epoch));
        life.hide();
        assert!(delayed_hide_is_stale(expected, life.epoch));
        assert!(!life.visible);
    }
}
