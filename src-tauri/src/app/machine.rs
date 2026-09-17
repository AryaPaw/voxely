use serde::{Deserialize, Serialize};
use std::time::Duration;

use crate::error::AppError;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum SessionState {
    Idle,
    StartingRecording,
    Recording,
    StoppingRecording,
    Saving,
    ProcessingAudio,
    Transcribing { attempt: u32 },
    RetryWaiting { attempt: u32, delay_ms: u64 },
    Completed,
    Failed { message: String, code: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SessionEvent {
    StartRequested,
    CaptureReady,
    CaptureFailed(AppError),
    StopRequested,
    Saved,
    SaveFailed(AppError),
    Processed,
    ProcessFailed(AppError),
    TranscriptAttemptStarted { attempt: u32 },
    RetryScheduled { attempt: u32, delay: Duration },
    Succeeded,
    Failed(AppError),
    Dismiss,
    Cancelled,
    Shutdown,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IllegalTransition {
    pub from: SessionState,
    pub event: String,
}

impl std::fmt::Display for IllegalTransition {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "cannot apply {} while {:?}", self.event, self.from)
    }
}

pub fn apply_event(
    state: SessionState,
    event: SessionEvent,
) -> Result<SessionState, IllegalTransition> {
    let event_name = format!("{event:?}");
    let next = match (&state, &event) {
        (SessionState::Idle, SessionEvent::StartRequested) => SessionState::StartingRecording,
        (SessionState::StartingRecording, SessionEvent::CaptureReady) => SessionState::Recording,
        (SessionState::StartingRecording, SessionEvent::CaptureFailed(err)) => failed(err),
        (SessionState::StartingRecording, SessionEvent::Failed(err)) => failed(err),
        (SessionState::StartingRecording, SessionEvent::Cancelled) => SessionState::Idle,
        (SessionState::StartingRecording, SessionEvent::StopRequested) => {
            SessionState::StoppingRecording
        }
        (SessionState::StartingRecording, SessionEvent::Shutdown) => SessionState::Idle,
        (SessionState::Recording, SessionEvent::StopRequested) => SessionState::StoppingRecording,
        (SessionState::Recording, SessionEvent::CaptureFailed(err)) => failed(err),
        (SessionState::Recording, SessionEvent::Failed(err)) => failed(err),
        (SessionState::Recording, SessionEvent::Cancelled) => SessionState::Idle,
        (SessionState::Recording, SessionEvent::Shutdown) => SessionState::StoppingRecording,
        (SessionState::Idle, SessionEvent::TranscriptAttemptStarted { attempt }) => {
            SessionState::Transcribing { attempt: *attempt }
        }
        (SessionState::Failed { .. }, SessionEvent::TranscriptAttemptStarted { attempt }) => {
            SessionState::Transcribing { attempt: *attempt }
        }
        (SessionState::StoppingRecording, SessionEvent::Saved) => SessionState::Saving,
        (SessionState::StoppingRecording, SessionEvent::SaveFailed(err)) => failed(err),
        (SessionState::StoppingRecording, SessionEvent::Failed(err)) => failed(err),
        (SessionState::StoppingRecording, SessionEvent::Cancelled) => SessionState::Idle,
        (SessionState::Saving, SessionEvent::Saved) => SessionState::ProcessingAudio,
        (SessionState::Saving, SessionEvent::SaveFailed(err)) => failed(err),
        (SessionState::Saving, SessionEvent::Failed(err)) => failed(err),
        (SessionState::Saving, SessionEvent::Cancelled) => SessionState::Idle,
        (SessionState::ProcessingAudio, SessionEvent::Processed) => {
            SessionState::Transcribing { attempt: 1 }
        }
        (SessionState::ProcessingAudio, SessionEvent::ProcessFailed(err)) => failed(err),
        (SessionState::ProcessingAudio, SessionEvent::Failed(err)) => failed(err),
        (SessionState::ProcessingAudio, SessionEvent::Cancelled) => SessionState::Idle,
        (SessionState::Transcribing { .. }, SessionEvent::TranscriptAttemptStarted { attempt }) => {
            SessionState::Transcribing { attempt: *attempt }
        }
        (SessionState::Transcribing { .. }, SessionEvent::RetryScheduled { attempt, delay }) => {
            SessionState::RetryWaiting {
                attempt: *attempt,
                delay_ms: delay.as_millis() as u64,
            }
        }
        (SessionState::Transcribing { .. }, SessionEvent::Succeeded) => SessionState::Completed,
        (SessionState::Transcribing { .. }, SessionEvent::Failed(err)) => failed(err),
        (SessionState::Transcribing { .. }, SessionEvent::Cancelled) => SessionState::Idle,
        (SessionState::RetryWaiting { .. }, SessionEvent::TranscriptAttemptStarted { attempt }) => {
            SessionState::Transcribing { attempt: *attempt }
        }
        (SessionState::RetryWaiting { .. }, SessionEvent::Failed(err)) => failed(err),
        (SessionState::RetryWaiting { .. }, SessionEvent::Cancelled) => SessionState::Idle,
        (SessionState::RetryWaiting { .. }, SessionEvent::Succeeded) => SessionState::Completed,
        (SessionState::Completed, SessionEvent::Dismiss) => SessionState::Idle,
        (SessionState::Failed { .. }, SessionEvent::Dismiss) => SessionState::Idle,
        (SessionState::Failed { .. }, SessionEvent::StartRequested) => {
            SessionState::StartingRecording
        }
        (SessionState::Completed, SessionEvent::StartRequested) => SessionState::StartingRecording,
        (SessionState::Idle, SessionEvent::Dismiss) => SessionState::Idle,
        (SessionState::Idle, SessionEvent::Shutdown) => SessionState::Idle,
        (SessionState::Failed { .. }, SessionEvent::Shutdown) => SessionState::Idle,
        (SessionState::Completed, SessionEvent::Shutdown) => SessionState::Idle,
        (_, SessionEvent::Shutdown) if can_abandon(&state) => SessionState::Idle,
        _ => {
            return Err(IllegalTransition {
                from: state,
                event: event_name,
            });
        }
    };
    Ok(next)
}

fn failed(err: &AppError) -> SessionState {
    SessionState::Failed {
        message: err.user_message(),
        code: err.code().to_string(),
    }
}

fn can_abandon(state: &SessionState) -> bool {
    matches!(
        state,
        SessionState::StartingRecording
            | SessionState::StoppingRecording
            | SessionState::Saving
            | SessionState::ProcessingAudio
            | SessionState::Transcribing { .. }
            | SessionState::RetryWaiting { .. }
    )
}

pub fn is_recording_active(state: &SessionState) -> bool {
    matches!(
        state,
        SessionState::StartingRecording | SessionState::Recording | SessionState::StoppingRecording
    )
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToggleHotkeyAction {
    Start,
    Stop,
    Cancel,
    Ignore,
}

pub fn toggle_hotkey_action(state: &SessionState) -> ToggleHotkeyAction {
    match state {
        SessionState::Idle | SessionState::Failed { .. } | SessionState::Completed => {
            ToggleHotkeyAction::Start
        }
        SessionState::StartingRecording | SessionState::Recording => ToggleHotkeyAction::Stop,
        SessionState::StoppingRecording
        | SessionState::Saving
        | SessionState::ProcessingAudio
        | SessionState::Transcribing { .. }
        | SessionState::RetryWaiting { .. } => ToggleHotkeyAction::Cancel,
    }
}

pub fn is_cancellable(state: &SessionState) -> bool {
    matches!(
        state,
        SessionState::StartingRecording
            | SessionState::Recording
            | SessionState::StoppingRecording
            | SessionState::Saving
            | SessionState::ProcessingAudio
            | SessionState::Transcribing { .. }
            | SessionState::RetryWaiting { .. }
    )
}

pub fn may_commit_session(
    started_generation: u64,
    current_generation: u64,
    cancelled: bool,
) -> bool {
    !cancelled && started_generation == current_generation
}

#[cfg(test)]
mod tests {
    use super::*;

    fn walk(events: &[SessionEvent]) -> SessionState {
        events
            .iter()
            .cloned()
            .fold(SessionState::Idle, |state, event| {
                apply_event(state, event).expect("legal")
            })
    }

    #[test]
    fn happy_path() {
        let state = walk(&[
            SessionEvent::StartRequested,
            SessionEvent::CaptureReady,
            SessionEvent::StopRequested,
            SessionEvent::Saved,
            SessionEvent::Saved,
            SessionEvent::Processed,
            SessionEvent::TranscriptAttemptStarted { attempt: 1 },
            SessionEvent::Succeeded,
        ]);
        assert_eq!(state, SessionState::Completed);
    }

    #[test]
    fn failed_allows_new_start() {
        let failed = SessionState::Failed {
            message: "Нет API-ключа OpenRouter".into(),
            code: "InvalidApiKey".into(),
        };
        let next = apply_event(failed, SessionEvent::StartRequested).unwrap();
        assert_eq!(next, SessionState::StartingRecording);
    }

    #[test]
    fn processing_failed_event_is_legal() {
        let state = walk(&[
            SessionEvent::StartRequested,
            SessionEvent::CaptureReady,
            SessionEvent::StopRequested,
            SessionEvent::Saved,
            SessionEvent::Saved,
            SessionEvent::Failed(AppError::InvalidApiKey),
        ]);
        match state {
            SessionState::Failed { code, .. } => assert_eq!(code, "InvalidApiKey"),
            other => panic!("expected failed, got {other:?}"),
        }
    }

    #[test]
    fn double_start_rejected() {
        let state = apply_event(SessionState::Idle, SessionEvent::StartRequested).unwrap();
        let err = apply_event(state, SessionEvent::StartRequested).unwrap_err();
        assert!(err.event.contains("StartRequested"));
    }

    #[test]
    fn stop_during_start() {
        let state = walk(&[SessionEvent::StartRequested, SessionEvent::StopRequested]);
        assert_eq!(state, SessionState::StoppingRecording);
    }

    #[test]
    fn retry_then_success() {
        let state = walk(&[
            SessionEvent::StartRequested,
            SessionEvent::CaptureReady,
            SessionEvent::StopRequested,
            SessionEvent::Saved,
            SessionEvent::Saved,
            SessionEvent::Processed,
            SessionEvent::RetryScheduled {
                attempt: 1,
                delay: Duration::from_millis(500),
            },
            SessionEvent::TranscriptAttemptStarted { attempt: 2 },
            SessionEvent::Succeeded,
            SessionEvent::Dismiss,
        ]);
        assert_eq!(state, SessionState::Idle);
    }

    #[test]
    fn cancel_from_recording_returns_idle() {
        let state = walk(&[
            SessionEvent::StartRequested,
            SessionEvent::CaptureReady,
            SessionEvent::Cancelled,
        ]);
        assert_eq!(state, SessionState::Idle);
    }

    #[test]
    fn cancel_from_transcribing_returns_idle() {
        let state = walk(&[
            SessionEvent::StartRequested,
            SessionEvent::CaptureReady,
            SessionEvent::StopRequested,
            SessionEvent::Saved,
            SessionEvent::Saved,
            SessionEvent::Processed,
            SessionEvent::Cancelled,
        ]);
        assert_eq!(state, SessionState::Idle);
        assert!(!is_cancellable(&state));
    }

    #[test]
    fn transcribing_is_cancellable() {
        assert!(is_cancellable(&SessionState::Transcribing { attempt: 1 }));
        assert!(is_cancellable(&SessionState::ProcessingAudio));
        assert!(is_cancellable(&SessionState::RetryWaiting {
            attempt: 1,
            delay_ms: 500,
        }));
        assert!(!is_recording_active(&SessionState::Transcribing {
            attempt: 1
        }));
    }

    #[test]
    fn transcribing_hotkey_cancels() {
        assert_eq!(
            toggle_hotkey_action(&SessionState::Transcribing { attempt: 1 }),
            ToggleHotkeyAction::Cancel
        );
        assert_eq!(
            toggle_hotkey_action(&SessionState::Idle),
            ToggleHotkeyAction::Start
        );
        assert_eq!(
            toggle_hotkey_action(&SessionState::Recording),
            ToggleHotkeyAction::Stop
        );
    }

    #[test]
    fn idle_save_does_not_revive_pipeline() {
        let err = apply_event(SessionState::Idle, SessionEvent::Saved).unwrap_err();
        assert!(err.event.contains("Saved"));
    }

    #[test]
    fn retry_from_idle_enters_transcribing() {
        let state = walk(&[SessionEvent::TranscriptAttemptStarted { attempt: 1 }]);
        assert_eq!(state, SessionState::Transcribing { attempt: 1 });
        assert!(is_cancellable(&state));
    }

    #[test]
    fn late_success_is_rejected_after_generation_bump() {
        assert!(may_commit_session(3, 3, false));
        assert!(!may_commit_session(3, 4, false));
        assert!(!may_commit_session(3, 3, true));
    }

    #[test]
    fn rapid_start_stop_is_legal() {
        let state = walk(&[
            SessionEvent::StartRequested,
            SessionEvent::StopRequested,
            SessionEvent::Saved,
            SessionEvent::SaveFailed(AppError::StorageFailed("disk".into())),
        ]);
        match state {
            SessionState::Failed { .. } => {}
            other => panic!("expected failed, got {other:?}"),
        }
    }
}
