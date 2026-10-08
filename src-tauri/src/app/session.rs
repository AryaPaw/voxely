use parking_lot::Mutex;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use tauri::{
    AppHandle, Emitter, LogicalSize, Manager, PhysicalPosition, Position, Size, WebviewUrl,
    WebviewWindow, WebviewWindowBuilder,
};

use crate::app::machine::{
    apply_event, is_cancellable, is_recording_active, may_commit_session, toggle_hotkey_action,
    SessionEvent, SessionState, ToggleHotkeyAction,
};
use crate::app::overlay::{
    overlay_physical_position, OverlayTimeline, WorkArea, OVERLAY_GAP_PX, OVERLAY_HEIGHT,
    OVERLAY_WIDTH,
};
use crate::app::overlay_controller::{delayed_hide_is_stale, OverlayLifecycle, OverlaySnapshot};
use crate::audio::capture::{
    read_pcm16_wav_with_rate, write_pcm16_wav, CaptureCleanupHandle, CaptureSession,
    CaptureStopOutcome, MeterSample,
};
use crate::audio::devices::list_input_devices;
use crate::dsp::metrics::{SAMPLE_RATE, STT_SAMPLE_RATE};
use crate::dsp::pipeline::{prepare_listen_audio, samples_for_stt};
use crate::error::AppError;
use crate::history::repository::{
    audio_dir, can_retry_from_history, new_recording, HistoryRepo, RecordingStatus,
};
use crate::history::retention::{
    apply_retention, cleanup_orphans, cleanup_orphans_except, delete_recording_and_usage, Retention,
};
use crate::settings::AppSettings;
use crate::transcription::openrouter::{
    transcribe_file_with_progress, OpenRouterTransport, SttProgress,
};
use crate::windows_int::credentials::get_api_key;
use crate::windows_int::overlay::{work_area_for_cursor, work_area_for_hwnd};
use crate::windows_int::text_injector::{
    insert_outcome_event, insert_should_abort, insert_transcript_now, native, CapturedTarget,
    InsertPolicy, InsertWorld, NativeHwnd,
};

type ShutdownCaptureCompletion = Arc<Mutex<Option<Box<dyn FnOnce() + Send>>>>;
const SHUTDOWN_CAPTURE_CLEANUP_BOUND: Duration = Duration::from_secs(30);
const CANCELLED_CLEANUP_MAX_ATTEMPTS: u32 = 8;

pub struct AppContext {
    pub settings_path: PathBuf,
    pub data_dir: PathBuf,
    pub settings: Mutex<AppSettings>,
    pub state: Mutex<SessionState>,
    pub capture: Mutex<Option<CaptureSession>>,
    pub capture_cleanup: Mutex<Option<CaptureCleanupHandle>>,
    pub history: Mutex<HistoryRepo>,
    pub admission: Mutex<()>,
    pub settings_write_gate: Mutex<()>,
    pub captured_target: Mutex<Option<CapturedTarget>>,
    pub overlay: Mutex<OverlayLifecycle>,
    pub in_flight: Mutex<HashSet<String>>,
    pub transport: Mutex<OpenRouterTransport>,
    pub overlay_timeline: Mutex<OverlayTimeline>,
    pub session_lifecycle: Mutex<()>,
    pub cancel_tx: Mutex<Option<tokio::sync::watch::Sender<bool>>>,
    pub retry_cancel: Mutex<HashMap<String, tokio::sync::watch::Sender<bool>>>,
    pub session_recording_id: Mutex<Option<String>>,
    pub hotkeys_suspended: Mutex<bool>,
    pub abort_start: AtomicBool,
    pub session_generation: AtomicU64,
    pub shortcut_sync_generation: AtomicU64,
    pub applied_shortcut_layout: Mutex<Option<crate::app::shortcuts::ShortcutLayout>>,
    pub pending_session_command: Mutex<Option<crate::app::shortcuts::SessionCommand>>,
    pub preview_capture: Mutex<Option<CaptureSession>>,
    pub compare_capture: Mutex<Option<CaptureSession>>,
    pub compare_cancel: Mutex<Option<tokio::sync::watch::Sender<bool>>>,
    pub compare_state: Mutex<crate::app::compare::CompareState>,
    pub compare_running: AtomicBool,
    pub capture_starting: AtomicBool,
    pub pending_native_capture_starts: AtomicUsize,
    pub capture_stopping: AtomicBool,
    pub shutdown_requested: AtomicBool,
    pub shutdown_action: Mutex<Option<crate::app::shutdown::Action>>,
    pub shutdown_cleanup_started: AtomicBool,
    pub shutdown_exit_scheduled: AtomicBool,
    pub shutdown_force_exit: AtomicBool,
    pub meter_monitor: Mutex<Option<CaptureSession>>,
    pub meter_starting: Arc<AtomicBool>,
    pub meter_generation: AtomicU64,
    pub meter_owner: Mutex<Option<String>>,
    pub insert_in_flight: AtomicBool,
    pub filter_generation: AtomicU64,
    pub update_gate: tokio::sync::Mutex<bool>,
    pub update_installing: AtomicBool,
    pub settings_recovery: Mutex<bool>,
    pub filter_recording: AtomicBool,
    cancelled_deletion_retries: Arc<Mutex<HashSet<String>>>,
}

impl AppContext {
    pub fn initialize(data_dir: PathBuf) -> Result<Self, AppError> {
        std::fs::create_dir_all(&data_dir).map_err(|e| AppError::StorageFailed(e.to_string()))?;
        let settings_path = data_dir.join("settings.json");
        let (settings, recovered) = AppSettings::load_with_recovery(&settings_path)?;
        let audio = audio_dir(&data_dir);
        std::fs::create_dir_all(&audio).map_err(|e| AppError::StorageFailed(e.to_string()))?;
        let history = HistoryRepo::open(&data_dir.join("history.sqlite"))?;
        if let Err(err) =
            crate::history::retention::cleanup_staged_audio_deletions(&audio, &history)
        {
            tracing::warn!(error = %err, "recover pending staged audio deletions");
        }
        cleanup_startup_cancelled_recordings(&history, &audio);
        recover_stale_processing(&history, &audio)?;
        if !recovered {
            cleanup_orphans(&audio, &history)?;
            apply_retention(
                &history,
                &audio,
                Retention::from_setting(&settings.retention),
                settings.storage_limit_bytes(),
            )?;
        }
        let transport = OpenRouterTransport::new(settings.retry.to_policy().connect_timeout)?;
        Ok(Self {
            settings_path,
            data_dir,
            settings: Mutex::new(settings),
            state: Mutex::new(SessionState::Idle),
            capture: Mutex::new(None),
            capture_cleanup: Mutex::new(None),
            history: Mutex::new(history),
            admission: Mutex::new(()),
            settings_write_gate: Mutex::new(()),
            captured_target: Mutex::new(None),
            overlay: Mutex::new(OverlayLifecycle::default()),
            in_flight: Mutex::new(HashSet::new()),
            transport: Mutex::new(transport),
            overlay_timeline: Mutex::new(OverlayTimeline::default()),
            session_lifecycle: Mutex::new(()),
            cancel_tx: Mutex::new(None),
            retry_cancel: Mutex::new(HashMap::new()),
            session_recording_id: Mutex::new(None),
            hotkeys_suspended: Mutex::new(false),
            abort_start: AtomicBool::new(false),
            session_generation: AtomicU64::new(0),
            shortcut_sync_generation: AtomicU64::new(0),
            applied_shortcut_layout: Mutex::new(None),
            pending_session_command: Mutex::new(None),
            preview_capture: Mutex::new(None),
            compare_capture: Mutex::new(None),
            compare_cancel: Mutex::new(None),
            compare_state: Mutex::new(crate::app::compare::CompareState::default()),
            compare_running: AtomicBool::new(false),
            capture_starting: AtomicBool::new(false),
            pending_native_capture_starts: AtomicUsize::new(0),
            capture_stopping: AtomicBool::new(false),
            shutdown_requested: AtomicBool::new(false),
            shutdown_action: Mutex::new(None),
            shutdown_cleanup_started: AtomicBool::new(false),
            shutdown_exit_scheduled: AtomicBool::new(false),
            shutdown_force_exit: AtomicBool::new(false),
            meter_monitor: Mutex::new(None),
            meter_starting: Arc::new(AtomicBool::new(false)),
            meter_generation: AtomicU64::new(0),
            meter_owner: Mutex::new(None),
            insert_in_flight: AtomicBool::new(false),
            filter_generation: AtomicU64::new(0),
            update_gate: tokio::sync::Mutex::new(false),
            update_installing: AtomicBool::new(false),
            settings_recovery: Mutex::new(recovered),
            filter_recording: AtomicBool::new(false),
            cancelled_deletion_retries: Arc::new(Mutex::new(HashSet::new())),
        })
    }

    pub fn overlay_snapshot(&self) -> OverlaySnapshot {
        let state = self.state.lock().clone();
        self.overlay.lock().snapshot(state)
    }

    pub fn emit_overlay(&self, app: &AppHandle) {
        let _ = app.emit_to("overlay", "overlay://snapshot", self.overlay_snapshot());
    }

    pub fn emit_state(&self, app: &AppHandle) {
        let state = self.state.lock().clone();
        let _ = app.emit_to("main", "session://state", state);
        self.overlay.lock().publish();
        self.emit_overlay(app);
    }

    pub fn emit_history(&self, app: &AppHandle) {
        let _ = app.emit_to("main", "history://changed", ());
    }

    pub fn transition(&self, event: SessionEvent) -> Result<SessionState, AppError> {
        let mut guard = self.state.lock();
        let next = apply_event(guard.clone(), event)
            .map_err(|e| AppError::IllegalTransition(e.to_string()))?;
        *guard = next.clone();
        Ok(next)
    }

    pub fn clone_transport_client(&self) -> Result<reqwest::Client, AppError> {
        let timeout = self.settings.lock().retry.to_policy().connect_timeout;
        let mut transport = self.transport.lock();
        transport.sync(timeout)?;
        Ok(transport.client())
    }
}

pub fn toggle_recording(app: &AppHandle) -> Result<(), AppError> {
    let ctx = app.state::<Arc<AppContext>>();
    let state = ctx.state.lock().clone();
    match toggle_hotkey_action(&state) {
        ToggleHotkeyAction::Start => start_recording(app),
        ToggleHotkeyAction::Stop => stop_recording(app),
        ToggleHotkeyAction::Ignore => Ok(()),
    }
}

pub fn cancel_recording(app: &AppHandle) -> Result<(), AppError> {
    let ctx = app.state::<Arc<AppContext>>();
    let _session_lifecycle = ctx.session_lifecycle.lock();
    let _admission = crate::app::operations::lock_admission(&ctx);
    ctx.session_generation.fetch_add(1, Ordering::SeqCst);
    if let Some(tx) = ctx.cancel_tx.lock().as_ref() {
        let _ = tx.send(true);
    }
    cancel_history_retries(&ctx);
    let state = ctx.state.lock().clone();
    match state {
        SessionState::StartingRecording | SessionState::Recording => {
            if ctx.transition(SessionEvent::Cancelled).is_err() {
                return Ok(());
            }
            let session = ctx.capture.lock().take();
            let recording_id = ctx.session_recording_id.lock().clone();
            if session.is_some() {
                ctx.capture_stopping.store(true, Ordering::SeqCst);
            }
            let native_start_pending =
                session.is_none() && ctx.capture_starting.load(Ordering::SeqCst);
            if session.is_some() || native_start_pending {
                if let Some(id) = recording_id.as_deref() {
                    discard_pending_capture_recording_locked(&ctx, id);
                } else {
                    *ctx.captured_target.lock() = None;
                    ctx.cancel_tx.lock().take();
                }
            } else {
                discard_uncommitted_recording_locked(&ctx);
            }
            if let Some(session) = session {
                let retry_context = ctx.inner().clone();
                let retry_app = app.clone();
                thread::spawn(move || {
                    stop_cancelled_capture(
                        retry_context,
                        Some(retry_app),
                        session,
                        recording_id,
                        crate::audio::capture::JOIN_BOUND,
                    );
                });
            }
            ctx.abort_start.store(true, Ordering::SeqCst);
            *ctx.captured_target.lock() = None;
            ctx.emit_history(app);
            ctx.emit_state(app);
            play_cancel_cue(&ctx);
            hide_overlay_now(app);
            Ok(())
        }
        SessionState::StoppingRecording => {
            discard_stopping_recording_locked(&ctx);
            ctx.emit_history(app);
            if ctx.transition(SessionEvent::Cancelled).is_err() {
                return Ok(());
            }
            ctx.emit_state(app);
            play_cancel_cue(&ctx);
            hide_overlay_now(app);
            Ok(())
        }
        SessionState::Saving
        | SessionState::ProcessingAudio
        | SessionState::Transcribing { .. }
        | SessionState::RetryWaiting { .. } => {
            fail_active_recording_locked(&ctx, &AppError::Cancelled);
            ctx.emit_history(app);
            if ctx.transition(SessionEvent::Cancelled).is_err() {
                return Ok(());
            }
            ctx.emit_state(app);
            play_cancel_cue(&ctx);
            hide_overlay_now(app);
            Ok(())
        }
        _ => {
            if ctx.insert_in_flight.load(Ordering::SeqCst) {
                // Success has committed already; Escape aborts delivery and still
                // hides the previous HUD without deleting completed history.
                hide_overlay_now(app);
            }
            Ok(())
        }
    }
}

fn stop_cancelled_capture(
    ctx: Arc<AppContext>,
    app: Option<AppHandle>,
    session: CaptureSession,
    recording_id: Option<String>,
    join_bound: Duration,
) {
    let late_ctx = Arc::clone(&ctx);
    let late_app = app.clone();
    let late_id = recording_id.clone();
    let outcome = session.stop_with_late_result_timeout(join_bound, move |result| {
        finish_cancelled_capture_stop(late_ctx, late_app, late_id, result);
    });
    match outcome {
        CaptureStopOutcome::Completed(result) => {
            finish_cancelled_capture_stop(ctx, app, recording_id, result);
        }
        CaptureStopOutcome::TimedOut(err) => {
            tracing::warn!(error = %err, "cancelled capture remains detached; its callback owns final cleanup");
        }
    }
}

fn finish_cancelled_capture_stop(
    ctx: Arc<AppContext>,
    app: Option<AppHandle>,
    recording_id: Option<String>,
    result: Result<crate::audio::capture::CaptureResult, AppError>,
) {
    match result {
        Ok(result) => {
            remove_capture_artifacts(&result.path);
        }
        Err(err) => tracing::warn!(error = %err, "cancelled capture did not stop cleanly"),
    }
    let Some(recording_id) = recording_id else {
        ctx.capture_stopping.store(false, Ordering::SeqCst);
        if let Some(app) = app.as_ref() {
            schedule_deferred_shutdown_exit(app, &ctx);
        }
        return;
    };
    let _ = discard_cancelled_recording(&ctx, &recording_id);
    clear_live_session(&ctx, &recording_id);
    ctx.capture_stopping.store(false, Ordering::SeqCst);
    if let Some(app) = app.as_ref() {
        schedule_deferred_shutdown_exit(app, &ctx);
    }
}

fn discard_pending_capture_recording_locked(ctx: &AppContext, recording_id: &str) {
    discard_pending_capture_recording_row(ctx, recording_id);
    if let Some(cleanup) = ctx.capture_cleanup.lock().as_ref() {
        cleanup.discard_late_result();
    }
    *ctx.captured_target.lock() = None;
    ctx.cancel_tx.lock().take();
}

fn discard_uncommitted_recording(ctx: &AppContext) {
    let _session_lifecycle = ctx.session_lifecycle.lock();
    discard_uncommitted_recording_locked(ctx);
}

fn discard_uncommitted_recording_locked(ctx: &AppContext) {
    let id = ctx.session_recording_id.lock().take();
    let discarded = id
        .as_deref()
        .is_none_or(|recording_id| discard_cancelled_recording(ctx, recording_id));
    if discarded {
        if let Some(cleanup) = ctx.capture_cleanup.lock().as_ref() {
            cleanup.discard_late_result();
        }
    }
    ctx.capture_cleanup.lock().take();
    *ctx.captured_target.lock() = None;
    ctx.cancel_tx.lock().take();
}

fn discard_stopping_recording_locked(ctx: &AppContext) {
    if !ctx.capture_stopping.load(Ordering::SeqCst) {
        fail_active_recording_locked(ctx, &AppError::Cancelled);
        discard_uncommitted_recording_locked(ctx);
    } else if let Some(id) = ctx.session_recording_id.lock().clone() {
        discard_pending_capture_recording_locked(ctx, &id);
    } else {
        *ctx.captured_target.lock() = None;
        ctx.cancel_tx.lock().take();
    }
}

fn remove_audio_file(path: &Path) {
    if let Err(err) = std::fs::remove_file(path) {
        if err.kind() != std::io::ErrorKind::NotFound {
            tracing::warn!(error = %err, file = %path.display(), "remove discarded audio");
        }
    }
}

fn remove_capture_artifacts(path: &Path) {
    if path.as_os_str().is_empty() {
        return;
    }
    remove_audio_file(path);
    remove_audio_file(&path.with_extension("wav.tmp"));
}

fn cleanup_failed_wav_read(ctx: &AppContext, recording_id: &str, path: &Path) {
    let referenced = match ctx.history.lock().get(recording_id) {
        Ok(Some(recording)) => path.file_name().is_some_and(|file_name| {
            let file_name = file_name.to_string_lossy();
            recording.raw_audio_path.as_deref() == Some(file_name.as_ref())
                || recording.processed_audio_path.as_deref() == Some(file_name.as_ref())
        }),
        Ok(None) => false,
        Err(err) => {
            tracing::warn!(error = %err, recording_id, "preserve WAV after history lookup failure");
            true
        }
    };

    remove_audio_file(&path.with_extension("wav.tmp"));
    if !referenced {
        remove_audio_file(path);
    }
}

fn cleanup_recording_audio(audio_root: &Path, recording_id: &str) {
    if uuid::Uuid::parse_str(recording_id).is_err() {
        tracing::warn!(recording_id, "skip cleanup for invalid recording id");
        return;
    }
    for name in [
        format!("{recording_id}.raw.wav"),
        format!("{recording_id}.processed.wav"),
        format!("{recording_id}.raw.stt.wav"),
        format!("{recording_id}.processed.stt.wav"),
    ] {
        remove_capture_artifacts(&audio_root.join(name));
    }
    let staged_prefix = format!("{recording_id}.");
    match std::fs::read_dir(audio_root) {
        Ok(entries) => {
            for entry in entries.filter_map(Result::ok) {
                let name = entry.file_name().to_string_lossy().into_owned();
                if name.starts_with(&staged_prefix) && name.contains(".voxely-delete-pending-") {
                    remove_audio_file(&entry.path());
                }
            }
        }
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
        Err(err) => {
            tracing::warn!(error = %err, path = %audio_root.display(), "list pending cancelled audio cleanup")
        }
    }
}

fn recording_audio_cleanup_complete(audio_root: &Path, recording_id: &str) -> bool {
    if uuid::Uuid::parse_str(recording_id).is_err() {
        return false;
    }
    for name in [
        format!("{recording_id}.raw.wav"),
        format!("{recording_id}.processed.wav"),
        format!("{recording_id}.raw.stt.wav"),
        format!("{recording_id}.processed.stt.wav"),
        format!("{recording_id}.raw.wav.tmp"),
        format!("{recording_id}.processed.wav.tmp"),
        format!("{recording_id}.raw.stt.wav.tmp"),
        format!("{recording_id}.processed.stt.wav.tmp"),
    ] {
        if !path_absent(&audio_root.join(name)) {
            return false;
        }
    }
    let entries = match std::fs::read_dir(audio_root) {
        Ok(entries) => entries,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return true,
        Err(_) => return false,
    };
    !entries.filter_map(Result::ok).any(|entry| {
        let name = entry.file_name().to_string_lossy().into_owned();
        name.starts_with(&format!("{recording_id}.")) && name.contains(".voxely-delete-pending-")
    })
}

fn cleanup_startup_cancelled_recordings(history: &HistoryRepo, audio_root: &Path) {
    let cancelled = match history.list_all() {
        Ok(records) => records
            .into_iter()
            .filter(|recording| recording.last_error_code.as_deref() == Some("Cancelled"))
            .map(|recording| recording.id)
            .collect::<Vec<_>>(),
        Err(err) => {
            tracing::warn!(error = %err, "list cancelled records for startup cleanup");
            return;
        }
    };
    for recording_id in cancelled {
        if !discard_cancelled_recording_from_repo(history, audio_root, &recording_id) {
            tracing::warn!(recording_id, "cancelled startup cleanup remains pending");
        }
    }
}

fn path_absent(path: &Path) -> bool {
    match std::fs::symlink_metadata(path) {
        Ok(_) => false,
        Err(err) => err.kind() == std::io::ErrorKind::NotFound,
    }
}

fn cleanup_stale_pipeline_audio(ctx: &AppContext, recording_id: &str) {
    if uuid::Uuid::parse_str(recording_id).is_err() {
        tracing::warn!(
            recording_id,
            "skip stale pipeline cleanup for invalid recording id"
        );
        return;
    }

    let audio_root = audio_dir(&ctx.data_dir);
    let recording = ctx.history.lock().get(recording_id);
    match recording {
        Ok(None) => cleanup_recording_audio(&audio_root, recording_id),
        Ok(Some(rec)) => {
            for name in [
                format!("{recording_id}.raw.wav"),
                format!("{recording_id}.processed.wav"),
            ] {
                let is_retained = rec.raw_audio_path.as_deref() == Some(name.as_str())
                    || rec.processed_audio_path.as_deref() == Some(name.as_str());
                if !is_retained {
                    remove_capture_artifacts(&audio_root.join(name));
                }
            }
            for name in [
                format!("{recording_id}.raw.stt.wav"),
                format!("{recording_id}.processed.stt.wav"),
            ] {
                remove_capture_artifacts(&audio_root.join(name));
            }
        }
        Err(err) => {
            tracing::warn!(error = %err, recording_id, "preserve audio after stale pipeline history lookup failed");
        }
    }
}

fn discard_cancelled_recording(ctx: &AppContext, recording_id: &str) -> bool {
    let audio = audio_dir(&ctx.data_dir);
    let (discarded, tombstone_pending) = {
        let history = ctx.history.lock();
        let discarded = discard_cancelled_recording_from_repo(&history, &audio, recording_id);
        let tombstone_pending = history
            .get(recording_id)
            .ok()
            .flatten()
            .is_some_and(|recording| recording.last_error_code.as_deref() == Some("Cancelled"));
        (discarded, tombstone_pending)
    };
    if !discarded || tombstone_pending {
        retry_cancelled_recording_deletion(ctx, recording_id);
    }
    discarded
}

fn retry_cancelled_recording_deletion(ctx: &AppContext, recording_id: &str) {
    let database = ctx.data_dir.join("history.sqlite");
    let audio_root = audio_dir(&ctx.data_dir);
    let pending = Arc::clone(&ctx.cancelled_deletion_retries);
    let recording_id = recording_id.to_string();
    if !pending.lock().insert(recording_id.clone()) {
        return;
    }
    thread::spawn(move || {
        let mut delay = Duration::from_millis(100);
        let mut attempt = 0u32;
        loop {
            attempt = attempt.saturating_add(1);
            let completed = match HistoryRepo::open(&database) {
                Ok(history) => {
                    let _ =
                        discard_cancelled_recording_from_repo(&history, &audio_root, &recording_id);
                    history
                        .get(&recording_id)
                        .is_ok_and(|recording| recording.is_none())
                        && recording_audio_cleanup_complete(&audio_root, &recording_id)
                }
                Err(err) => {
                    if attempt.is_power_of_two() {
                        tracing::error!(error = %err, recording_id, attempt, "retry opening history to delete cancelled recording");
                    }
                    false
                }
            };
            if completed {
                pending.lock().remove(&recording_id);
                return;
            }
            if attempt.is_power_of_two() {
                tracing::warn!(
                    recording_id,
                    attempt,
                    "retry deleting cancelled recording after storage failure"
                );
            }
            if attempt >= CANCELLED_CLEANUP_MAX_ATTEMPTS {
                tracing::error!(
                    recording_id,
                    attempt,
                    "cancelled recording cleanup remains pending; periodic maintenance will retry"
                );
                pending.lock().remove(&recording_id);
                return;
            }
            thread::sleep(delay);
            delay = (delay * 2).min(Duration::from_secs(30));
        }
    });
}

fn discard_pending_capture_recording_row(ctx: &AppContext, recording_id: &str) {
    if let Err(err) = ctx
        .history
        .lock()
        .suppress_cancelled_recording(recording_id)
    {
        tracing::error!(error = %err, recording_id, "sanitize pending cancelled recording before native capture completes");
    }
}

fn retry_pending_cancelled_recordings(ctx: &AppContext) {
    let ids = match ctx.history.lock().list_all() {
        Ok(records) => records
            .into_iter()
            .filter(|recording| recording.last_error_code.as_deref() == Some("Cancelled"))
            .map(|recording| recording.id)
            .collect::<Vec<_>>(),
        Err(err) => {
            tracing::warn!(error = %err, "list cancelled recordings for cleanup retry");
            return;
        }
    };
    let active_id = ctx.session_recording_id.lock().clone();
    for id in ids
        .into_iter()
        .filter(|id| active_id.as_deref() != Some(id))
    {
        retry_cancelled_recording_deletion(ctx, &id);
    }
}

fn discard_cancelled_recording_from_repo(
    history: &HistoryRepo,
    audio_root: &Path,
    recording_id: &str,
) -> bool {
    match history.suppress_cancelled_recording(recording_id) {
        Ok(_) => {}
        Err(err) => match history.get(recording_id) {
            Ok(None) => {
                return match crate::history::retention::cleanup_cancelled_audio(
                    audio_root,
                    recording_id,
                ) {
                    Ok(()) => true,
                    Err(cleanup_err) => {
                        tracing::warn!(error = %cleanup_err, recording_id, "cancelled audio cleanup remains pending after history deletion");
                        false
                    }
                };
            }
            Ok(Some(_)) => {
                tracing::error!(error = %err, recording_id, "could not sanitize cancelled history before audio cleanup");
                return false;
            }
            Err(check_err) => {
                tracing::error!(error = %err, check_error = %check_err, recording_id, "could not verify cancelled history before audio cleanup");
                return false;
            }
        },
    }

    if let Err(err) = crate::history::retention::cleanup_cancelled_audio(audio_root, recording_id) {
        tracing::warn!(error = %err, recording_id, "cancelled audio cleanup remains pending");
        return false;
    }

    match history.discard_cancelled_recording(recording_id) {
        Ok(_) => true,
        Err(err) => {
            tracing::warn!(error = %err, recording_id, "cancelled tombstone deletion remains pending");
            false
        }
    }
}

fn discard_stale_capture_result(ctx: &AppContext, recording_id: Option<&str>, path: &Path) {
    let Some(recording_id) = recording_id else {
        remove_capture_artifacts(path);
        return;
    };
    let recording = ctx.history.lock().get(recording_id);
    match recording {
        Ok(Some(rec)) if rec.status == RecordingStatus::Processing => {
            let _ = discard_cancelled_recording(ctx, recording_id);
        }
        Ok(Some(rec)) if rec.last_error_code.as_deref() == Some("Cancelled") => {
            let _ = discard_cancelled_recording(ctx, recording_id);
        }
        Ok(Some(_)) => {}
        Ok(None) => {
            remove_capture_artifacts(path);
            cleanup_recording_audio(&audio_dir(&ctx.data_dir), recording_id);
        }
        Err(err) => {
            tracing::warn!(error = %err, recording_id, "preserve stale capture after history lookup failure");
        }
    }
    clear_live_session(ctx, recording_id);
}

fn discard_stale_capture_completion(
    ctx: &AppContext,
    recording_id: Option<&str>,
    result: &crate::audio::capture::CaptureResult,
) {
    let Some(recording_id) = recording_id else {
        remove_capture_artifacts(&result.path);
        return;
    };
    let interrupted_without_duration = match ctx.history.lock().get(recording_id) {
        Ok(Some(rec)) => rec.status == RecordingStatus::Interrupted && rec.duration_ms <= 0,
        Ok(None) => false,
        Err(err) => {
            tracing::warn!(error = %err, recording_id, "preserve stale capture after history lookup failure");
            clear_live_session(ctx, recording_id);
            return;
        }
    };
    if interrupted_without_duration {
        persist_interrupted_capture(ctx, recording_id, result);
        clear_live_session(ctx, recording_id);
    } else {
        discard_stale_capture_result(ctx, Some(recording_id), &result.path);
    }
}

fn discard_stale_capture_failure(ctx: &AppContext, recording_id: &str) {
    let recording = ctx.history.lock().get(recording_id);
    match recording {
        Ok(Some(rec)) if rec.status == RecordingStatus::Processing => {
            discard_cancelled_recording(ctx, recording_id);
        }
        Ok(Some(rec)) if rec.last_error_code.as_deref() == Some("Cancelled") => {
            discard_cancelled_recording(ctx, recording_id);
        }
        Ok(None) => cleanup_recording_audio(&audio_dir(&ctx.data_dir), recording_id),
        Ok(Some(_)) => {}
        Err(err) => {
            tracing::warn!(error = %err, recording_id, "preserve stale capture after history lookup failure")
        }
    }
    clear_live_session(ctx, recording_id);
}

fn clear_live_session(ctx: &AppContext, recording_id: &str) {
    let _session_lifecycle = ctx.session_lifecycle.lock();
    clear_live_session_locked(ctx, recording_id);
}

fn lock_current_session_generation(
    ctx: &AppContext,
    generation: u64,
) -> Option<parking_lot::MutexGuard<'_, ()>> {
    let lifecycle = ctx.session_lifecycle.lock();
    crate::app::operations::lease_matches(generation, ctx.session_generation.load(Ordering::SeqCst))
        .then_some(lifecycle)
}

fn clear_live_session_locked(ctx: &AppContext, recording_id: &str) {
    let is_current = ctx.session_recording_id.lock().as_deref() == Some(recording_id);
    if is_current {
        *ctx.session_recording_id.lock() = None;
        ctx.capture_cleanup.lock().take();
        *ctx.captured_target.lock() = None;
        ctx.cancel_tx.lock().take();
    }
}

fn cancel_history_retries(ctx: &AppContext) {
    let senders = ctx.retry_cancel.lock();
    for tx in senders.values() {
        let _ = tx.send(true);
    }
}

pub fn signal_retry_cancel(
    senders: &Mutex<HashMap<String, tokio::sync::watch::Sender<bool>>>,
    recording_id: &str,
) -> bool {
    if let Some(tx) = senders.lock().get(recording_id) {
        let _ = tx.send(true);
        true
    } else {
        false
    }
}

pub fn cancel_history_retry(app: &AppHandle, recording_id: &str) -> Result<(), AppError> {
    let ctx = app.state::<Arc<AppContext>>();
    let session_id = ctx.session_recording_id.lock().clone();
    if session_id.as_deref() == Some(recording_id) {
        return cancel_recording(app);
    }
    let _ = signal_retry_cancel(&ctx.retry_cancel, recording_id);
    Ok(())
}

pub fn should_auto_stop_capture(state: &SessionState, capture_finished: bool) -> bool {
    matches!(state, SessionState::Recording) && capture_finished
}

fn play_cancel_cue(ctx: &AppContext) {
    if ctx.settings.lock().notifications {
        crate::audio::cue::play_dictation_cue(crate::audio::cue::CueKind::Cancel);
    }
}

fn start_recording(app: &AppHandle) -> Result<(), AppError> {
    let ctx = app.state::<Arc<AppContext>>();
    let _session_lifecycle = ctx.session_lifecycle.lock();
    let _admission = crate::app::operations::lock_admission(&ctx);
    if !ctx.settings.lock().first_run_complete {
        return Err(AppError::DisclosureRequired);
    }
    if crate::app::operations::system_busy(&ctx) {
        return Err(AppError::TranscriptionInProgress);
    }
    if !ctx.in_flight.lock().is_empty() {
        return Err(AppError::TranscriptionInProgress);
    }
    ctx.abort_start.store(false, Ordering::SeqCst);
    ctx.session_generation.fetch_add(1, Ordering::SeqCst);
    if let Some(preview) = ctx.preview_capture.lock().take() {
        ctx.filter_recording.store(false, Ordering::SeqCst);
        let _ = app.emit_to("main", "filter://sample", false);
        thread::spawn(move || {
            let _ = preview.stop_and_discard();
        });
    }
    crate::app::compare::steal_compare_capture(app);
    if !release_meter_monitor(&ctx) {
        return Err(AppError::TranscriptionInProgress);
    }
    let (tx, _) = tokio::sync::watch::channel(false);
    *ctx.cancel_tx.lock() = Some(tx);
    ctx.transition(SessionEvent::StartRequested)?;
    let skip = voxely_window_roots(app);
    let generation = ctx.session_generation.load(Ordering::SeqCst);
    *ctx.captured_target.lock() = native::capture_session_target(&skip, generation);
    ctx.overlay_timeline.lock().begin_show();
    show_overlay(app);
    schedule_keep_captured_caret(app, generation);
    ctx.emit_state(app);
    let settings = ctx.settings.lock().clone();
    if settings.notifications {
        crate::audio::cue::play_dictation_cue(crate::audio::cue::CueKind::Start);
    }
    let id = uuid::Uuid::new_v4().to_string();
    let dest = audio_dir(&ctx.data_dir).join(format!("{id}.raw.wav"));
    let mut rec = new_recording(settings.model.clone());
    rec.id = id.clone();
    rec.raw_audio_path = Some(format!("{id}.raw.wav"));
    rec.status = RecordingStatus::Processing;
    ctx.history.lock().insert(&rec).inspect_err(|err| {
        let _ = ctx.transition(SessionEvent::CaptureFailed(err.clone()));
    })?;
    *ctx.session_recording_id.lock() = Some(id.clone());
    ctx.capture_starting.store(true, Ordering::SeqCst);
    ctx.pending_native_capture_starts
        .fetch_add(1, Ordering::SeqCst);
    ctx.emit_history(app);
    let device = if settings.input_device == "default" {
        None
    } else {
        Some(settings.input_device.clone())
    };
    let app_handle = app.clone();
    let recording_id = id.clone();
    let pending_worker = Arc::new(AtomicBool::new(true));
    let late_pending_worker = Arc::clone(&pending_worker);
    let completion_ctx = ctx.inner().clone();
    let late_ctx = completion_ctx.clone();
    let late_app = app.clone();
    let late_recording_id = recording_id.clone();
    thread::spawn(move || {
        let started = CaptureSession::start_with_late_completion(
            device.as_deref(),
            dest,
            move || {
                let _lifecycle = late_ctx.session_lifecycle.lock();
                let recording = late_ctx.history.lock().get(&late_recording_id);
                let still_processing = recording.as_ref().is_ok_and(|recording| {
                    recording
                        .as_ref()
                        .is_some_and(|recording| recording.status == RecordingStatus::Processing)
                });
                if late_ctx.shutdown_requested.load(Ordering::SeqCst) && still_processing {
                    if let Err(err) =
                        mark_recording_failed(&late_ctx, &late_recording_id, &AppError::Interrupted)
                    {
                        tracing::error!(error = %err, recording_id = %late_recording_id, "persist interrupted capture after timed-out start");
                    }
                    clear_live_session_locked(&late_ctx, &late_recording_id);
                    late_pending_worker.store(false, Ordering::SeqCst);
                    release_pending_native_capture_start(&late_ctx);
                    late_ctx.capture_starting.store(false, Ordering::SeqCst);
                    drop(_lifecycle);
                    schedule_deferred_shutdown_exit(&late_app, &late_ctx);
                    return;
                }
                if late_ctx.abort_start.load(Ordering::SeqCst) {
                    let _ = discard_cancelled_recording(&late_ctx, &late_recording_id);
                    clear_live_session_locked(&late_ctx, &late_recording_id);
                    late_pending_worker.store(false, Ordering::SeqCst);
                    release_pending_native_capture_start(&late_ctx);
                    late_ctx.capture_starting.store(false, Ordering::SeqCst);
                    drop(_lifecycle);
                    schedule_deferred_shutdown_exit(&late_app, &late_ctx);
                    return;
                }
                late_pending_worker.store(false, Ordering::SeqCst);
                release_pending_native_capture_start(&late_ctx);
                late_ctx.capture_starting.store(false, Ordering::SeqCst);
                drop(_lifecycle);
                schedule_deferred_shutdown_exit(&late_app, &late_ctx);
            },
        );
        let native_worker_pending = started
            .as_ref()
            .err()
            .is_some_and(|failure| failure.native_worker_pending);
        if !native_worker_pending {
            release_pending_native_capture_start(&completion_ctx);
        }
        let started = started.map_err(|failure| failure.error);
        let handle = app_handle.clone();
        let _ = app_handle.run_on_main_thread(move || {
            finish_capture_start(
                &handle,
                started,
                generation,
                &recording_id,
                native_worker_pending,
                pending_worker,
            );
        });
    });
    Ok(())
}

fn finish_capture_start(
    app: &AppHandle,
    started: Result<CaptureSession, AppError>,
    generation: u64,
    recording_id: &str,
    native_worker_pending: bool,
    pending_worker: Arc<AtomicBool>,
) {
    let ctx = app.state::<Arc<AppContext>>();
    let lifecycle = ctx.session_lifecycle.lock();
    let admission = crate::app::operations::lock_admission(&ctx);
    if !crate::app::operations::lease_matches(
        generation,
        ctx.session_generation.load(Ordering::SeqCst),
    ) || ctx.abort_start.load(Ordering::SeqCst)
    {
        drop(admission);
        drop(lifecycle);
        let stale_ctx = ctx.inner().clone();
        match started {
            Ok(session) => {
                let stale_app = app.clone();
                let recording_id = recording_id.to_string();
                thread::spawn(move || {
                    stop_stale_capture_start(stale_ctx, Some(stale_app), session, recording_id);
                });
            }
            Err(err) => {
                finish_stale_capture_start_error(&stale_ctx, recording_id, &err);
                if !native_worker_pending || !pending_worker.load(Ordering::SeqCst) {
                    ctx.capture_starting.store(false, Ordering::SeqCst);
                }
                schedule_deferred_shutdown_exit(app, &ctx);
            }
        }
        return;
    }
    match started {
        Ok(session) => {
            *ctx.capture_cleanup.lock() = Some(session.cleanup_handle());
            if session.used_fallback_device() {
                persist_default_microphone(&ctx, app);
            }
            *ctx.capture.lock() = Some(session);
            if let Err(err) = ctx.transition(SessionEvent::CaptureReady) {
                let session = ctx.capture.lock().take();
                let recording_id = ctx.session_recording_id.lock().clone();
                drop(admission);
                drop(lifecycle);
                if let (Some(session), Some(recording_id)) = (session, recording_id.clone()) {
                    let stale_ctx = ctx.inner().clone();
                    let stale_app = app.clone();
                    thread::spawn(move || {
                        stop_stale_capture_start(stale_ctx, Some(stale_app), session, recording_id);
                    });
                } else {
                    ctx.capture_starting.store(false, Ordering::SeqCst);
                    schedule_deferred_shutdown_exit(app, &ctx);
                }
                if let Some(id) = recording_id {
                    clear_live_session(&ctx, &id);
                    ctx.emit_history(app);
                }
                let _ = ctx.transition(SessionEvent::CaptureFailed(err));
                ctx.emit_state(app);
                return;
            }
            if !native_worker_pending || !pending_worker.load(Ordering::SeqCst) {
                ctx.capture_starting.store(false, Ordering::SeqCst);
            }
            ctx.emit_state(app);
            spawn_openrouter_prewarm(&ctx);
            schedule_capture_limit_watch(app, generation);
        }
        Err(err) => {
            tracing::error!(error = %err, "capture start failed");
            let recording_id = ctx.session_recording_id.lock().clone();
            if let Some(id) = recording_id.as_deref() {
                if let Err(storage_err) = mark_recording_failed(&ctx, id, &err) {
                    tracing::error!(error = %storage_err, recording_id = id, "persist capture-start failure");
                }
                remove_capture_artifacts(&audio_dir(&ctx.data_dir).join(format!("{id}.raw.wav")));
                clear_live_session_locked(&ctx, id);
            }
            let _ = ctx.transition(SessionEvent::CaptureFailed(err.clone()));
            if !native_worker_pending || !pending_worker.load(Ordering::SeqCst) {
                ctx.capture_starting.store(false, Ordering::SeqCst);
            }
            drop(admission);
            drop(lifecycle);
            if recording_id.is_some() {
                ctx.emit_history(app);
            }
            ctx.emit_state(app);
            crate::notify::show_error(app, &err);
            schedule_error_overlay_hide(app.clone(), Duration::from_millis(2000), generation);
            schedule_deferred_shutdown_exit(app, &ctx);
        }
    }
}

fn stop_stale_capture_start(
    ctx: Arc<AppContext>,
    app: Option<AppHandle>,
    session: CaptureSession,
    recording_id: String,
) {
    stop_stale_capture_start_with_timeout(
        ctx,
        app,
        session,
        recording_id,
        crate::audio::capture::JOIN_BOUND,
    );
}

fn stop_stale_capture_start_with_timeout(
    ctx: Arc<AppContext>,
    app: Option<AppHandle>,
    session: CaptureSession,
    recording_id: String,
    join_bound: Duration,
) {
    let late_ctx = Arc::clone(&ctx);
    let late_app = app.clone();
    let late_id = recording_id.clone();
    let outcome = session.stop_with_late_result_timeout(join_bound, move |result| {
        finish_stale_capture_start_result(&late_ctx, &late_id, result);
        late_ctx.capture_starting.store(false, Ordering::SeqCst);
        if let Some(app) = late_app.as_ref() {
            schedule_deferred_shutdown_exit(app, &late_ctx);
        }
    });
    match outcome {
        CaptureStopOutcome::Completed(result) => {
            finish_stale_capture_start_result(&ctx, &recording_id, result);
            ctx.capture_starting.store(false, Ordering::SeqCst);
            if let Some(app) = app.as_ref() {
                schedule_deferred_shutdown_exit(app, &ctx);
            }
        }
        CaptureStopOutcome::TimedOut(err) => {
            tracing::warn!(error = %err, recording_id, "stale capture remains detached; its callback owns final cleanup");
        }
    }
}

fn finish_stale_capture_start_result(
    ctx: &AppContext,
    recording_id: &str,
    result: Result<crate::audio::capture::CaptureResult, AppError>,
) {
    let preserve_interrupted = ctx.shutdown_requested.load(Ordering::SeqCst)
        && ctx
            .history
            .lock()
            .get(recording_id)
            .ok()
            .flatten()
            .is_some_and(|recording| recording.status == RecordingStatus::Processing);
    if preserve_interrupted {
        match result {
            Ok(result) => persist_interrupted_capture(ctx, recording_id, &result),
            Err(err) => {
                tracing::warn!(error = %err, recording_id, "pending capture failed while the app was closing");
                if let Err(storage_err) =
                    mark_recording_failed(ctx, recording_id, &AppError::Interrupted)
                {
                    tracing::error!(error = %storage_err, recording_id, "persist interrupted pending capture failure");
                }
            }
        }
        clear_live_session(ctx, recording_id);
    } else {
        match result {
            Ok(result) => discard_stale_capture_completion(ctx, Some(recording_id), &result),
            Err(err) => {
                tracing::warn!(error = %err, recording_id, "late stale capture start failed");
                discard_stale_capture_failure(ctx, recording_id);
            }
        }
    }
}

fn finish_stale_capture_start_error(ctx: &AppContext, recording_id: &str, err: &AppError) {
    if ctx.shutdown_requested.load(Ordering::SeqCst)
        && ctx
            .history
            .lock()
            .get(recording_id)
            .ok()
            .flatten()
            .is_some_and(|recording| recording.status == RecordingStatus::Processing)
    {
        tracing::warn!(error = %err, recording_id, "capture start failed while the app was closing");
        if let Err(storage_err) = mark_recording_failed(ctx, recording_id, &AppError::Interrupted) {
            tracing::error!(error = %storage_err, recording_id, "persist interrupted capture start failure");
        }
        clear_live_session(ctx, recording_id);
    } else {
        discard_stale_capture_failure(ctx, recording_id);
    }
}

fn persist_default_microphone(ctx: &AppContext, app: &AppHandle) {
    let settings = match persist_fallback_microphone_settings(ctx) {
        Ok(Some(settings)) => settings,
        Ok(None) | Err(_) => return,
    };
    let _ = app.emit_to("main", "settings://changed", settings.clone());
    let _ = app.emit_to("overlay", "settings://changed", settings);
    crate::notify::show_error(app, &AppError::MicrophoneUnavailable);
}

fn persist_fallback_microphone_settings(
    ctx: &AppContext,
) -> Result<Option<crate::settings::AppSettings>, AppError> {
    let _writer = ctx.settings_write_gate.lock();
    let mut settings = ctx.settings.lock().clone();
    if settings.input_device == "default" {
        return Ok(None);
    }
    settings.input_device = "default".into();
    settings.write_seq = settings.write_seq.saturating_add(1);
    settings.save(&ctx.settings_path)?;
    *ctx.settings.lock() = settings.clone();
    Ok(Some(settings))
}

fn schedule_capture_limit_watch(app: &AppHandle, generation: u64) {
    let app = app.clone();
    thread::spawn(move || loop {
        thread::sleep(Duration::from_millis(50));
        let ctx = app.state::<Arc<AppContext>>();
        if !crate::app::operations::lease_matches(
            generation,
            ctx.session_generation.load(Ordering::SeqCst),
        ) {
            return;
        }
        let state = ctx.state.lock().clone();
        let finished = ctx
            .capture
            .lock()
            .as_ref()
            .is_some_and(CaptureSession::is_finished);
        if !matches!(
            state,
            SessionState::Recording | SessionState::StartingRecording
        ) {
            return;
        }
        if should_auto_stop_capture(&state, finished) {
            let handle = app.clone();
            let _ = app.run_on_main_thread(move || {
                auto_stop_truncated_capture(&handle);
            });
            return;
        }
    });
}

fn auto_stop_truncated_capture(app: &AppHandle) {
    let ctx = app.state::<Arc<AppContext>>();
    let finished = ctx
        .capture
        .lock()
        .as_ref()
        .is_some_and(CaptureSession::is_finished);
    if !should_auto_stop_capture(&ctx.state.lock(), finished) {
        return;
    }
    ctx.overlay.lock().set_limit_reached(true);
    ctx.emit_overlay(app);
    let _ = stop_recording(app);
}

fn spawn_openrouter_prewarm(ctx: &AppContext) {
    let Ok(client) = ctx.clone_transport_client() else {
        return;
    };
    tauri::async_runtime::spawn(async move {
        crate::transcription::openrouter::prewarm_connection(&client).await;
    });
}

fn take_capture_for_stop(ctx: &AppContext) -> Result<Option<CaptureSession>, AppError> {
    let _session_lifecycle = ctx.session_lifecycle.lock();
    let _admission = crate::app::operations::lock_admission(ctx);
    // Recheck after acquiring the lifecycle lock: another stop may have won.
    if !matches!(*ctx.state.lock(), SessionState::Recording) {
        return Ok(None);
    }
    let session = ctx.capture.lock().take();
    if session.is_some() {
        ctx.transition(SessionEvent::StopRequested)?;
        ctx.capture_stopping.store(true, Ordering::SeqCst);
    }
    Ok(session)
}

fn stop_recording(app: &AppHandle) -> Result<(), AppError> {
    let ctx = app.state::<Arc<AppContext>>();
    let session = take_capture_for_stop(&ctx)?;
    let Some(session) = session else {
        return Ok(());
    };
    /*
     * The capture stop runs outside the admission and lifecycle locks. Their
     * reservation remains visible through capture_stopping until this worker
     * completes, so another operation cannot start in the gap.
     */
    ctx.emit_state(app);
    refresh_insert_target(app);
    if ctx.settings.lock().notifications {
        crate::audio::cue::play_dictation_cue(crate::audio::cue::CueKind::Stop);
    }
    let generation = ctx.session_generation.load(Ordering::SeqCst);
    let recording_id = ctx.session_recording_id.lock().clone();
    let app_handle = app.clone();
    thread::spawn(move || {
        let late_handle = app_handle.clone();
        let late_recording_id = recording_id.clone();
        match session.stop_with_late_result(move |result| {
            dispatch_capture_stop_result(late_handle, result, generation, late_recording_id);
        }) {
            CaptureStopOutcome::Completed(result) => {
                dispatch_capture_stop_result(app_handle, result, generation, recording_id);
            }
            CaptureStopOutcome::TimedOut(err) => {
                tracing::warn!(error = %err, recording_id = ?recording_id, "capture stop timed out; keep the recording reservation until the native worker exits");
                let handle = app_handle.clone();
                let _ = app_handle.run_on_main_thread(move || {
                    let ctx = handle.state::<Arc<AppContext>>();
                    if crate::app::operations::lease_matches(
                        generation,
                        ctx.session_generation.load(Ordering::SeqCst),
                    ) && ctx.capture_stopping.load(Ordering::SeqCst)
                    {
                        crate::notify::show_error(&handle, &err);
                    }
                });
            }
        }
    });
    Ok(())
}

fn dispatch_capture_stop_result(
    app: AppHandle,
    result: Result<crate::audio::capture::CaptureResult, AppError>,
    generation: u64,
    recording_id: Option<String>,
) {
    let handle = app.clone();
    let _ = app.run_on_main_thread(move || {
        let ctx = handle.state::<Arc<AppContext>>();
        match result {
            Ok(result) => finish_stop(&handle, result, generation, recording_id.as_deref()),
            Err(err) => finish_stop_error(&handle, err, generation, recording_id.as_deref()),
        }
        ctx.capture_stopping.store(false, Ordering::SeqCst);
        schedule_deferred_shutdown_exit(&handle, &ctx);
    });
}

fn finish_stop_error(app: &AppHandle, err: AppError, generation: u64, recording_id: Option<&str>) {
    let ctx = app.state::<Arc<AppContext>>();
    if !crate::app::operations::lease_matches(
        generation,
        ctx.session_generation.load(Ordering::SeqCst),
    ) {
        if let Some(id) = recording_id {
            discard_stale_capture_failure(&ctx, id);
        }
        return;
    }
    if let Some(id) = recording_id {
        if let Err(storage_err) = mark_recording_failed(&ctx, id, &err) {
            tracing::error!(error = %storage_err, recording_id = id, "persist capture-stop failure");
        }
        clear_live_session(&ctx, id);
        ctx.emit_history(app);
    }
    let _ = ctx.transition(SessionEvent::SaveFailed(err.clone()));
    ctx.emit_state(app);
    crate::notify::show_error(app, &err);
    schedule_error_overlay_hide(app.clone(), Duration::from_millis(2000), generation);
}

fn finish_stop(
    app: &AppHandle,
    result: crate::audio::capture::CaptureResult,
    generation: u64,
    recording_id: Option<&str>,
) {
    let ctx = app.state::<Arc<AppContext>>();
    if !crate::app::operations::lease_matches(
        generation,
        ctx.session_generation.load(Ordering::SeqCst),
    ) {
        discard_stale_capture_completion(&ctx, recording_id, &result);
        return;
    }
    let (can_continue, id) = {
        let state = ctx.state.lock().clone();
        let id = ctx.session_recording_id.lock().clone();
        (finish_stop_may_continue(&state, id.is_some()), id)
    };
    if !can_continue {
        if let Some(id) = recording_id {
            let err = match read_capture_samples(result.samples, &result.path) {
                Ok(_) => {
                    AppError::StorageFailed("capture completed outside an active session".into())
                }
                Err(err) => err,
            };
            if let Err(storage_err) = mark_recording_failed(&ctx, id, &err) {
                tracing::error!(error = %storage_err, recording_id = id, "persist out-of-session capture failure");
            }
            clear_live_session(&ctx, id);
            ctx.emit_history(app);
            let _ = ctx.transition(SessionEvent::SaveFailed(err.clone()));
            ctx.emit_state(app);
            crate::notify::show_error(app, &err);
            schedule_error_overlay_hide(app.clone(), Duration::from_millis(2000), generation);
        } else {
            remove_capture_artifacts(&result.path);
        }
        return;
    }
    let Some(id) = id else {
        remove_capture_artifacts(&result.path);
        return;
    };
    if let Err(err) = ctx.transition(SessionEvent::Saved) {
        if let Err(storage_err) = mark_recording_failed(&ctx, &id, &err) {
            tracing::error!(error = %storage_err, recording_id = id, "persist invalid save transition");
        }
        clear_live_session(&ctx, &id);
        ctx.emit_history(app);
        if let Err(state_err) = ctx.transition(SessionEvent::SaveFailed(err.clone())) {
            tracing::warn!(error = %state_err, recording_id = id, "publish invalid capture-save transition failure");
        }
        ctx.emit_state(app);
        return;
    }
    ctx.emit_state(app);
    let saved = persist_capture_result(&ctx.history.lock(), &id, &result);
    let (rec, capture_failure) = match saved {
        Ok(saved) => saved,
        Err(err) => {
            if let Err(storage_err) = mark_recording_failed(&ctx, &id, &err) {
                tracing::error!(error = %storage_err, recording_id = id, "persist capture-save failure");
            }
            clear_live_session(&ctx, &id);
            ctx.emit_history(app);
            let _ = ctx.transition(SessionEvent::SaveFailed(err.clone()));
            ctx.emit_state(app);
            crate::notify::show_error(app, &err);
            schedule_error_overlay_hide(app.clone(), Duration::from_millis(2000), generation);
            return;
        }
    };
    if result.truncated {
        ctx.overlay.lock().set_limit_reached(true);
    }
    ctx.emit_history(app);
    if let Some(err) = capture_failure {
        clear_live_session(&ctx, &id);
        let _ = ctx.transition(SessionEvent::SaveFailed(err.clone()));
        ctx.emit_state(app);
        crate::notify::show_error(app, &err);
        schedule_error_overlay_hide(app.clone(), Duration::from_millis(2000), generation);
        return;
    }
    let _ = ctx.transition(SessionEvent::Saved);
    ctx.emit_state(app);
    let app_handle = app.clone();
    let wav_path = result.path.clone();
    let inline_samples = result.samples;
    let recording_id = rec.id.clone();
    tauri::async_runtime::spawn(async move {
        let io_app = app_handle.clone();
        let io_wav_path = wav_path.clone();
        let samples = tokio::task::spawn_blocking(move || -> Result<Vec<f32>, AppError> {
            let ctx = io_app.state::<Arc<AppContext>>();
            if !crate::app::operations::lease_matches(
                generation,
                ctx.session_generation.load(Ordering::SeqCst),
            ) {
                return Err(AppError::Cancelled);
            }
            let _ = apply_configured_retention(ctx.as_ref());
            read_capture_samples(inline_samples, &io_wav_path)
        })
        .await
        .map_err(|err| AppError::AudioProcessingFailed(err.to_string()))
        .and_then(|samples| samples);
        let ctx = app_handle.state::<Arc<AppContext>>();
        if !crate::app::operations::lease_matches(
            generation,
            ctx.session_generation.load(Ordering::SeqCst),
        ) {
            cleanup_stale_pipeline_audio(&ctx, &recording_id);
            return;
        }
        let outcome = match samples {
            Ok(samples) => {
                process_and_transcribe(
                    app_handle.clone(),
                    recording_id.clone(),
                    samples,
                    generation,
                )
                .await
            }
            Err(err) => {
                cleanup_failed_wav_read(&ctx, &recording_id, &wav_path);
                Err(err)
            }
        };
        let Some(session_lifecycle) = lock_current_session_generation(&ctx, generation) else {
            // Cancellation, shutdown, or a newer take owns global session state now.
            cleanup_stale_pipeline_audio(&ctx, &recording_id);
            return;
        };
        match outcome {
            Ok(()) => {}
            Err(AppError::Cancelled) => {
                let ctx = app_handle.state::<Arc<AppContext>>();
                discard_cancelled_recording(&ctx, &recording_id);
                clear_live_session_locked(&ctx, &recording_id);
                ctx.emit_history(&app_handle);
                let _ = ctx.transition(SessionEvent::Cancelled);
                ctx.emit_state(&app_handle);
                drop(session_lifecycle);
                hide_overlay_for_generation(&app_handle, generation);
            }
            Err(err) => {
                tracing::error!(error = %err, "pipeline failed");
                let ctx = app_handle.state::<Arc<AppContext>>();
                if let Err(storage_err) = mark_recording_failed(&ctx, &recording_id, &err) {
                    tracing::error!(error = %storage_err, recording_id, "persist pipeline failure");
                }
                clear_live_session_locked(&ctx, &recording_id);
                ctx.emit_history(&app_handle);
                let _ = ctx.transition(SessionEvent::Failed(err.clone()));
                ctx.emit_state(&app_handle);
                drop(session_lifecycle);
                crate::notify::show_error(&app_handle, &err);
                schedule_error_overlay_hide(
                    app_handle.clone(),
                    Duration::from_millis(2000),
                    generation,
                );
            }
        }
    });
}

async fn process_and_transcribe(
    app: AppHandle,
    recording_id: String,
    samples: Vec<f32>,
    generation: u64,
) -> Result<(), AppError> {
    let ctx = app.state::<Arc<AppContext>>();
    let cancel = begin_live_transcription(&ctx, &recording_id, generation)?;
    let finish = || {
        ctx.in_flight.lock().remove(&recording_id);
    };
    let outcome =
        process_and_transcribe_inner(&app, &recording_id, samples, generation, cancel).await;
    finish();
    outcome
}

fn begin_live_transcription(
    ctx: &AppContext,
    recording_id: &str,
    generation: u64,
) -> Result<tokio::sync::watch::Receiver<bool>, AppError> {
    let _lifecycle = ctx.session_lifecycle.lock();
    if generation != ctx.session_generation.load(Ordering::SeqCst)
        || ctx.shutdown_requested.load(Ordering::SeqCst)
        || ctx.session_recording_id.lock().as_deref() != Some(recording_id)
    {
        return Err(AppError::Cancelled);
    }
    let cancel = ctx
        .cancel_tx
        .lock()
        .as_ref()
        .ok_or(AppError::Cancelled)?
        .subscribe();
    if *cancel.borrow() {
        return Err(AppError::Cancelled);
    }
    begin_transcription(ctx, recording_id)?;
    Ok(cancel)
}

async fn process_and_transcribe_inner(
    app: &AppHandle,
    recording_id: &str,
    samples: Vec<f32>,
    started_generation: u64,
    rx: tokio::sync::watch::Receiver<bool>,
) -> Result<(), AppError> {
    let ctx = app.state::<Arc<AppContext>>();
    let settings = ctx.settings.lock().clone();
    let rec = {
        let history = ctx.history.lock();
        history
            .get(recording_id)?
            .ok_or_else(|| AppError::StorageFailed("recording missing".into()))?
    };
    let audio_root = audio_dir(&ctx.data_dir);
    let raw_name = rec
        .raw_audio_path
        .clone()
        .ok_or_else(|| AppError::StorageFailed("raw missing".into()))?;
    let raw_path = audio_root.join(&raw_name);
    let preset = settings.active_preset();
    let processed_name = format!("{recording_id}.processed.wav");
    let processed_path = audio_root.join(&processed_name);
    let keep_original = settings.keep_original_recordings;
    let processed_pcm = tokio::task::spawn_blocking({
        let processed_path = processed_path.clone();
        move || prepare_processed_audio(preset, samples, processed_path)
    })
    .await
    .map_err(|e| AppError::AudioProcessingFailed(e.to_string()))??;
    let mut rec = {
        let _lifecycle = ctx.session_lifecycle.lock();
        if !may_commit_session(
            started_generation,
            ctx.session_generation.load(Ordering::SeqCst),
            *rx.borrow(),
        ) {
            return Err(AppError::Cancelled);
        }
        let mut rec = rec;
        rec.processed_audio_path = Some(processed_name);
        rec.updated_at = chrono::Utc::now();
        persist_processed_audio(&ctx.history.lock(), &mut rec, keep_original, &raw_path)?;
        ctx.emit_history(app);
        let _ = ctx.transition(SessionEvent::Processed);
        ctx.emit_state(app);
        rec
    };

    let key = match get_api_key()? {
        Some(key) => key,
        None => {
            rec.status = RecordingStatus::Failed;
            rec.last_error_message = Some(AppError::InvalidApiKey.user_message());
            rec.last_error_code = Some("InvalidApiKey".into());
            rec.updated_at = chrono::Utc::now();
            persist_live_recording(&ctx, started_generation, &rx, &rec)?;
            ctx.emit_history(app);
            return Err(AppError::InvalidApiKey);
        }
    };
    let language = if settings.language == "auto" {
        None
    } else {
        Some(settings.language.as_str())
    };
    rec.model = settings.model.clone();
    rec.request_started_at = Some(chrono::Utc::now());
    persist_live_recording(&ctx, started_generation, &rx, &rec)?;
    ctx.emit_history(app);
    if *rx.borrow()
        || !may_commit_session(
            started_generation,
            ctx.session_generation.load(Ordering::SeqCst),
            false,
        )
    {
        return Err(AppError::Cancelled);
    }
    let stt_dir = processed_path.clone();
    let stt_path = tokio::task::spawn_blocking(move || write_stt_pcm(&processed_pcm, &stt_dir))
        .await
        .map_err(|e| AppError::AudioProcessingFailed(e.to_string()))??;
    let transcribed = run_transcription(
        app,
        recording_id,
        &key,
        &settings.model,
        language,
        &stt_path,
        settings.retry.to_policy(),
        Duration::from_millis(rec.duration_ms as u64),
        0,
        rx.clone(),
    )
    .await;
    let _ = std::fs::remove_file(&stt_path);
    let session_lifecycle = ctx.session_lifecycle.lock();
    if !may_commit_session(
        started_generation,
        ctx.session_generation.load(Ordering::SeqCst),
        *rx.borrow(),
    ) {
        return Err(AppError::Cancelled);
    }
    match transcribed {
        Ok(success) => {
            let cancelled = *rx.borrow();
            if !may_commit_session(
                started_generation,
                ctx.session_generation.load(Ordering::SeqCst),
                cancelled,
            ) {
                return Err(AppError::Cancelled);
            }
            let transcript = crate::text_replacements::apply_text_replacements(
                success.text,
                &settings.text_replacements,
            );
            rec.status = RecordingStatus::Completed;
            rec.transcript = Some(transcript.clone());
            rec.attempt_count = success.attempt as i64;
            rec.usage_json = success.usage_json.clone();
            rec.cost = success.cost;
            rec.generation_id = success.generation_id.clone();
            rec.latency_ms = Some(success.latency_ms as i64);
            rec.completed_at = Some(chrono::Utc::now());
            rec.updated_at = chrono::Utc::now();
            ctx.history.lock().update(&rec)?;
            ctx.emit_history(app);
            tracing::info!(stt_http_ms = success.latency_ms, "stt http");
            publish_stt_success(
                app,
                ctx.as_ref(),
                started_generation,
                recording_id.to_string(),
                transcript,
                session_lifecycle,
            );
            Ok(())
        }
        Err(AppError::Cancelled) => Err(AppError::Cancelled),
        Err(err) => {
            let history = ctx.history.lock();
            if let Some(persisted) = history.get(recording_id)? {
                rec.attempt_count = rec.attempt_count.max(persisted.attempt_count);
            }
            rec.status = RecordingStatus::Failed;
            rec.last_error_message = Some(err.user_message());
            rec.last_error_code = Some(err.code().to_string());
            rec.updated_at = chrono::Utc::now();
            history.update(&rec)?;
            Err(err)
        }
    }
}

fn publish_stt_success(
    app: &AppHandle,
    ctx: &AppContext,
    started_generation: u64,
    recording_id: String,
    text: String,
    session_lifecycle: parking_lot::MutexGuard<'_, ()>,
) {
    let insertion = prepare_stt_insertion(ctx, session_lifecycle);
    // No lifecycle/admission lock may be held across a synchronous HWND getter:
    // the UI thread can be waiting for that lock to handle Escape or shutdown.
    hide_overlay_for_generation(app, started_generation);
    ctx.emit_state(app);
    let SttInsertion {
        mode,
        hotkey,
        captured,
    } = insertion;
    let app_clone = app.clone();
    let overlay_root = overlay_native_hwnd(app).map(|h| h.value);
    let main_root = native_hwnd_for_label(app, "main").map(|h| h.value);
    thread::spawn(move || {
        let abort = {
            let abort_app = app_clone.clone();
            move || {
                let ctx = abort_app.state::<Arc<AppContext>>();
                insert_should_abort(
                    started_generation,
                    ctx.session_generation.load(Ordering::SeqCst),
                )
            }
        };
        let outcome = insert_transcript_now(
            &mode,
            captured,
            overlay_root,
            main_root,
            &text,
            abort,
            &hotkey,
        );
        app_clone
            .state::<Arc<AppContext>>()
            .insert_in_flight
            .store(false, Ordering::SeqCst);
        let notify = app_clone.clone();
        let _ = app_clone.run_on_main_thread(move || {
            crate::notify::notify_insert_outcome(&notify, outcome);
            if outcome == crate::windows_int::text_injector::InsertOutcome::CancelledBeforeDelivery
            {
                let _ = notify.emit(
                    "session://insert-cancelled",
                    serde_json::json!({
                        "recordingId": recording_id,
                        "status": "cancelled_before_delivery"
                    }),
                );
            } else if let Some(event) = insert_outcome_event(outcome) {
                let _ = notify.emit("session://insert", event);
            }
        });
    });
}

struct SttInsertion {
    mode: String,
    hotkey: String,
    captured: Option<CapturedTarget>,
}

fn prepare_stt_insertion(
    ctx: &AppContext,
    session_lifecycle: parking_lot::MutexGuard<'_, ()>,
) -> SttInsertion {
    // Reserve insertion before publishing Idle. Cancellation can invalidate the
    // generation after this commit, but must never delete the completed recording.
    ctx.insert_in_flight.store(true, Ordering::SeqCst);
    let _ = ctx.transition(SessionEvent::Succeeded);
    let _ = ctx.transition(SessionEvent::Dismiss);
    *ctx.session_recording_id.lock() = None;
    ctx.capture_cleanup.lock().take();
    ctx.cancel_tx.lock().take();
    let settings = ctx.settings.lock();
    let insertion = SttInsertion {
        mode: settings.insertion_mode.clone(),
        hotkey: settings.hotkey.clone(),
        captured: ctx.captured_target.lock().take(),
    };
    drop(settings);
    drop(session_lifecycle);
    insertion
}

fn overlay_native_hwnd(app: &AppHandle) -> Option<NativeHwnd> {
    native_hwnd_for_label(app, "overlay")
}

fn native_hwnd_for_label(app: &AppHandle, label: &str) -> Option<NativeHwnd> {
    let window = app.get_webview_window(label)?;
    let hwnd = window.hwnd().ok()?;
    Some(NativeHwnd {
        value: hwnd.0 as usize,
    })
}

fn voxely_window_roots(app: &AppHandle) -> Vec<usize> {
    ["overlay", "main"]
        .into_iter()
        .filter_map(|label| native_hwnd_for_label(app, label).map(|h| h.value))
        .collect()
}

fn refresh_insert_target(app: &AppHandle) {
    let ctx = app.state::<Arc<AppContext>>();
    let overlay_root = overlay_native_hwnd(app).map(|h| h.value);
    let main_root = native_hwnd_for_label(app, "main").map(|h| h.value);
    let skip = [overlay_root, main_root]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>();
    let generation = ctx.session_generation.load(Ordering::SeqCst);
    let start = *ctx.captured_target.lock();
    let live = native::capture_session_target(&skip, generation);
    if let Some(target) = crate::windows_int::insert_engine::resolve_captured_insert_target(
        start,
        live,
        overlay_root,
        main_root,
    ) {
        *ctx.captured_target.lock() = Some(target);
    }
}

fn keep_captured_caret(app: &AppHandle) {
    let ctx = app.state::<Arc<AppContext>>();
    let Some(target) = *ctx.captured_target.lock() else {
        return;
    };
    let overlay_root = overlay_native_hwnd(app).map(|h| h.value);
    let main_root = native_hwnd_for_label(app, "main").map(|h| h.value);
    let mut world = native::LiveWorld {
        overlay_root,
        main_root,
    };
    let mut snap = world.snapshot(Some(target));
    snap.overlay_root = overlay_root;
    snap.main_root = main_root;
    snap.captured = Some(target);
    match crate::windows_int::insert_engine::classify_insert_policy(&snap) {
        InsertPolicy::SendUnicode => {
            let _ = world.focus_caret(&target);
        }
        InsertPolicy::RestoreThenSend => {
            if world.restore_foreground(&target) {
                let _ = world.focus_caret(&target);
            }
        }
        InsertPolicy::CopyOnly(_) => {}
    }
}

fn schedule_keep_captured_caret(app: &AppHandle, generation: u64) {
    let app = app.clone();
    thread::spawn(move || {
        thread::sleep(Duration::from_millis(40));
        let ctx = app.state::<Arc<AppContext>>();
        if ctx.session_generation.load(Ordering::SeqCst) != generation {
            return;
        }
        if !is_recording_active(&ctx.state.lock()) {
            return;
        }
        keep_captured_caret(&app);
    });
}

fn overlay_work_area(window: &WebviewWindow) -> WorkArea {
    let fallback = WorkArea {
        left: 0,
        top: 0,
        right: 1920,
        bottom: 1040,
    };
    if let Some(ctx) = window.try_state::<Arc<AppContext>>() {
        if let Some(target) = *ctx.captured_target.lock() {
            if let Some(work) = work_area_for_hwnd(target.hwnd as isize) {
                return work;
            }
        }
    }
    work_area_for_cursor().unwrap_or(fallback)
}

fn position_overlay(window: &WebviewWindow) {
    let work = overlay_work_area(window);
    let scale = overlay_scale_factor(window);
    let width = (OVERLAY_WIDTH * scale).round() as u32;
    let height = (OVERLAY_HEIGHT * scale).round() as u32;
    let gap = (f64::from(OVERLAY_GAP_PX) * scale).round() as i32;
    let (x, y) = overlay_physical_position(work, width, height, gap);
    let _ = window.set_size(Size::Logical(LogicalSize::new(
        OVERLAY_WIDTH,
        OVERLAY_HEIGHT,
    )));
    let _ = window.set_position(Position::Physical(PhysicalPosition::new(x, y)));
}

fn overlay_scale_factor(window: &WebviewWindow) -> f64 {
    if let Some(ctx) = window.try_state::<Arc<AppContext>>() {
        if let Some(target) = *ctx.captured_target.lock() {
            if let Some(scale) =
                crate::windows_int::overlay::dpi_scale_for_hwnd(target.hwnd as isize)
            {
                return scale;
            }
        }
    }
    window.scale_factor().unwrap_or(1.0)
}

fn reveal_overlay(window: &WebviewWindow) {
    position_overlay(window);
    decorate_overlay(window);
    let _ = window.set_ignore_cursor_events(false);
    #[cfg(windows)]
    if let Ok(hwnd) = window.hwnd() {
        crate::windows_int::overlay::show_noactivate(hwnd.0 as isize);
        return;
    }
    let _ = window.show();
}

fn show_overlay(app: &AppHandle) {
    let ctx = app.state::<Arc<AppContext>>();
    ctx.overlay.lock().show();
    ctx.overlay_timeline.lock().mark_window_created();
    if let Some(window) = app.get_webview_window("overlay") {
        reveal_overlay(&window);
        ctx.emit_overlay(app);
        return;
    }
    match build_overlay_window(app, overlay_url()) {
        Ok(window) => {
            reveal_overlay(&window);
            ctx.emit_overlay(app);
        }
        Err(err) => {
            tracing::error!(error = %err, "overlay window failed");
        }
    }
}

fn build_overlay_window(app: &AppHandle, url: WebviewUrl) -> tauri::Result<WebviewWindow> {
    WebviewWindowBuilder::new(app, "overlay", url)
        .title("Voxely Overlay")
        .decorations(false)
        .always_on_top(true)
        .skip_taskbar(true)
        .focused(false)
        .visible(false)
        .resizable(false)
        .transparent(true)
        .shadow(false)
        .inner_size(OVERLAY_WIDTH, OVERLAY_HEIGHT)
        .build()
}

fn decorate_overlay(window: &WebviewWindow) {
    let _ = window.set_ignore_cursor_events(false);
    #[cfg(windows)]
    if let Ok(hwnd) = window.hwnd() {
        crate::windows_int::overlay::apply_overlay_exstyle(hwnd.0 as isize);
    }
}

fn overlay_url() -> WebviewUrl {
    WebviewUrl::App("overlay.html".into())
}

pub fn prepare_overlay_window(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("overlay") {
        decorate_overlay(&window);
        position_overlay(&window);
        return;
    }
    match build_overlay_window(app, overlay_url()) {
        Ok(window) => {
            decorate_overlay(&window);
            position_overlay(&window);
        }
        Err(err) => {
            tracing::error!(error = %err, "overlay window failed");
        }
    }
}

fn hide_overlay_hwnd(window: &WebviewWindow) {
    let _ = window.set_ignore_cursor_events(true);
    #[cfg(windows)]
    if let Ok(hwnd) = window.hwnd() {
        crate::windows_int::overlay::hide(hwnd.0 as isize);
    }
    let _ = window.hide();
}

fn hide_overlay_now(app: &AppHandle) {
    let ctx = app.state::<Arc<AppContext>>();
    let generation = ctx.session_generation.load(Ordering::SeqCst);
    hide_overlay_for_generation(app, generation);
}

fn hide_overlay_for_generation(app: &AppHandle, expected: u64) {
    let handle = app.clone();
    if let Err(err) = app.run_on_main_thread(move || hide_overlay_on_main_thread(&handle, expected))
    {
        tracing::warn!(error = %err, "could not dispatch overlay hide");
    }
}

fn hide_overlay_on_main_thread(app: &AppHandle, expected: u64) {
    // Check and operate on HWND in the same UI turn. A background getter can
    // otherwise wait for UI while cancellation/start changes the generation.
    let ctx = app.state::<Arc<AppContext>>();
    let current = ctx.session_generation.load(Ordering::SeqCst);
    let state = ctx.state.lock().clone();
    if !crate::app::operations::hide_overlay_allowed(&state, expected, current) {
        return;
    }
    ctx.overlay.lock().hide();
    if let Some(window) = app.get_webview_window("overlay") {
        hide_overlay_hwnd(&window);
    }
    ctx.overlay_timeline.lock().hide();
    ctx.emit_overlay(app);
}

fn schedule_error_overlay_hide(app: AppHandle, delay: Duration, generation: u64) {
    let ctx = app.state::<Arc<AppContext>>();
    let expected = ctx.overlay.lock().epoch;
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(delay).await;
        let ctx = app.state::<Arc<AppContext>>();
        if delayed_hide_is_stale(expected, ctx.overlay.lock().epoch) {
            return;
        }
        hide_overlay_for_generation(&app, generation);
        let _ = ctx.transition(SessionEvent::Dismiss);
        ctx.emit_state(&app);
    });
}

pub(crate) fn shutdown_session(app: &AppHandle) -> bool {
    let ctx = app.state::<Arc<AppContext>>();
    if ctx.shutdown_force_exit.load(Ordering::SeqCst) {
        return true;
    }
    let _session_lifecycle = ctx.session_lifecycle.lock();
    if !ctx.shutdown_requested.load(Ordering::SeqCst) {
        crate::runtime_diagnostics::record(crate::runtime_diagnostics::Event::ShutdownStarted);
    }
    ctx.shutdown_requested.store(true, Ordering::SeqCst);
    ctx.shutdown_cleanup_started.store(true, Ordering::SeqCst);
    ctx.session_generation.fetch_add(1, Ordering::SeqCst);
    ctx.abort_start.store(true, Ordering::SeqCst);
    let admission = crate::app::operations::lock_admission(&ctx);
    ctx.filter_generation.fetch_add(1, Ordering::SeqCst);
    ctx.meter_generation.fetch_add(1, Ordering::SeqCst);
    if let Some(tx) = ctx.cancel_tx.lock().take() {
        let _ = tx.send(true);
    }
    let recording_id = ctx.session_recording_id.lock().clone();
    if let Some(session) = ctx.capture.lock().take() {
        ctx.capture_stopping.store(true, Ordering::SeqCst);
        stop_capture_for_shutdown(
            &ctx,
            Some(app.clone()),
            session,
            recording_id.clone(),
            crate::audio::capture::JOIN_BOUND,
        );
    } else if !ctx.capture_starting.load(Ordering::SeqCst) {
        if let Some(id) = recording_id.as_deref() {
            if let Err(err) = mark_recording_failed(&ctx, id, &AppError::Interrupted) {
                tracing::error!(error = %err, recording_id = id, "persist interrupted recording");
            }
        }
    }
    let preview = ctx.preview_capture.lock().take();
    if let Some(session) = preview {
        ctx.filter_recording.store(true, Ordering::SeqCst);
        let done_ctx = Arc::clone(&ctx);
        let done_app = app.clone();
        let stop_ctx = Arc::clone(&ctx);
        let stop_app = app.clone();
        thread::spawn(move || {
            stop_capture_discard_during_shutdown(stop_ctx, stop_app, session, move || {
                done_ctx.filter_recording.store(false, Ordering::SeqCst);
                let _ = done_app.emit_to("main", "filter://sample", false);
            });
        });
    }
    request_compare_cancel_for_shutdown(&ctx);
    let compare = ctx.compare_capture.lock().take();
    if let Some(session) = compare {
        let nonce = ctx.compare_state.lock().nonce;
        ctx.compare_state.lock().recording = true;
        let done_ctx = Arc::clone(&ctx);
        let done_app = app.clone();
        let stop_ctx = Arc::clone(&ctx);
        let stop_app = app.clone();
        thread::spawn(move || {
            stop_capture_discard_during_shutdown(stop_ctx, stop_app, session, move || {
                crate::app::compare::release_compare_capture_reservation(&done_ctx, nonce);
                let state = done_ctx.compare_state.lock().clone();
                let _ = done_app.emit_to("main", "compare://state", state);
            });
        });
    }
    *ctx.meter_owner.lock() = None;
    if let Some(session) = ctx.meter_monitor.lock().take() {
        ctx.meter_starting.store(true, Ordering::SeqCst);
        let done_ctx = Arc::clone(&ctx);
        let stop_ctx = Arc::clone(&ctx);
        let stop_app = app.clone();
        thread::spawn(move || {
            stop_capture_discard_during_shutdown(stop_ctx, stop_app, session, move || {
                done_ctx.meter_starting.store(false, Ordering::SeqCst)
            });
        });
    }
    drop(admission);
    *ctx.session_recording_id.lock() = None;
    ctx.capture_cleanup.lock().take();
    *ctx.captured_target.lock() = None;
    let _ = ctx.transition(SessionEvent::Shutdown);
    // Queued capture completion runs on the UI thread and also needs lifecycle.
    // Never retain this guard while waiting for an HWND/UI getter.
    drop(_session_lifecycle);
    hide_overlay_now(app);
    ctx.emit_state(app);
    if !shutdown_session_exit_ready(&ctx) {
        crate::runtime_diagnostics::record(
            crate::runtime_diagnostics::Event::ShutdownCleanupDeferred,
        );
    }
    shutdown_session_exit_ready(&ctx)
}

fn shutdown_capture_cleanup_pending(ctx: &AppContext) -> bool {
    ctx.capture_starting.load(Ordering::SeqCst)
        || ctx.capture_stopping.load(Ordering::SeqCst)
        || ctx.pending_native_capture_starts.load(Ordering::SeqCst) > 0
        || ctx.preview_capture.lock().is_some()
        || ctx.compare_capture.lock().is_some()
        || ctx.compare_running.load(Ordering::SeqCst)
        || ctx.compare_state.lock().recording
        || ctx.filter_recording.load(Ordering::SeqCst)
        || ctx.meter_monitor.lock().is_some()
        || ctx.meter_starting.load(Ordering::SeqCst)
}

fn shutdown_session_exit_ready(ctx: &AppContext) -> bool {
    ctx.shutdown_force_exit.load(Ordering::SeqCst) || !shutdown_capture_cleanup_pending(ctx)
}

fn request_compare_cancel_for_shutdown(ctx: &AppContext) {
    if let Some(tx) = ctx.compare_cancel.lock().as_ref() {
        tx.send_replace(true);
    }
}

pub(crate) fn stop_capture_discard_during_shutdown<F>(
    ctx: Arc<AppContext>,
    app: AppHandle,
    session: CaptureSession,
    on_complete: F,
) where
    F: FnOnce() + Send + 'static,
{
    let cleanup = session.cleanup_handle();
    cleanup.discard_late_result();
    let completion: ShutdownCaptureCompletion = Arc::new(Mutex::new(Some(Box::new(on_complete))));
    let late_cleanup = cleanup.clone();
    let late_completion = Arc::clone(&completion);
    let late_ctx = Arc::clone(&ctx);
    let late_app = app.clone();
    match session.stop_with_late_result_timeout(crate::audio::capture::JOIN_BOUND, move |_| {
        late_cleanup.discard_late_result();
        if let Some(done) = late_completion.lock().take() {
            done();
        }
        schedule_deferred_shutdown_exit(&late_app, &late_ctx);
    }) {
        CaptureStopOutcome::Completed(Err(err)) => {
            tracing::warn!(error = %err, "auxiliary capture did not finish cleanly during shutdown");
            cleanup.discard_late_result();
            if let Some(done) = completion.lock().take() {
                done();
            }
        }
        CaptureStopOutcome::Completed(Ok(_)) => {
            cleanup.discard_late_result();
            if let Some(done) = completion.lock().take() {
                done();
            }
        }
        CaptureStopOutcome::TimedOut(err) => {
            tracing::warn!(error = %err, "auxiliary capture remains detached during shutdown; its late callback owns cleanup");
        }
    }
}

fn stop_capture_for_shutdown(
    ctx: &Arc<AppContext>,
    app: Option<AppHandle>,
    session: CaptureSession,
    recording_id: Option<String>,
    join_bound: Duration,
) {
    let late_ctx = Arc::clone(ctx);
    let late_app = app.clone();
    let late_id = recording_id.clone();
    let outcome = session.stop_with_late_result_timeout(join_bound, move |result| {
        if let Some(recording_id) = late_id.as_deref() {
            persist_late_shutdown_capture(&late_ctx, recording_id, result);
        }
        late_ctx.capture_stopping.store(false, Ordering::SeqCst);
        if let Some(app) = late_app.as_ref() {
            schedule_deferred_shutdown_exit(app, &late_ctx);
        }
    });
    match outcome {
        CaptureStopOutcome::Completed(Ok(result)) => {
            if let Some(id) = recording_id.as_deref() {
                persist_late_shutdown_capture(ctx, id, Ok(result));
            }
            ctx.capture_stopping.store(false, Ordering::SeqCst);
        }
        CaptureStopOutcome::Completed(Err(err)) => {
            tracing::warn!(error = %err, "capture did not finish cleanly during shutdown");
            mark_shutdown_capture_interrupted(ctx, recording_id.as_deref());
            ctx.capture_stopping.store(false, Ordering::SeqCst);
        }
        CaptureStopOutcome::TimedOut(err) => {
            tracing::warn!(error = %err, "capture remains detached during shutdown; its late result will update history");
            mark_shutdown_capture_interrupted(ctx, recording_id.as_deref());
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
enum ShutdownExitReservation {
    Wait,
    AlreadyScheduled,
    Reserved,
}

fn reserve_shutdown_exit(ctx: &AppContext) -> ShutdownExitReservation {
    let Some(_lifecycle) = ctx.session_lifecycle.try_lock() else {
        return ShutdownExitReservation::Wait;
    };
    if ctx.shutdown_exit_scheduled.load(Ordering::SeqCst) {
        return ShutdownExitReservation::AlreadyScheduled;
    }
    if !ctx.shutdown_requested.load(Ordering::SeqCst)
        || !ctx.shutdown_cleanup_started.load(Ordering::SeqCst)
        || shutdown_capture_cleanup_pending(ctx)
        || ctx
            .shutdown_exit_scheduled
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .is_err()
    {
        return ShutdownExitReservation::Wait;
    }
    ShutdownExitReservation::Reserved
}

pub(crate) fn schedule_deferred_shutdown_exit(app: &AppHandle, ctx: &AppContext) -> bool {
    match reserve_shutdown_exit(ctx) {
        ShutdownExitReservation::Wait => return false,
        ShutdownExitReservation::AlreadyScheduled => return true,
        ShutdownExitReservation::Reserved => {}
    }
    crate::runtime_diagnostics::record(crate::runtime_diagnostics::Event::ShutdownCleanupReady);
    let exit_app = app.clone();
    let exit_ctx = app.state::<Arc<AppContext>>().inner().clone();
    if let Err(err) =
        app.run_on_main_thread(move || crate::app::shutdown::dispatch(&exit_app, &exit_ctx))
    {
        tracing::error!(error = %err, "could not dispatch app exit after capture cleanup; requesting exit directly");
        crate::app::shutdown::dispatch(app, ctx);
    }
    true
}

pub(crate) fn shutdown_capture_cleanup_expired(started: Instant, now: Instant) -> bool {
    now.checked_duration_since(started)
        .is_some_and(|elapsed| elapsed >= SHUTDOWN_CAPTURE_CLEANUP_BOUND)
}

pub(crate) fn schedule_forced_shutdown_exit(app: &AppHandle, ctx: &AppContext) {
    if ctx
        .shutdown_exit_scheduled
        .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
        .is_err()
    {
        return;
    }
    ctx.shutdown_force_exit.store(true, Ordering::SeqCst);
    crate::runtime_diagnostics::record(
        crate::runtime_diagnostics::Event::ForcedShutdownRequested {
            reason: crate::runtime_diagnostics::ShutdownReason::CaptureCleanupTimeout,
        },
    );
    let exit_app = app.clone();
    let exit_ctx = app.state::<Arc<AppContext>>().inner().clone();
    if let Err(err) =
        app.run_on_main_thread(move || crate::app::shutdown::dispatch(&exit_app, &exit_ctx))
    {
        tracing::error!(error = %err, "could not dispatch forced app exit after capture cleanup timeout; requesting exit directly");
        crate::app::shutdown::dispatch(app, ctx);
    }
}

pub(crate) fn release_pending_native_capture_start(ctx: &AppContext) {
    if ctx
        .pending_native_capture_starts
        .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |pending| {
            pending.checked_sub(1)
        })
        .is_err()
    {
        tracing::error!("pending native capture start reservation underflow");
    }
}

fn mark_shutdown_capture_interrupted(ctx: &AppContext, recording_id: Option<&str>) {
    if let Some(id) = recording_id {
        if let Err(storage_err) = mark_recording_failed(ctx, id, &AppError::Interrupted) {
            tracing::error!(error = %storage_err, recording_id = id, "persist interrupted recording during shutdown");
        }
    }
}

fn recover_stale_processing(
    history: &HistoryRepo,
    audio_root: &std::path::Path,
) -> Result<(), AppError> {
    for rec in history.list_all()? {
        let raw_tmp_exists = rec
            .raw_audio_path
            .as_ref()
            .is_some_and(|name| audio_root.join(name).with_extension("wav.tmp").is_file());
        let stale_processing = rec.status == RecordingStatus::Processing;
        let interrupted_missing_duration =
            rec.status == RecordingStatus::Interrupted && rec.duration_ms <= 0;
        if !stale_processing && !raw_tmp_exists && !interrupted_missing_duration {
            continue;
        }
        let mut rec = rec;
        let truncated = salvage_raw_tmp(audio_root, &mut rec);
        if rec.duration_ms <= 0 {
            for name in [&rec.raw_audio_path, &rec.processed_audio_path]
                .into_iter()
                .flatten()
            {
                let path = audio_root.join(name);
                if !path.is_file() {
                    continue;
                }
                match crate::audio::capture::capture_wav_duration_ms(&path) {
                    Ok(duration) if duration > 0 => {
                        rec.duration_ms = duration as i64;
                        break;
                    }
                    Ok(_) => {}
                    Err(err) => {
                        tracing::warn!(error = %err, path = %path.display(), "could not restore interrupted capture duration");
                    }
                }
            }
        }
        let has_audio = [&rec.raw_audio_path, &rec.processed_audio_path]
            .into_iter()
            .flatten()
            .any(|name| audio_root.join(name).is_file());
        if has_audio {
            rec.status = RecordingStatus::Interrupted;
            rec.last_error_code = Some(
                if truncated {
                    AppError::RecordingTruncated.code()
                } else {
                    AppError::Interrupted.code()
                }
                .into(),
            );
            rec.last_error_message = Some(if truncated {
                "Recovered capture reached the maximum recording size".into()
            } else {
                AppError::Interrupted.user_message()
            });
            rec.updated_at = chrono::Utc::now();
            history.update(&rec)?;
        } else if stale_processing {
            if let Err(err) = delete_recording_and_usage(history, audio_root, &rec) {
                tracing::warn!(error = %err, recording_id = rec.id, "preserve stale processing history and usage after atomic deletion failure");
            }
        }
    }
    Ok(())
}

fn salvage_raw_tmp(audio_root: &Path, rec: &mut crate::history::repository::Recording) -> bool {
    let Some(name) = rec.raw_audio_path.clone() else {
        return false;
    };
    let dest = audio_root.join(&name);
    if dest.is_file() {
        if let Ok(duration) = crate::audio::capture::capture_wav_duration_ms(&dest) {
            rec.duration_ms = duration as i64;
        }
        return std::fs::metadata(&dest)
            .is_ok_and(|metadata| metadata.len() >= crate::audio::capture::MAX_WAV_BYTES);
    }
    let tmp = dest.with_extension("wav.tmp");
    if !tmp.is_file() {
        return false;
    }
    let recovered = match crate::audio::capture::recover_incomplete_capture_wav(&tmp) {
        Ok(Some(recovered)) => recovered,
        Ok(None) => return false,
        Err(err) => {
            tracing::warn!(error = %err, path = %tmp.display(), "failed to repair partial capture");
            return false;
        }
    };
    if let Err(rename_error) = std::fs::rename(&tmp, &dest) {
        if let Err(copy_error) = std::fs::copy(&tmp, &dest) {
            tracing::warn!(error = %copy_error, rename_error = %rename_error, path = %tmp.display(), "failed to salvage partial capture");
            return false;
        }
    }
    rec.duration_ms = recovered.duration_ms as i64;
    recovered.truncated
}

fn begin_transcription(ctx: &AppContext, recording_id: &str) -> Result<(), AppError> {
    if !ctx.in_flight.lock().insert(recording_id.to_string()) {
        return Err(AppError::TranscriptionInProgress);
    }
    Ok(())
}

fn fail_active_recording(ctx: &AppContext, err: &AppError) {
    let _session_lifecycle = ctx.session_lifecycle.lock();
    fail_active_recording_locked(ctx, err);
}

fn fail_active_recording_locked(ctx: &AppContext, err: &AppError) {
    let recording_id = ctx.session_recording_id.lock().clone();
    if let Some(id) = recording_id {
        if matches!(err, AppError::Cancelled) {
            discard_cancelled_recording(ctx, &id);
        } else {
            if let Err(mark_err) = mark_recording_failed(ctx, &id, err) {
                tracing::error!(error = %mark_err, recording_id = id, "persist cancelled-session failure");
            }
        }
        clear_live_session_locked(ctx, &id);
    }
}

fn persist_late_shutdown_capture(
    ctx: &AppContext,
    recording_id: &str,
    result: Result<crate::audio::capture::CaptureResult, AppError>,
) {
    match result {
        Ok(result) => persist_interrupted_capture(ctx, recording_id, &result),
        Err(err) => {
            tracing::warn!(error = %err, recording_id, "late capture failed during shutdown");
            if let Err(storage_err) =
                mark_recording_failed(ctx, recording_id, &AppError::Interrupted)
            {
                tracing::error!(error = %storage_err, recording_id, "persist interrupted recording after late shutdown capture failure");
            }
        }
    }
}

fn emit_stt_progress(
    ctx: &AppContext,
    app: &AppHandle,
    recording_id: &str,
    model: &str,
    progress: SttProgress,
) -> Result<(), AppError> {
    match &progress {
        SttProgress::Finished {
            attempt,
            outcome,
            http_status,
            latency_ms,
            category,
            cost_usd,
        } => {
            persist_attempt(
                ctx,
                recording_id,
                model,
                *attempt,
                outcome,
                *http_status,
                *latency_ms,
                category.clone(),
                *cost_usd,
            )?;
        }
        SttProgress::Attempt(_) | SttProgress::Waiting { .. } => {}
    }
    if ctx.session_recording_id.lock().as_deref() != Some(recording_id)
        && !ctx.in_flight.lock().contains(recording_id)
    {
        return Ok(());
    }
    let event = match progress {
        SttProgress::Attempt(attempt) => SessionEvent::TranscriptAttemptStarted { attempt },
        SttProgress::Waiting { attempt, delay } => SessionEvent::RetryScheduled { attempt, delay },
        SttProgress::Finished { .. } => return Ok(()),
    };
    if ctx.session_recording_id.lock().as_deref() == Some(recording_id)
        && ctx.transition(event).is_ok()
    {
        ctx.emit_state(app);
    }
    Ok(())
}

fn persist_attempt(
    ctx: &AppContext,
    recording_id: &str,
    model: &str,
    attempt: u32,
    outcome: &str,
    http_status: Option<u16>,
    latency_ms: u128,
    category: Option<String>,
    cost_usd: Option<f64>,
) -> Result<(), AppError> {
    let ended = chrono::Utc::now();
    let started = ended - chrono::Duration::milliseconds(latency_ms as i64);
    let row = crate::history::repository::TranscriptionAttempt {
        id: uuid::Uuid::new_v4().to_string(),
        recording_id: recording_id.into(),
        attempt_number: attempt as i64,
        started_at: started,
        ended_at: Some(ended),
        outcome: outcome.into(),
        error_category: category,
        http_status: http_status.map(|s| s as i64),
        latency_ms: Some(latency_ms as i64),
    };
    ctx.history
        .lock()
        .record_attempt_for_model(&row, cost_usd, model)
}

async fn run_transcription(
    app: &AppHandle,
    recording_id: &str,
    api_key: &str,
    model: &str,
    language: Option<&str>,
    path: &std::path::Path,
    policy: crate::transcription::retry::RetryPolicy,
    audio_duration: Duration,
    attempt_offset: u32,
    cancel: tokio::sync::watch::Receiver<bool>,
) -> Result<crate::transcription::openrouter::TranscriptionSuccess, AppError> {
    let client = {
        let ctx = app.state::<Arc<AppContext>>();
        ctx.clone_transport_client()?
    };
    let recording_id_owned = recording_id.to_string();
    let model_for_progress = model.to_string();
    let app_for_progress = app.clone();
    let persistence_error = Arc::new(Mutex::new(None));
    let persistence_error_for_callback = Arc::clone(&persistence_error);
    let result = transcribe_file_with_progress(
        &client,
        crate::transcription::openrouter::default_base_url(),
        api_key,
        model,
        language,
        path,
        policy,
        audio_duration,
        cancel,
        move |progress| {
            let ctx = app_for_progress.state::<Arc<AppContext>>();
            let progress = offset_attempt_number(progress, attempt_offset);
            if let Err(err) = emit_stt_progress(
                &ctx,
                &app_for_progress,
                &recording_id_owned,
                &model_for_progress,
                progress,
            ) {
                let mut stored = persistence_error_for_callback.lock();
                if stored.is_none() {
                    *stored = Some(err);
                }
            }
        },
    )
    .await;
    if let Some(err) = persistence_error.lock().take() {
        return Err(err);
    }
    result.map(|mut success| {
        success.attempt = success.attempt.saturating_add(attempt_offset);
        success
    })
}

fn offset_attempt_number(progress: SttProgress, offset: u32) -> SttProgress {
    match progress {
        SttProgress::Attempt(attempt) => SttProgress::Attempt(attempt.saturating_add(offset)),
        SttProgress::Waiting { attempt, delay } => SttProgress::Waiting {
            attempt: attempt.saturating_add(offset),
            delay,
        },
        SttProgress::Finished {
            attempt,
            outcome,
            http_status,
            latency_ms,
            category,
            cost_usd,
        } => SttProgress::Finished {
            attempt: attempt.saturating_add(offset),
            outcome,
            http_status,
            latency_ms,
            category,
            cost_usd,
        },
    }
}

fn mark_recording_failed(
    ctx: &AppContext,
    recording_id: &str,
    err: &AppError,
) -> Result<(), AppError> {
    let history = ctx.history.lock();
    let Some(mut rec) = history.get(recording_id)? else {
        return Ok(());
    };
    rec.status = if matches!(err, AppError::Interrupted) {
        RecordingStatus::Interrupted
    } else {
        RecordingStatus::Failed
    };
    rec.last_error_message = Some(err.user_message());
    rec.last_error_code = Some(err.code().to_string());
    rec.updated_at = chrono::Utc::now();
    let audio = audio_dir(&ctx.data_dir);
    if rec.raw_audio_path.as_ref().is_some_and(|name| {
        let raw = audio.join(name);
        !raw.is_file() && !raw.with_extension("wav.tmp").is_file()
    }) {
        rec.raw_audio_path = None;
    }
    if rec.processed_audio_path.as_ref().is_some_and(|name| {
        let processed = audio.join(name);
        !processed.is_file() && !processed.with_extension("wav.tmp").is_file()
    }) {
        rec.processed_audio_path = None;
    }
    history.update(&rec)?;
    Ok(())
}

pub fn apply_configured_retention(ctx: &AppContext) -> Result<Vec<String>, AppError> {
    let admission = crate::app::operations::lock_admission(ctx);
    apply_configured_retention_admitted(ctx, &admission)
}

fn apply_configured_retention_admitted(
    ctx: &AppContext,
    _admission: &parking_lot::MutexGuard<'_, ()>,
) -> Result<Vec<String>, AppError> {
    if *ctx.settings_recovery.lock() {
        return Ok(Vec::new());
    }
    let settings = ctx.settings.lock().clone();
    let protected = crate::app::operations::protected_recording_ids(ctx);
    let audio = audio_dir(&ctx.data_dir);
    let names = crate::app::operations::protected_audio_names(ctx);
    let history = ctx.history.lock();
    cleanup_orphans_except(&audio, &history, &names)?;
    crate::history::retention::apply_retention_with_result(
        &history,
        &audio,
        Retention::from_setting(&settings.retention),
        settings.storage_limit_bytes(),
        &protected,
        &names,
    )
    .map(|result| result.deleted)
}

pub fn start_retention_maintenance(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(10 * 60));
        loop {
            interval.tick().await;
            let ctx = app.state::<Arc<AppContext>>();
            retry_pending_cancelled_recordings(&ctx);
            let _admission = crate::app::operations::lock_admission(&ctx);
            if crate::app::operations::system_busy(&ctx) || *ctx.settings_recovery.lock() {
                continue;
            }
            if let Err(err) = apply_configured_retention_admitted(&ctx, &_admission) {
                tracing::warn!(error = %err, "periodic retention failed");
            }
        }
    });
}

pub fn current_meter(app: &AppHandle) -> MeterSample {
    let ctx = app.state::<Arc<AppContext>>();
    if let Some(sample) = ctx.capture.lock().as_ref().map(CaptureSession::meter) {
        return sample;
    }
    if let Some(sample) = ctx
        .preview_capture
        .lock()
        .as_ref()
        .map(CaptureSession::meter)
    {
        return sample;
    }
    if let Some(sample) = ctx
        .compare_capture
        .lock()
        .as_ref()
        .map(CaptureSession::meter)
    {
        return sample;
    }
    let sample = ctx
        .meter_monitor
        .lock()
        .as_ref()
        .map(CaptureSession::meter)
        .unwrap_or_else(MeterSample::silent);
    sample
}

pub fn release_meter_monitor(ctx: &AppContext) -> bool {
    ctx.meter_generation.fetch_add(1, Ordering::SeqCst);
    *ctx.meter_owner.lock() = None;
    let session = ctx.meter_monitor.lock().take();
    if let Some(session) = session {
        ctx.meter_starting.store(true, Ordering::SeqCst);
        stop_meter_capture(ctx, session).is_ok()
    } else {
        !ctx.meter_starting.load(Ordering::SeqCst)
    }
}

pub fn stop_meter_capture(ctx: &AppContext, session: CaptureSession) -> Result<(), AppError> {
    stop_meter_capture_with_timeout(ctx, session, crate::audio::capture::JOIN_BOUND)
}

fn stop_meter_capture_with_timeout(
    ctx: &AppContext,
    session: CaptureSession,
    join_bound: Duration,
) -> Result<(), AppError> {
    let cleanup = session.cleanup_handle();
    let late_cleanup = cleanup.clone();
    let late_reservation = Arc::clone(&ctx.meter_starting);
    let outcome = session.stop_with_late_result_timeout(join_bound, move |_| {
        late_cleanup.discard_late_result();
        late_reservation.store(false, Ordering::SeqCst);
    });
    match outcome {
        CaptureStopOutcome::Completed(result) => {
            cleanup.discard_late_result();
            ctx.meter_starting.store(false, Ordering::SeqCst);
            result.map(|_| ())
        }
        CaptureStopOutcome::TimedOut(err) => {
            cleanup.discard_late_result();
            Err(err)
        }
    }
}

pub fn reserve_meter_monitor(ctx: &AppContext, owner: &str) -> Option<u64> {
    let _admission = crate::app::operations::lock_admission(ctx);
    if crate::app::operations::system_busy(ctx) || ctx.meter_monitor.lock().is_some() {
        return None;
    }
    let generation = ctx.meter_generation.fetch_add(1, Ordering::SeqCst) + 1;
    *ctx.meter_owner.lock() = Some(owner.to_owned());
    ctx.meter_starting.store(true, Ordering::SeqCst);
    ctx.pending_native_capture_starts
        .fetch_add(1, Ordering::SeqCst);
    Some(generation)
}

pub fn take_owned_meter_monitor(ctx: &AppContext, owner: &str) -> Option<CaptureSession> {
    let _admission = crate::app::operations::lock_admission(ctx);
    if ctx.meter_owner.lock().as_deref() != Some(owner) {
        return None;
    }
    ctx.meter_generation.fetch_add(1, Ordering::SeqCst);
    *ctx.meter_owner.lock() = None;
    let session = ctx.meter_monitor.lock().take();
    if session.is_some() {
        ctx.meter_starting.store(true, Ordering::SeqCst);
    }
    session
}

#[cfg(test)]
fn finish_meter_start<T>(
    ctx: &AppContext,
    generation: u64,
    result: Result<T, AppError>,
    publish: impl FnOnce(T),
    discard: impl FnOnce(T) -> bool,
) -> Result<bool, AppError> {
    let result = finish_meter_start_with_pending(ctx, generation, result, false, publish, discard);
    release_pending_native_capture_start(ctx);
    result
}

fn finish_meter_start_with_pending<T>(
    ctx: &AppContext,
    generation: u64,
    result: Result<T, AppError>,
    native_worker_pending: bool,
    publish: impl FnOnce(T),
    discard: impl FnOnce(T) -> bool,
) -> Result<bool, AppError> {
    let admission = crate::app::operations::lock_admission(ctx);
    let current = ctx.meter_generation.load(Ordering::SeqCst) == generation;
    match result {
        Ok(session) if current => {
            publish(session);
            ctx.meter_starting.store(false, Ordering::SeqCst);
            Ok(true)
        }
        other => {
            drop(admission);
            let (error, discarded) = match other {
                Ok(session) => (None, discard(session)),
                Err(error) => (Some(error), !native_worker_pending),
            };
            let _admission = crate::app::operations::lock_admission(ctx);
            // No newer reservation is admitted until the stale capture has closed.
            if discarded {
                ctx.meter_starting.store(false, Ordering::SeqCst);
            }
            if current {
                *ctx.meter_owner.lock() = None;
            }
            match error {
                Some(error) if current => Err(error),
                _ => Ok(false),
            }
        }
    }
}

pub fn start_meter_monitor(
    ctx: &AppContext,
    generation: u64,
    app: &AppHandle,
) -> Result<bool, AppError> {
    let settings = ctx.settings.lock().clone();
    let device = if settings.input_device == "default" {
        None
    } else {
        Some(settings.input_device.clone())
    };
    let late_ctx = app.state::<Arc<AppContext>>().inner().clone();
    let late_app = app.clone();
    let started =
        CaptureSession::start_monitor_with_late_completion(device.as_deref(), move || {
            let _admission = crate::app::operations::lock_admission(&late_ctx);
            if late_ctx.meter_generation.load(Ordering::SeqCst) == generation {
                *late_ctx.meter_owner.lock() = None;
            }
            late_ctx.meter_starting.store(false, Ordering::SeqCst);
            release_pending_native_capture_start(&late_ctx);
            drop(_admission);
            schedule_deferred_shutdown_exit(&late_app, &late_ctx);
        });
    let native_worker_pending = started
        .as_ref()
        .err()
        .is_some_and(|failure| failure.native_worker_pending);
    let started = started.map_err(|failure| failure.error);
    let result = finish_meter_start_with_pending(
        ctx,
        generation,
        started,
        native_worker_pending,
        |session| *ctx.meter_monitor.lock() = Some(session),
        |session| match stop_meter_capture_with_timeout(
            ctx,
            session,
            crate::audio::capture::JOIN_BOUND,
        ) {
            Ok(()) => true,
            Err(err) => {
                tracing::warn!(error = %err, "stale meter capture remains detached; its callback owns final cleanup");
                false
            }
        },
    );
    if !native_worker_pending {
        release_pending_native_capture_start(ctx);
        schedule_deferred_shutdown_exit(app, ctx);
    }
    result
}

pub fn devices() -> Result<Vec<crate::audio::devices::InputDeviceInfo>, AppError> {
    list_input_devices().map_err(|_| AppError::MicrophoneUnavailable)
}

pub async fn manual_retry(
    app: AppHandle,
    recording_id: String,
) -> Result<crate::history::repository::Recording, AppError> {
    manual_history_transcription(app, recording_id, false).await
}

pub async fn manual_reprocess(
    app: AppHandle,
    recording_id: String,
) -> Result<crate::history::repository::Recording, AppError> {
    manual_history_transcription(app, recording_id, true).await
}

async fn manual_history_transcription(
    app: AppHandle,
    recording_id: String,
    reprocess_original: bool,
) -> Result<crate::history::repository::Recording, AppError> {
    let ctx = app.state::<Arc<AppContext>>();
    {
        let _admission = crate::app::operations::lock_admission(&ctx);
        if !ctx.settings.lock().first_run_complete {
            return Err(AppError::DisclosureRequired);
        }
        if crate::app::operations::system_busy(&ctx) {
            return Err(AppError::TranscriptionInProgress);
        }
        begin_transcription(&ctx, &recording_id)?;
    }
    let result = manual_retry_inner(&app, &recording_id, reprocess_original).await;
    ctx.retry_cancel.lock().remove(&recording_id);
    ctx.in_flight.lock().remove(&recording_id);
    if let Err(err) = &result {
        crate::notify::show_error(&app, err);
    }
    result
}

async fn manual_retry_inner(
    app: &AppHandle,
    recording_id: &str,
    reprocess_original: bool,
) -> Result<crate::history::repository::Recording, AppError> {
    let ctx = app.state::<Arc<AppContext>>();
    history_retry_allowed(&ctx.state.lock())?;
    let (tx, rx) = tokio::sync::watch::channel(false);
    ctx.retry_cancel.lock().insert(recording_id.to_string(), tx);
    let settings = ctx.settings.lock().clone();
    let mut rec = ctx
        .history
        .lock()
        .get(recording_id)?
        .ok_or_else(|| AppError::StorageFailed("recording missing".into()))?;
    let original_recording = rec.clone();
    let audio_root = audio_dir(&ctx.data_dir);
    if !can_retry_from_history(&rec) {
        return Err(AppError::StorageFailed("audio missing".into()));
    }
    let mut staged_processed = None;
    let processed_path = if reprocess_original {
        let raw_path = rec
            .raw_audio_path
            .as_ref()
            .map(|name| audio_root.join(name))
            .filter(|path| path.is_file())
            .ok_or_else(|| AppError::StorageFailed("original audio missing".into()))?;
        let id = uuid::Uuid::parse_str(recording_id)
            .map_err(|_| AppError::StorageFailed("invalid recording id".into()))?;
        let processed_name = format!("{id}.processed-{}.wav", uuid::Uuid::new_v4());
        let processed_path = audio_root.join(&processed_name);
        let preset = settings.active_preset();
        let source_for_dsp = raw_path.clone();
        let dest_for_dsp = processed_path.clone();
        let dsp_result = tokio::task::spawn_blocking(move || {
            reprocess_original_audio(&source_for_dsp, &dest_for_dsp, preset)
        })
        .await
        .map_err(|err| AppError::AudioProcessingFailed(err.to_string()))?;
        if let Err(err) = dsp_result {
            remove_capture_artifacts(&processed_path);
            return Err(err);
        }
        staged_processed = Some(processed_path.clone());
        rec.processed_audio_path = Some(processed_name);
        processed_path
    } else {
        let processed = rec
            .processed_audio_path
            .as_ref()
            .map(|name| audio_root.join(name))
            .filter(|path| path.is_file());
        let raw = rec
            .raw_audio_path
            .as_ref()
            .map(|name| audio_root.join(name))
            .filter(|path| path.is_file());
        processed
            .or(raw)
            .ok_or_else(|| AppError::StorageFailed("audio missing".into()))?
    };
    if let Err(err) = validate_retry_audio_path(&processed_path) {
        if let Some(path) = staged_processed.as_deref() {
            remove_capture_artifacts(path);
        }
        return Err(err);
    }
    if *rx.borrow() {
        if let Some(path) = staged_processed.as_deref() {
            remove_capture_artifacts(path);
        }
        return Err(AppError::Cancelled);
    }
    let key = match get_api_key().and_then(|key| key.ok_or(AppError::InvalidApiKey)) {
        Ok(key) => key,
        Err(err) => {
            if let Some(path) = staged_processed.as_deref() {
                remove_capture_artifacts(path);
            }
            return Err(err);
        }
    };
    if *rx.borrow() {
        if let Some(path) = staged_processed.as_deref() {
            remove_capture_artifacts(path);
        }
        return Err(AppError::Cancelled);
    }
    let attempt_offset = u32::try_from(rec.attempt_count.max(0)).unwrap_or(u32::MAX);
    let language = if settings.language == "auto" {
        None
    } else {
        Some(settings.language.as_str())
    };
    let stt_source = processed_path.clone();
    let stt_result = tokio::task::spawn_blocking(move || write_stt_upload(&stt_source))
        .await
        .map_err(|err| AppError::AudioProcessingFailed(err.to_string()));
    let stt_result = match stt_result {
        Ok(result) => result,
        Err(err) => {
            if let Some(path) = staged_processed.as_deref() {
                remove_capture_artifacts(path);
            }
            return Err(err);
        }
    };
    let stt_path = match stt_result {
        Ok(path) => path,
        Err(err) => {
            if let Some(path) = staged_processed.as_deref() {
                remove_capture_artifacts(path);
            }
            return Err(err);
        }
    };
    if *rx.borrow() {
        remove_capture_artifacts(&stt_path);
        if let Some(path) = staged_processed.as_deref() {
            remove_capture_artifacts(path);
        }
        return Err(AppError::Cancelled);
    }
    rec.model = settings.model.clone();
    if let Err(err) = persist_retry_processing(ctx.inner(), &mut rec) {
        remove_capture_artifacts(&stt_path);
        if let Some(path) = staged_processed.as_deref() {
            remove_capture_artifacts(path);
        }
        return Err(err);
    }
    ctx.emit_history(app);
    dismiss_overlay_if_session_idle(app);
    let transcribed = run_transcription(
        app,
        recording_id,
        &key,
        &settings.model,
        language,
        &stt_path,
        settings.retry.to_policy(),
        Duration::from_millis(rec.duration_ms as u64),
        attempt_offset,
        rx.clone(),
    )
    .await;
    let _ = std::fs::remove_file(&stt_path);
    let transcribed = if *rx.borrow() {
        Err(AppError::Cancelled)
    } else {
        transcribed
    };
    match transcribed {
        Ok(success) => {
            let transcript = crate::text_replacements::apply_text_replacements(
                success.text,
                &settings.text_replacements,
            );
            let cancel_gate = ctx.retry_cancel.lock();
            if *rx.borrow() {
                drop(cancel_gate);
                restore_cancelled_retry(
                    ctx.inner(),
                    recording_id,
                    &original_recording,
                    staged_processed.as_deref(),
                )?;
                ctx.emit_history(app);
                release_retry_session(ctx.inner(), app, recording_id, Some(AppError::Cancelled));
                return Err(AppError::Cancelled);
            }
            rec.status = RecordingStatus::Completed;
            rec.model = settings.model.clone();
            rec.transcript = Some(transcript);
            rec.attempt_count = rec.attempt_count.max(success.attempt as i64);
            rec.usage_json = success.usage_json;
            rec.cost = success.cost;
            rec.generation_id = success.generation_id;
            rec.latency_ms = Some(success.latency_ms as i64);
            rec.completed_at = Some(chrono::Utc::now());
            if !preserve_capture_truncation_notice(
                rec.last_error_code.as_deref(),
                rec.last_error_message.as_deref(),
            ) {
                rec.last_error_code = None;
                rec.last_error_message = None;
            }
            rec.updated_at = chrono::Utc::now();
            ctx.history.lock().update(&rec)?;
            drop(cancel_gate);
            ctx.emit_history(app);
            release_retry_session(ctx.inner(), app, recording_id, None);
            Ok(rec)
        }
        Err(AppError::Cancelled) => {
            restore_cancelled_retry(
                ctx.inner(),
                recording_id,
                &original_recording,
                staged_processed.as_deref(),
            )?;
            ctx.emit_history(app);
            release_retry_session(ctx.inner(), app, recording_id, Some(AppError::Cancelled));
            Err(AppError::Cancelled)
        }
        Err(err) => {
            if let Some(persisted) = ctx.history.lock().get(recording_id)? {
                rec.attempt_count = rec.attempt_count.max(persisted.attempt_count);
            }
            rec.status = RecordingStatus::Failed;
            rec.last_error_message = Some(err.user_message());
            rec.last_error_code = Some(err.code().to_string());
            rec.updated_at = chrono::Utc::now();
            ctx.history.lock().update(&rec)?;
            ctx.emit_history(app);
            release_retry_session(ctx.inner(), app, recording_id, Some(err.clone()));
            Err(err)
        }
    }
}

fn preserve_capture_truncation_notice(code: Option<&str>, message: Option<&str>) -> bool {
    if code == Some(AppError::RecordingTruncated.code()) {
        return true;
    }
    if code != Some(AppError::RecordingTooLarge.code()) {
        return false;
    }
    let message = message.unwrap_or_default().to_ascii_lowercase();
    message.contains("truncated")
        || message.contains("maximum size")
        || message.contains("maximum recording size")
}

fn persist_retry_processing(
    ctx: &AppContext,
    recording: &mut crate::history::repository::Recording,
) -> Result<(), AppError> {
    recording.status = RecordingStatus::Processing;
    recording.updated_at = chrono::Utc::now();
    if ctx.history.lock().update(recording)? {
        Ok(())
    } else {
        Err(AppError::StorageFailed(
            "recording was removed before retry started".into(),
        ))
    }
}

fn restore_cancelled_retry(
    ctx: &AppContext,
    recording_id: &str,
    original: &crate::history::repository::Recording,
    staged_processed: Option<&Path>,
) -> Result<(), AppError> {
    let history = ctx.history.lock();
    let Some(mut rec) = history.get(recording_id)? else {
        return Ok(());
    };
    rec.status = original.status.clone();
    rec.model = original.model.clone();
    rec.processed_audio_path = original.processed_audio_path.clone();
    rec.updated_at = chrono::Utc::now();
    history.update(&rec)?;
    drop(history);
    if let Some(path) = staged_processed {
        remove_capture_artifacts(path);
    }
    Ok(())
}

fn reprocess_original_audio(
    raw_path: &Path,
    processed_path: &Path,
    preset: crate::dsp::pipeline::DspPreset,
) -> Result<(), AppError> {
    let (samples, rate) = read_pcm16_wav_with_rate(raw_path)?;
    if samples.is_empty() {
        return Err(AppError::AudioProcessingFailed(
            "capture contains no audio samples".into(),
        ));
    }
    let samples = if rate == SAMPLE_RATE {
        samples
    } else {
        crate::audio::resample::resample_sinc(&samples, rate, SAMPLE_RATE)
    };
    let processed = prepare_listen_audio(preset, samples)?;
    write_pcm16_wav(processed_path, SAMPLE_RATE, &processed)
}

fn validate_retry_audio_path(path: &Path) -> Result<(), AppError> {
    if crate::audio::capture::capture_wav_duration_ms(path)? == 0 {
        return Err(AppError::AudioProcessingFailed(
            "capture contains no audio samples".into(),
        ));
    }
    Ok(())
}

#[cfg(test)]
fn history_retry_claims_live_hud() -> bool {
    false
}

fn history_retry_allowed(state: &SessionState) -> Result<(), AppError> {
    match state {
        SessionState::Idle | SessionState::Failed { .. } | SessionState::Completed => Ok(()),
        _ => Err(AppError::TranscriptionInProgress),
    }
}

fn dismiss_overlay_if_session_idle(app: &AppHandle) {
    let ctx = app.state::<Arc<AppContext>>();
    if is_cancellable(&ctx.state.lock()) {
        return;
    }
    hide_overlay_now(app);
}

fn release_retry_session(
    ctx: &AppContext,
    app: &AppHandle,
    recording_id: &str,
    error: Option<AppError>,
) {
    let claimed = ctx.session_recording_id.lock().as_deref() == Some(recording_id);
    if claimed {
        *ctx.session_recording_id.lock() = None;
        match error {
            None => {
                let _ = ctx.transition(SessionEvent::Succeeded);
                let _ = ctx.transition(SessionEvent::Dismiss);
            }
            Some(AppError::Cancelled) => {
                let _ = ctx.transition(SessionEvent::Cancelled);
            }
            Some(err) => {
                let _ = ctx.transition(SessionEvent::Failed(err));
                let _ = ctx.transition(SessionEvent::Dismiss);
            }
        }
    }
    if !is_cancellable(&ctx.state.lock()) {
        hide_overlay_now(app);
    }
    ctx.emit_state(app);
}

pub fn stop_pipeline_allowed(lease_ok: bool, has_recording_id: bool) -> bool {
    lease_ok && has_recording_id
}

pub fn finish_stop_may_continue(state: &SessionState, has_id: bool) -> bool {
    has_id
        && matches!(
            state,
            SessionState::StoppingRecording | SessionState::Saving
        )
}

fn apply_raw_retention(
    rec: &mut crate::history::repository::Recording,
    keep_original: bool,
    raw_path: &Path,
) {
    if !keep_original {
        match std::fs::remove_file(raw_path) {
            Ok(()) => rec.raw_audio_path = None,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => rec.raw_audio_path = None,
            Err(err) => {
                tracing::warn!(error = %err, "preserve raw audio reference after cleanup failure")
            }
        }
    }
}

fn persist_live_recording(
    ctx: &AppContext,
    generation: u64,
    cancelled: &tokio::sync::watch::Receiver<bool>,
    rec: &crate::history::repository::Recording,
) -> Result<(), AppError> {
    let _lifecycle = ctx.session_lifecycle.lock();
    if !may_commit_session(
        generation,
        ctx.session_generation.load(Ordering::SeqCst),
        *cancelled.borrow(),
    ) {
        return Err(AppError::Cancelled);
    }
    if !ctx.history.lock().update(rec)? {
        return Err(AppError::StorageFailed("recording missing".into()));
    }
    Ok(())
}

fn persist_processed_audio(
    history: &HistoryRepo,
    rec: &mut crate::history::repository::Recording,
    keep_original: bool,
    raw_path: &Path,
) -> Result<(), AppError> {
    // The processed file must have a durable reference before deleting the raw
    // capture. A failed second update leaves a valid processed reference in SQLite.
    if !history.update(rec)? {
        return Err(AppError::StorageFailed("recording missing".into()));
    }
    let previous_raw = rec.raw_audio_path.clone();
    apply_raw_retention(rec, keep_original, raw_path);
    if rec.raw_audio_path != previous_raw && !history.update(rec)? {
        return Err(AppError::StorageFailed("recording missing".into()));
    }
    Ok(())
}

fn persist_capture_result(
    history: &HistoryRepo,
    recording_id: &str,
    result: &crate::audio::capture::CaptureResult,
) -> Result<(crate::history::repository::Recording, Option<AppError>), AppError> {
    let Some(mut rec) = history.get(recording_id)? else {
        remove_capture_artifacts(&result.path);
        return Err(AppError::StorageFailed("recording missing".into()));
    };
    let file_name = result
        .path
        .file_name()
        .ok_or_else(|| AppError::StorageFailed("capture output path is invalid".into()))?
        .to_string_lossy()
        .into_owned();
    let failure = apply_capture_result_metadata(&mut rec, result, file_name);
    // A lookup/write failure preserves the WAV. Its raw reference was committed
    // before capture began, so restart recovery and orphan cleanup can find it.
    if !history.update(&rec)? {
        return Err(AppError::StorageFailed("recording missing".into()));
    }
    Ok((rec, failure))
}

fn apply_capture_result_metadata(
    recording: &mut crate::history::repository::Recording,
    result: &crate::audio::capture::CaptureResult,
    file_name: String,
) -> Option<AppError> {
    recording.duration_ms = result.duration_ms as i64;
    recording.raw_audio_path = Some(file_name);
    recording.updated_at = chrono::Utc::now();
    if result.truncated {
        recording.last_error_code = Some(AppError::RecordingTruncated.code().into());
        recording.last_error_message = Some("Recording truncated".into());
    }
    let failure = result
        .capture_error
        .as_ref()
        .map(|message| AppError::AudioCaptureFailed(message.clone()));
    if let Some(err) = &failure {
        recording.status = RecordingStatus::Failed;
        recording.last_error_code = Some(err.code().into());
        recording.last_error_message = Some(err.user_message());
    }
    failure
}

fn read_capture_samples(inline_samples: Vec<f32>, wav_path: &Path) -> Result<Vec<f32>, AppError> {
    let samples = if inline_samples.is_empty() {
        match read_pcm16_wav_with_rate(wav_path) {
            Ok((samples, _)) => samples,
            Err(err) => return Err(err),
        }
    } else {
        inline_samples
    };
    if samples.is_empty() {
        Err(AppError::AudioProcessingFailed(
            "capture contains no audio samples".into(),
        ))
    } else {
        Ok(samples)
    }
}

fn persist_interrupted_capture(
    ctx: &AppContext,
    recording_id: &str,
    result: &crate::audio::capture::CaptureResult,
) {
    let history = ctx.history.lock();
    let Ok(Some(mut rec)) = history.get(recording_id) else {
        return;
    };
    let capture_error = result
        .capture_error
        .as_ref()
        .map(|message| AppError::AudioCaptureFailed(message.clone()));
    rec.status = if capture_error.is_some() {
        RecordingStatus::Failed
    } else {
        RecordingStatus::Interrupted
    };
    rec.duration_ms = result.duration_ms as i64;
    rec.raw_audio_path = result
        .path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned());
    rec.last_error_code = Some(
        if let Some(err) = capture_error.as_ref() {
            err.code()
        } else if result.truncated {
            AppError::RecordingTruncated.code()
        } else {
            AppError::Interrupted.code()
        }
        .into(),
    );
    rec.last_error_message = Some(if let Some(err) = capture_error.as_ref() {
        err.user_message()
    } else if result.truncated {
        "Recording reached the maximum size before shutdown".into()
    } else {
        AppError::Interrupted.user_message()
    });
    rec.updated_at = chrono::Utc::now();
    if let Err(err) = history.update(&rec) {
        tracing::error!(error = %err, recording_id, "persist interrupted capture result");
    }
}

fn prepare_processed_audio(
    preset: crate::dsp::pipeline::DspPreset,
    samples: Vec<f32>,
    processed_path: PathBuf,
) -> Result<Vec<f32>, AppError> {
    let dsp_started = Instant::now();
    let processed = prepare_listen_audio(preset, samples)?;
    let dsp_ms = dsp_started.elapsed().as_millis() as u64;
    let processed_write_started = Instant::now();
    write_pcm16_wav(&processed_path, SAMPLE_RATE, &processed)?;
    let processed_write_ms = processed_write_started.elapsed().as_millis() as u64;
    tracing::info!(
        raw_read_ms = 0u64,
        dsp_ms,
        metrics_ms = 0u64,
        processed_write_ms,
        "dictation dsp"
    );
    Ok(processed)
}

fn write_stt_pcm(processed: &[f32], processed_path: &Path) -> Result<PathBuf, AppError> {
    let downsample_started = Instant::now();
    let stt = samples_for_stt(processed, SAMPLE_RATE);
    let downsample_ms = downsample_started.elapsed().as_millis() as u64;
    let stt_path = processed_path.with_extension("stt.wav");
    let stt_write_started = Instant::now();
    write_pcm16_wav(&stt_path, STT_SAMPLE_RATE, &stt)?;
    let stt_write_ms = stt_write_started.elapsed().as_millis() as u64;
    tracing::info!(downsample_ms, stt_write_ms, "dictation stt wav");
    Ok(stt_path)
}

fn prepare_local_stt(
    preset: crate::dsp::pipeline::DspPreset,
    samples: Vec<f32>,
    processed_path: PathBuf,
) -> Result<PathBuf, AppError> {
    let processed = prepare_processed_audio(preset, samples, processed_path.clone())?;
    write_stt_pcm(&processed, &processed_path)
}

fn write_stt_upload(listen_path: &Path) -> Result<PathBuf, AppError> {
    let (samples, rate) = read_pcm16_wav_with_rate(listen_path)?;
    let stt = samples_for_stt(&samples, rate);
    let dest = listen_path.with_extension("stt.wav");
    write_pcm16_wav(&dest, STT_SAMPLE_RATE, &stt)?;
    Ok(dest)
}

#[cfg(test)]
mod tests {
    use rusqlite::Connection;

    #[test]
    fn meter_stop_timeout_keeps_system_busy_until_native_capture_exits() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = Arc::new(AppContext::initialize(dir.path().to_path_buf()).unwrap());
        ctx.meter_starting.store(true, Ordering::SeqCst);
        let path = dir.path().join("late-meter.wav");
        let (release, wait) = std::sync::mpsc::channel();
        let worker_path = path.clone();
        let worker = thread::spawn(move || {
            wait.recv().unwrap();
            std::fs::write(&worker_path, b"late meter sample").unwrap();
            Ok(crate::audio::capture::CaptureResult {
                path: worker_path,
                duration_ms: 100,
                sample_rate: SAMPLE_RATE,
                samples: vec![0.1; 8],
                truncated: false,
                capture_error: None,
            })
        });
        let session = CaptureSession::from_test_worker(worker, path.clone());

        let outcome = stop_meter_capture_with_timeout(&ctx, session, Duration::from_millis(20));
        assert!(matches!(outcome, Err(AppError::AudioCaptureFailed(_))));
        assert!(ctx.meter_starting.load(Ordering::SeqCst));
        assert!(crate::app::operations::system_busy(&ctx));

        release.send(()).unwrap();
        let deadline = std::time::Instant::now() + Duration::from_secs(2);
        while ctx.meter_starting.load(Ordering::SeqCst) && std::time::Instant::now() < deadline {
            thread::sleep(Duration::from_millis(5));
        }
        assert!(!ctx.meter_starting.load(Ordering::SeqCst));
        assert!(!crate::app::operations::system_busy(&ctx));
        assert!(!path.exists());
    }

    #[test]
    fn cancelled_meter_start_timeout_keeps_reservation_until_native_capture_exits() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = Arc::new(AppContext::initialize(dir.path().to_path_buf()).unwrap());
        let generation = reserve_meter_monitor(&ctx, "cancelled").unwrap();
        assert!(take_owned_meter_monitor(&ctx, "cancelled").is_none());
        let path = dir.path().join("late-started-meter.wav");
        let (release, wait) = std::sync::mpsc::channel();
        let worker_path = path.clone();
        let worker = thread::spawn(move || {
            wait.recv().unwrap();
            std::fs::write(&worker_path, b"late meter start").unwrap();
            Ok(crate::audio::capture::CaptureResult {
                path: worker_path,
                duration_ms: 100,
                sample_rate: SAMPLE_RATE,
                samples: vec![0.1; 8],
                truncated: false,
                capture_error: None,
            })
        });
        let session = CaptureSession::from_test_worker(worker, path.clone());

        assert!(!finish_meter_start(
            &ctx,
            generation,
            Ok(session),
            |_| panic!("cancelled meter capture must not publish"),
            |session| stop_meter_capture_with_timeout(&ctx, session, Duration::from_millis(20),)
                .is_ok()
        )
        .unwrap());
        assert!(ctx.meter_starting.load(Ordering::SeqCst));
        assert!(crate::app::operations::system_busy(&ctx));
        assert!(reserve_meter_monitor(&ctx, "replacement").is_none());

        release.send(()).unwrap();
        let deadline = std::time::Instant::now() + Duration::from_secs(2);
        while ctx.meter_starting.load(Ordering::SeqCst) && std::time::Instant::now() < deadline {
            thread::sleep(Duration::from_millis(5));
        }
        assert!(!ctx.meter_starting.load(Ordering::SeqCst));
        assert!(!crate::app::operations::system_busy(&ctx));
        assert!(!path.exists());
    }

    #[test]
    fn cancelled_pending_capture_start_stays_busy_until_late_native_result_is_discarded() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = Arc::new(AppContext::initialize(dir.path().to_path_buf()).unwrap());
        let mut rec = new_recording("openai/gpt-transcribe".into());
        rec.status = RecordingStatus::Processing;
        rec.raw_audio_path = Some(format!("{}.raw.wav", rec.id));
        ctx.history.lock().insert(&rec).unwrap();
        *ctx.session_recording_id.lock() = Some(rec.id.clone());
        ctx.capture_starting.store(true, Ordering::SeqCst);
        discard_pending_capture_recording_row(&ctx, &rec.id);
        let tombstone = ctx.history.lock().get(&rec.id).unwrap().unwrap();
        assert_eq!(tombstone.last_error_code.as_deref(), Some("Cancelled"));
        assert_eq!(tombstone.transcript, None);

        let path = audio_dir(&ctx.data_dir).join(format!("{}.raw.wav", rec.id));
        let (release, wait) = std::sync::mpsc::channel();
        let worker_path = path.clone();
        let worker = thread::spawn(move || {
            wait.recv().unwrap();
            std::fs::write(&worker_path, b"late cancelled capture").unwrap();
            Ok(crate::audio::capture::CaptureResult {
                path: worker_path,
                duration_ms: 1234,
                sample_rate: SAMPLE_RATE,
                samples: vec![0.1; 8],
                truncated: false,
                capture_error: None,
            })
        });
        let session = CaptureSession::from_test_worker(worker, path.clone());
        stop_stale_capture_start_with_timeout(
            Arc::clone(&ctx),
            None,
            session,
            rec.id.clone(),
            Duration::from_millis(20),
        );
        assert!(ctx.capture_starting.load(Ordering::SeqCst));
        assert!(crate::app::operations::system_busy(&ctx));
        assert!(ctx.history.lock().get(&rec.id).unwrap().is_some());

        release.send(()).unwrap();
        let deadline = std::time::Instant::now() + Duration::from_secs(2);
        while ctx.capture_starting.load(Ordering::SeqCst) && std::time::Instant::now() < deadline {
            thread::sleep(Duration::from_millis(5));
        }
        assert!(!ctx.capture_starting.load(Ordering::SeqCst));
        assert!(!crate::app::operations::system_busy(&ctx));
        assert!(!path.exists());
    }

    #[test]
    fn pending_start_cancel_hides_history_and_waits_for_late_audio_cleanup() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = Arc::new(AppContext::initialize(dir.path().to_path_buf()).unwrap());
        let mut rec = new_recording("openai/gpt-transcribe".into());
        rec.status = RecordingStatus::Processing;
        rec.raw_audio_path = Some(format!("{}.raw.wav", rec.id));
        ctx.history.lock().insert(&rec).unwrap();
        *ctx.session_recording_id.lock() = Some(rec.id.clone());
        ctx.capture_starting.store(true, Ordering::SeqCst);

        let late_path = audio_dir(&ctx.data_dir).join(format!("{}.raw.wav", rec.id));
        std::fs::write(&late_path, b"late native capture output").unwrap();
        discard_pending_capture_recording_locked(&ctx, &rec.id);

        let tombstone = ctx.history.lock().get(&rec.id).unwrap().unwrap();
        assert_eq!(tombstone.last_error_code.as_deref(), Some("Cancelled"));
        assert_eq!(tombstone.transcript, None);
        assert_eq!(tombstone.raw_audio_path, None);
        assert_eq!(ctx.history.lock().count(None).unwrap(), 0);
        assert_eq!(
            ctx.session_recording_id.lock().as_deref(),
            Some(rec.id.as_str())
        );
        assert!(late_path.exists());

        retry_cancelled_recording_deletion(&ctx, &rec.id);
        let deadline = Instant::now() + Duration::from_secs(2);
        while ctx.history.lock().get(&rec.id).unwrap().is_some() && Instant::now() < deadline {
            thread::sleep(Duration::from_millis(5));
        }
        assert!(!late_path.exists());
        assert!(ctx.history.lock().get(&rec.id).unwrap().is_none());
    }

    #[test]
    fn cancelled_capture_releases_session_when_tombstone_delete_keeps_failing() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = Arc::new(AppContext::initialize(dir.path().to_path_buf()).unwrap());
        let mut rec = new_recording("openai/gpt-transcribe".into());
        rec.status = RecordingStatus::Processing;
        rec.raw_audio_path = Some(format!("{}.raw.wav", rec.id));
        ctx.history.lock().insert(&rec).unwrap();
        ctx.history
            .lock()
            .suppress_cancelled_recording(&rec.id)
            .unwrap();
        let raw_audio = audio_dir(&ctx.data_dir).join(format!("{}.raw.wav", rec.id));
        std::fs::write(&raw_audio, b"cancelled capture").unwrap();
        Connection::open(ctx.data_dir.join("history.sqlite"))
            .unwrap()
            .execute_batch(
                "CREATE TRIGGER reject_cancelled_delete BEFORE DELETE ON recordings \
                 BEGIN SELECT RAISE(ABORT, 'injected cancelled delete failure'); END;",
            )
            .unwrap();
        *ctx.session_recording_id.lock() = Some(rec.id.clone());
        ctx.capture_stopping.store(true, Ordering::SeqCst);

        finish_cancelled_capture_stop(
            Arc::clone(&ctx),
            None,
            Some(rec.id.clone()),
            Err(AppError::Cancelled),
        );

        assert!(!ctx.capture_stopping.load(Ordering::SeqCst));
        assert!(ctx.session_recording_id.lock().is_none());
        assert!(!crate::app::operations::system_busy(&ctx));
        assert_eq!(ctx.history.lock().count(None).unwrap(), 0);
        assert!(ctx.history.lock().get(&rec.id).unwrap().is_some());
        assert!(!raw_audio.exists());

        Connection::open(ctx.data_dir.join("history.sqlite"))
            .unwrap()
            .execute_batch("DROP TRIGGER reject_cancelled_delete;")
            .unwrap();
        let deadline = Instant::now() + Duration::from_secs(3);
        while Instant::now() < deadline {
            let row_exists = ctx.history.lock().get(&rec.id).unwrap().is_some();
            let cleanup_pending = ctx.cancelled_deletion_retries.lock().contains(&rec.id);
            if !row_exists && !cleanup_pending {
                break;
            }
            thread::sleep(Duration::from_millis(10));
        }
        assert!(ctx.history.lock().get(&rec.id).unwrap().is_none());
        assert!(!ctx.cancelled_deletion_retries.lock().contains(&rec.id));
    }

    #[test]
    fn shutdown_waits_for_auxiliary_capture_reservations() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = AppContext::initialize(dir.path().to_path_buf()).unwrap();
        assert!(!shutdown_capture_cleanup_pending(&ctx));

        ctx.filter_recording.store(true, Ordering::SeqCst);
        assert!(shutdown_capture_cleanup_pending(&ctx));
        ctx.filter_recording.store(false, Ordering::SeqCst);

        ctx.compare_state.lock().recording = true;
        assert!(shutdown_capture_cleanup_pending(&ctx));
        ctx.compare_state.lock().recording = false;

        ctx.compare_running.store(true, Ordering::SeqCst);
        assert!(shutdown_capture_cleanup_pending(&ctx));
        ctx.compare_running.store(false, Ordering::SeqCst);

        ctx.meter_starting.store(true, Ordering::SeqCst);
        assert!(shutdown_capture_cleanup_pending(&ctx));
        ctx.meter_starting.store(false, Ordering::SeqCst);
        assert!(!shutdown_capture_cleanup_pending(&ctx));
    }

    #[test]
    fn shutdown_cancels_compare_requests_even_after_capture_stopped() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = AppContext::initialize(dir.path().to_path_buf()).unwrap();
        let (tx, rx) = tokio::sync::watch::channel(false);
        *ctx.compare_cancel.lock() = Some(tx);
        ctx.compare_running.store(true, Ordering::SeqCst);

        request_compare_cancel_for_shutdown(&ctx);

        assert!(*rx.borrow());
        assert!(shutdown_capture_cleanup_pending(&ctx));
    }

    #[test]
    fn forced_shutdown_exit_is_allowed_when_cleanup_stays_pending() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = AppContext::initialize(dir.path().to_path_buf()).unwrap();
        ctx.compare_running.store(true, Ordering::SeqCst);
        assert!(shutdown_capture_cleanup_pending(&ctx));

        ctx.shutdown_force_exit.store(true, Ordering::SeqCst);

        assert!(shutdown_session_exit_ready(&ctx));
    }

    #[test]
    fn shutdown_exit_reservation_refuses_contention_and_requires_completed_cleanup() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = Arc::new(AppContext::initialize(dir.path().to_path_buf()).unwrap());
        let held = ctx.session_lifecycle.lock();
        let worker_ctx = Arc::clone(&ctx);
        let (done_tx, done_rx) = std::sync::mpsc::channel();
        let worker =
            thread::spawn(move || done_tx.send(reserve_shutdown_exit(&worker_ctx)).unwrap());
        let result = done_rx.recv_timeout(Duration::from_secs(2));
        drop(held);
        worker.join().unwrap();
        assert_eq!(
            result.expect("exit reservation waited for lifecycle"),
            ShutdownExitReservation::Wait
        );
        ctx.shutdown_requested.store(true, Ordering::SeqCst);
        assert_eq!(reserve_shutdown_exit(&ctx), ShutdownExitReservation::Wait);
        ctx.shutdown_cleanup_started.store(true, Ordering::SeqCst);
        ctx.capture_stopping.store(true, Ordering::SeqCst);
        assert_eq!(reserve_shutdown_exit(&ctx), ShutdownExitReservation::Wait);
        assert!(!ctx.shutdown_exit_scheduled.load(Ordering::SeqCst));
        ctx.capture_stopping.store(false, Ordering::SeqCst);
        assert_eq!(
            reserve_shutdown_exit(&ctx),
            ShutdownExitReservation::Reserved
        );
        assert_eq!(
            reserve_shutdown_exit(&ctx),
            ShutdownExitReservation::AlreadyScheduled
        );
    }

    #[test]
    fn shutdown_capture_cleanup_wait_has_a_finite_bound() {
        let started = Instant::now();
        assert!(!shutdown_capture_cleanup_expired(
            started,
            started + SHUTDOWN_CAPTURE_CLEANUP_BOUND - Duration::from_millis(1)
        ));
        assert!(shutdown_capture_cleanup_expired(
            started,
            started + SHUTDOWN_CAPTURE_CLEANUP_BOUND
        ));
    }

    #[test]
    fn cancelled_active_capture_stays_busy_until_late_stop_and_file_cleanup_finish() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = Arc::new(AppContext::initialize(dir.path().to_path_buf()).unwrap());
        let mut rec = new_recording("openai/gpt-transcribe".into());
        rec.status = RecordingStatus::Processing;
        rec.raw_audio_path = Some(format!("{}.raw.wav", rec.id));
        ctx.history.lock().insert(&rec).unwrap();
        ctx.history
            .lock()
            .discard_cancelled_recording(&rec.id)
            .unwrap();
        let path = audio_dir(&ctx.data_dir).join(format!("{}.raw.wav", rec.id));
        ctx.capture_stopping.store(true, Ordering::SeqCst);

        let (release, wait) = std::sync::mpsc::channel();
        let worker_path = path.clone();
        let worker = thread::spawn(move || {
            wait.recv().unwrap();
            std::fs::write(&worker_path, b"late cancelled output").unwrap();
            Ok(crate::audio::capture::CaptureResult {
                path: worker_path,
                duration_ms: 1500,
                sample_rate: SAMPLE_RATE,
                samples: vec![0.1; 8],
                truncated: false,
                capture_error: None,
            })
        });
        let session = CaptureSession::from_test_worker(worker, path.clone());
        stop_cancelled_capture(
            Arc::clone(&ctx),
            None,
            session,
            Some(rec.id.clone()),
            Duration::from_millis(20),
        );

        assert!(ctx.capture_stopping.load(Ordering::SeqCst));
        assert!(crate::app::operations::system_busy(&ctx));
        assert!(ctx.history.lock().get(&rec.id).unwrap().is_none());
        release.send(()).unwrap();
        let deadline = std::time::Instant::now() + Duration::from_secs(2);
        while ctx.capture_stopping.load(Ordering::SeqCst) && std::time::Instant::now() < deadline {
            thread::sleep(Duration::from_millis(5));
        }
        assert!(!ctx.capture_stopping.load(Ordering::SeqCst));
        assert!(!crate::app::operations::system_busy(&ctx));
        assert!(!path.exists());
    }

    #[test]
    fn shutdown_pending_capture_start_saves_interrupted_metadata_before_releasing_reservation() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = Arc::new(AppContext::initialize(dir.path().to_path_buf()).unwrap());
        let mut rec = new_recording("openai/gpt-transcribe".into());
        rec.status = RecordingStatus::Processing;
        rec.raw_audio_path = Some(format!("{}.raw.wav", rec.id));
        ctx.history.lock().insert(&rec).unwrap();
        *ctx.session_recording_id.lock() = Some(rec.id.clone());
        ctx.capture_starting.store(true, Ordering::SeqCst);
        ctx.shutdown_requested.store(true, Ordering::SeqCst);

        let path = audio_dir(&ctx.data_dir).join(format!("{}.raw.wav", rec.id));
        let (release, wait) = std::sync::mpsc::channel();
        let worker_path = path.clone();
        let worker = thread::spawn(move || {
            wait.recv().unwrap();
            std::fs::write(&worker_path, b"late interrupted capture").unwrap();
            Ok(crate::audio::capture::CaptureResult {
                path: worker_path,
                duration_ms: 4321,
                sample_rate: SAMPLE_RATE,
                samples: vec![0.1; 16],
                truncated: false,
                capture_error: None,
            })
        });
        let session = CaptureSession::from_test_worker(worker, path.clone());
        stop_stale_capture_start_with_timeout(
            Arc::clone(&ctx),
            None,
            session,
            rec.id.clone(),
            Duration::from_millis(20),
        );
        assert!(ctx.capture_starting.load(Ordering::SeqCst));
        assert_eq!(
            ctx.history.lock().get(&rec.id).unwrap().unwrap().status,
            RecordingStatus::Processing
        );

        release.send(()).unwrap();
        let deadline = std::time::Instant::now() + Duration::from_secs(2);
        while ctx.capture_starting.load(Ordering::SeqCst) && std::time::Instant::now() < deadline {
            thread::sleep(Duration::from_millis(5));
        }
        let interrupted = ctx.history.lock().get(&rec.id).unwrap().unwrap();
        assert!(!ctx.capture_starting.load(Ordering::SeqCst));
        assert_eq!(interrupted.status, RecordingStatus::Interrupted);
        assert_eq!(interrupted.duration_ms, 4321);
        assert!(path.is_file());
    }

    #[test]
    fn late_shutdown_capture_result_updates_interrupted_recording_metadata() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = AppContext::initialize(dir.path().to_path_buf()).unwrap();
        let mut rec = new_recording("openai/gpt-transcribe".into());
        rec.status = RecordingStatus::Interrupted;
        rec.raw_audio_path = Some(format!("{}.raw.wav", rec.id));
        ctx.history.lock().insert(&rec).unwrap();
        let raw_path = audio_dir(&ctx.data_dir).join(rec.raw_audio_path.as_ref().unwrap());
        std::fs::create_dir_all(raw_path.parent().unwrap()).unwrap();

        persist_late_shutdown_capture(
            &ctx,
            &rec.id,
            Ok(crate::audio::capture::CaptureResult {
                path: raw_path,
                duration_ms: 4_321,
                sample_rate: SAMPLE_RATE,
                samples: Vec::new(),
                truncated: true,
                capture_error: None,
            }),
        );

        let updated = ctx.history.lock().get(&rec.id).unwrap().unwrap();
        assert_eq!(updated.status, RecordingStatus::Interrupted);
        assert_eq!(updated.duration_ms, 4_321);
        assert_eq!(
            updated.last_error_code.as_deref(),
            Some(AppError::RecordingTruncated.code())
        );
    }

    #[test]
    fn shutdown_during_normal_capture_stop_preserves_late_result_as_interrupted() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = AppContext::initialize(dir.path().to_path_buf()).unwrap();
        let mut rec = new_recording("openai/gpt-transcribe".into());
        rec.status = RecordingStatus::Processing;
        rec.raw_audio_path = Some(format!("{}.raw.wav", rec.id));
        ctx.history.lock().insert(&rec).unwrap();
        *ctx.session_recording_id.lock() = Some(rec.id.clone());
        ctx.shutdown_requested.store(true, Ordering::SeqCst);
        ctx.capture_stopping.store(true, Ordering::SeqCst);

        mark_shutdown_capture_interrupted(&ctx, Some(&rec.id));
        let raw_path = audio_dir(&ctx.data_dir).join(rec.raw_audio_path.as_ref().unwrap());
        std::fs::write(&raw_path, b"late normal-stop WAV").unwrap();
        discard_stale_capture_completion(
            &ctx,
            Some(&rec.id),
            &crate::audio::capture::CaptureResult {
                path: raw_path.clone(),
                duration_ms: 2_468,
                sample_rate: SAMPLE_RATE,
                samples: Vec::new(),
                truncated: false,
                capture_error: None,
            },
        );

        let saved = ctx.history.lock().get(&rec.id).unwrap().unwrap();
        assert_eq!(saved.status, RecordingStatus::Interrupted);
        assert_eq!(saved.duration_ms, 2_468);
        assert_eq!(
            saved.raw_audio_path.as_deref(),
            Some(raw_path.file_name().unwrap().to_str().unwrap())
        );
        assert!(raw_path.is_file());
    }

    #[test]
    fn shutdown_timeout_persists_metadata_when_native_capture_finishes_late() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = Arc::new(AppContext::initialize(dir.path().to_path_buf()).unwrap());
        let mut rec = new_recording("openai/gpt-transcribe".into());
        rec.status = RecordingStatus::Processing;
        rec.raw_audio_path = Some(format!("{}.raw.wav", rec.id));
        ctx.history.lock().insert(&rec).unwrap();
        let path = audio_dir(&ctx.data_dir).join(rec.raw_audio_path.as_ref().unwrap());
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let (release, wait) = std::sync::mpsc::channel();
        let writer_path = path.clone();
        let worker = thread::spawn(move || {
            wait.recv().unwrap();
            std::fs::write(&writer_path, b"late completed capture").unwrap();
            Ok(crate::audio::capture::CaptureResult {
                path: writer_path,
                duration_ms: 6_789,
                sample_rate: SAMPLE_RATE,
                samples: Vec::new(),
                truncated: true,
                capture_error: None,
            })
        });
        let session = CaptureSession::from_test_worker(worker, path);
        ctx.capture_stopping.store(true, Ordering::SeqCst);

        stop_capture_for_shutdown(
            &ctx,
            None,
            session,
            Some(rec.id.clone()),
            Duration::from_millis(20),
        );
        assert_eq!(
            ctx.history.lock().get(&rec.id).unwrap().unwrap().status,
            RecordingStatus::Interrupted
        );

        release.send(()).unwrap();
        let deadline = std::time::Instant::now() + Duration::from_secs(2);
        loop {
            let updated = ctx.history.lock().get(&rec.id).unwrap().unwrap();
            if updated.duration_ms == 6_789 {
                assert_eq!(
                    updated.last_error_code.as_deref(),
                    Some(AppError::RecordingTruncated.code())
                );
                break;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "late metadata was not persisted"
            );
            thread::sleep(Duration::from_millis(5));
        }
        assert!(!ctx.capture_stopping.load(Ordering::SeqCst));
    }

    #[test]
    fn finalized_wav_at_capture_limit_is_recovered_as_truncated() {
        let dir = tempfile::tempdir().unwrap();
        let name = "capture.wav";
        let wav = dir.path().join(name);
        write_sparse_pcm16_wav(&wav, crate::audio::capture::MAX_WAV_BYTES);
        let mut rec = new_recording("openai/gpt-transcribe".into());
        rec.raw_audio_path = Some(name.into());

        assert!(salvage_raw_tmp(dir.path(), &mut rec));
        assert!(rec.duration_ms > 0);
    }

    fn write_sparse_pcm16_wav(path: &std::path::Path, total_size: u64) {
        use std::io::Write;

        let data_size = (total_size - 44) as u32;
        let mut file = std::fs::File::create(path).unwrap();
        file.write_all(b"RIFF").unwrap();
        file.write_all(&((total_size - 8) as u32).to_le_bytes())
            .unwrap();
        file.write_all(b"WAVEfmt ").unwrap();
        file.write_all(&16u32.to_le_bytes()).unwrap();
        file.write_all(&1u16.to_le_bytes()).unwrap();
        file.write_all(&1u16.to_le_bytes()).unwrap();
        file.write_all(&48_000u32.to_le_bytes()).unwrap();
        file.write_all(&96_000u32.to_le_bytes()).unwrap();
        file.write_all(&2u16.to_le_bytes()).unwrap();
        file.write_all(&16u16.to_le_bytes()).unwrap();
        file.write_all(b"data").unwrap();
        file.write_all(&data_size.to_le_bytes()).unwrap();
        file.set_len(total_size).unwrap();
    }

    #[test]
    fn cancelled_pipeline_delete_failure_is_retried_until_history_and_usage_are_removed() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = AppContext::initialize(dir.path().to_path_buf()).unwrap();
        let database = ctx.data_dir.join("history.sqlite");
        let mut rec = new_recording("openai/gpt-transcribe".into());
        rec.status = RecordingStatus::Processing;
        rec.raw_audio_path = Some(format!("{}.raw.wav", rec.id));
        ctx.history.lock().insert(&rec).unwrap();
        *ctx.session_recording_id.lock() = Some(rec.id.clone());
        Connection::open(&database)
            .unwrap()
            .execute_batch(
                "CREATE TRIGGER reject_cancelled_delete BEFORE DELETE ON recordings BEGIN SELECT RAISE(ABORT, 'injected cancelled delete failure'); END;",
            )
            .unwrap();

        fail_active_recording_locked(&ctx, &AppError::Cancelled);
        assert!(ctx.history.lock().get(&rec.id).unwrap().is_some());
        assert!(ctx.session_recording_id.lock().is_none());

        Connection::open(&database)
            .unwrap()
            .execute_batch("DROP TRIGGER reject_cancelled_delete;")
            .unwrap();
        let deadline = std::time::Instant::now() + Duration::from_secs(3);
        while std::time::Instant::now() < deadline
            && ctx.history.lock().get(&rec.id).unwrap().is_some()
        {
            thread::sleep(Duration::from_millis(20));
        }
        assert!(ctx.history.lock().get(&rec.id).unwrap().is_none());
        let stats = ctx
            .history
            .lock()
            .get_usage_statistics(
                rec.created_at,
                rec.created_at + chrono::Duration::seconds(1),
            )
            .unwrap();
        assert_eq!((stats.dictations, stats.api_requests), (0, 0));
    }

    #[test]
    fn meter_cancelled_start_discards_before_readmitting_and_stale_stop_is_ignored() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = AppContext::initialize(dir.path().to_path_buf()).unwrap();
        let generation = reserve_meter_monitor(&ctx, "old").unwrap();
        assert!(crate::app::operations::system_busy(&ctx));
        assert!(reserve_meter_monitor(&ctx, "old").is_none());
        assert!(take_owned_meter_monitor(&ctx, "old").is_none());
        assert!(reserve_meter_monitor(&ctx, "new").is_none());
        let discarded = std::sync::atomic::AtomicBool::new(false);
        assert!(!finish_meter_start(
            &ctx,
            generation,
            Ok(()),
            |_| panic!("stale publish"),
            |_| {
                assert!(crate::app::operations::system_busy(&ctx));
                assert!(reserve_meter_monitor(&ctx, "new").is_none());
                discarded.store(true, Ordering::SeqCst);
                true
            }
        )
        .unwrap());
        assert!(discarded.load(Ordering::SeqCst));
        let new_generation = reserve_meter_monitor(&ctx, "new").unwrap();
        assert!(take_owned_meter_monitor(&ctx, "old").is_none());
        assert_eq!(ctx.meter_generation.load(Ordering::SeqCst), new_generation);
        assert_eq!(ctx.meter_owner.lock().as_deref(), Some("new"));
        assert!(finish_meter_start(
            &ctx,
            new_generation,
            Ok(()),
            |_| {},
            |_| panic!("new discard")
        )
        .unwrap());
        assert!(!ctx.meter_starting.load(Ordering::SeqCst));
    }

    #[test]
    fn meter_busy_and_failed_open_do_not_leave_a_reservation() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = AppContext::initialize(dir.path().to_path_buf()).unwrap();
        ctx.update_installing.store(true, Ordering::SeqCst);
        assert!(reserve_meter_monitor(&ctx, "owner").is_none());
        ctx.update_installing.store(false, Ordering::SeqCst);
        let generation = reserve_meter_monitor(&ctx, "owner").unwrap();
        assert!(finish_meter_start::<()>(
            &ctx,
            generation,
            Err(AppError::MicrophoneUnavailable),
            |_| {},
            |_| true
        )
        .is_err());
        assert!(!crate::app::operations::system_busy(&ctx));
        assert!(ctx.meter_owner.lock().is_none());
        assert!(reserve_meter_monitor(&ctx, "retry").is_some());
    }
    #[test]
    fn recovery_guard_preserves_orphans_during_configured_retention() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = AppContext::initialize(dir.path().to_path_buf()).unwrap();
        *ctx.settings_recovery.lock() = true;
        let orphan = audio_dir(dir.path()).join("orphan.wav");
        std::fs::write(&orphan, [1, 2, 3]).unwrap();
        assert!(apply_configured_retention(&ctx).unwrap().is_empty());
        assert!(orphan.is_file());
        ctx.settings.lock().theme = "light".into();
        assert!(apply_configured_retention(&ctx).unwrap().is_empty());
        assert!(orphan.is_file());
    }

    #[test]
    fn configured_retention_waits_for_retry_reservation_before_snapshotting_protection() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = Arc::new(AppContext::initialize(dir.path().to_path_buf()).unwrap());
        ctx.settings.lock().retention = "1d".into();
        let mut recording = new_recording("vendor/model".into());
        recording.status = RecordingStatus::Completed;
        recording.created_at = chrono::Utc::now() - chrono::Duration::days(3);
        let name = format!("{}.raw.wav", recording.id);
        let wav = audio_dir(dir.path()).join(&name);
        crate::audio::capture::write_pcm16_wav(&wav, crate::dsp::metrics::SAMPLE_RATE, &[0.1; 64])
            .unwrap();
        recording.raw_audio_path = Some(name);
        ctx.history.lock().insert(&recording).unwrap();

        let admission = crate::app::operations::lock_admission(&ctx);
        let started = Arc::new(std::sync::Barrier::new(2));
        let worker_started = started.clone();
        let worker_ctx = ctx.clone();
        let (done_tx, done_rx) = std::sync::mpsc::channel();
        let worker = std::thread::spawn(move || {
            worker_started.wait();
            let result = apply_configured_retention(&worker_ctx);
            done_tx.send(()).unwrap();
            result
        });
        started.wait();
        // The retention caller cannot snapshot or delete while retry admission is held.
        assert!(done_rx.recv_timeout(Duration::from_millis(100)).is_err());
        begin_transcription(&ctx, &recording.id).unwrap();
        drop(admission);
        assert!(worker.join().unwrap().unwrap().is_empty());
        assert!(ctx.history.lock().get(&recording.id).unwrap().is_some());
        assert!(wav.is_file());
    }

    use super::*;
    use crate::history::repository::{new_recording, RecordingStatus, TranscriptionAttempt};

    #[test]
    fn retry_does_not_start_when_history_row_was_removed_during_preparation() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = AppContext::initialize(dir.path().to_path_buf()).unwrap();
        let mut recording = new_recording("vendor/model".into());

        let err = persist_retry_processing(&ctx, &mut recording).unwrap_err();

        assert_eq!(
            err,
            AppError::StorageFailed("recording was removed before retry started".into())
        );
        assert!(ctx.history.lock().get(&recording.id).unwrap().is_none());
    }

    #[test]
    fn legacy_truncation_notice_is_not_confused_with_upload_size_failure() {
        assert!(preserve_capture_truncation_notice(
            Some("RecordingTruncated"),
            None
        ));
        assert!(preserve_capture_truncation_notice(
            Some("RecordingTooLarge"),
            Some("Recovered capture reached the maximum recording size")
        ));
        assert!(!preserve_capture_truncation_notice(
            Some("RecordingTooLarge"),
            Some("Recording is too large to send")
        ));
    }

    #[test]
    fn cancelled_generation_does_not_transcribe() {
        assert!(!stop_pipeline_allowed(false, true));
        assert!(!stop_pipeline_allowed(true, false));
        assert!(stop_pipeline_allowed(true, true));
    }

    #[test]
    fn second_transcription_claim_is_busy() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = AppContext::initialize(dir.path().to_path_buf()).unwrap();
        begin_transcription(&ctx, "rec-1").unwrap();
        assert_eq!(
            begin_transcription(&ctx, "rec-1").unwrap_err(),
            AppError::TranscriptionInProgress
        );
    }

    #[test]
    fn history_retry_does_not_drive_live_hud() {
        assert!(!history_retry_claims_live_hud());
        assert!(history_retry_allowed(&SessionState::Idle).is_ok());
        assert!(history_retry_allowed(&SessionState::Completed).is_ok());
        assert_eq!(
            history_retry_allowed(&SessionState::Transcribing { attempt: 1 }).unwrap_err(),
            AppError::TranscriptionInProgress
        );
        assert_eq!(
            toggle_hotkey_action(&SessionState::Idle),
            ToggleHotkeyAction::Start
        );
        let dir = tempfile::tempdir().unwrap();
        let ctx = AppContext::initialize(dir.path().to_path_buf()).unwrap();
        ctx.overlay.lock().show();
        let idle = ctx.state.lock().clone();
        assert_eq!(idle, SessionState::Idle);
        assert!(!ctx.overlay.lock().snapshot(idle).visible);
        assert!(ctx.session_recording_id.lock().is_none());
    }

    #[test]
    fn duplicate_stop_preserves_recording_audio_and_usage() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = AppContext::initialize(dir.path().to_path_buf()).unwrap();
        let mut rec = new_recording("openai/gpt-transcribe".into());
        rec.status = RecordingStatus::Processing;
        rec.raw_audio_path = Some(format!("{}.raw.wav", rec.id));
        let audio_path = audio_dir(&ctx.data_dir).join(rec.raw_audio_path.as_ref().unwrap());
        crate::audio::capture::write_pcm16_wav(&audio_path, 16_000, &[0.1; 160]).unwrap();
        ctx.history.lock().insert(&rec).unwrap();
        *ctx.session_recording_id.lock() = Some(rec.id.clone());
        *ctx.state.lock() = SessionState::Recording;
        ctx.session_generation.store(7, Ordering::SeqCst);
        let worker_path = audio_path.clone();
        let worker = thread::spawn(move || {
            Ok(crate::audio::capture::CaptureResult {
                path: worker_path,
                duration_ms: 10,
                sample_rate: 16_000,
                samples: vec![0.1; 160],
                truncated: false,
                capture_error: None,
            })
        });
        *ctx.capture.lock() = Some(CaptureSession::from_test_worker(worker, audio_path.clone()));
        let before = ctx
            .history
            .lock()
            .get_usage_statistics(
                rec.created_at,
                rec.created_at + chrono::Duration::seconds(1),
            )
            .unwrap();

        let capture = take_capture_for_stop(&ctx).unwrap().unwrap();
        assert_eq!(*ctx.state.lock(), SessionState::StoppingRecording);
        assert!(ctx.capture_stopping.load(Ordering::SeqCst));
        for _ in 0..3 {
            assert!(take_capture_for_stop(&ctx).unwrap().is_none());
        }
        assert_eq!(*ctx.state.lock(), SessionState::StoppingRecording);
        assert_eq!(ctx.session_generation.load(Ordering::SeqCst), 7);
        assert_eq!(
            ctx.session_recording_id.lock().as_deref(),
            Some(rec.id.as_str())
        );
        let saved = ctx.history.lock().get(&rec.id).unwrap().unwrap();
        assert_eq!(saved.status, RecordingStatus::Processing);
        assert_eq!(saved.last_error_code, None);
        let result = capture.stop().unwrap();
        assert_eq!(result.path, audio_path);
        assert!(audio_path.exists());
        let after = ctx
            .history
            .lock()
            .get_usage_statistics(
                rec.created_at,
                rec.created_at + chrono::Duration::seconds(1),
            )
            .unwrap();
        assert_eq!(after.dictations, before.dictations);
        assert_eq!(after.api_requests, before.api_requests);
        assert_eq!(after.reported_cost_usd, before.reported_cost_usd);
    }

    #[test]
    fn cancel_discards_audio_history_and_usage() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = AppContext::initialize(dir.path().to_path_buf()).unwrap();
        let mut rec = new_recording("openai/gpt-transcribe".into());
        rec.processed_audio_path = Some(format!("{}.processed.wav", rec.id));
        let audio_path = audio_dir(&ctx.data_dir).join(rec.processed_audio_path.as_ref().unwrap());
        crate::audio::capture::write_pcm16_wav(&audio_path, 16_000, &[0.0; 160]).unwrap();
        ctx.history.lock().insert(&rec).unwrap();
        ctx.history
            .lock()
            .record_attempt(
                &TranscriptionAttempt {
                    id: "cancelled-session-attempt".into(),
                    recording_id: rec.id.clone(),
                    attempt_number: 1,
                    started_at: rec.created_at,
                    ended_at: Some(rec.created_at),
                    outcome: "success".into(),
                    error_category: None,
                    http_status: Some(200),
                    latency_ms: Some(12),
                },
                Some(0.5),
            )
            .unwrap();
        *ctx.session_recording_id.lock() = Some(rec.id.clone());
        fail_active_recording(&ctx, &AppError::Cancelled);
        assert!(ctx.history.lock().get(&rec.id).unwrap().is_none());
        assert!(!audio_path.exists());
        let stats = ctx
            .history
            .lock()
            .get_usage_statistics(
                rec.created_at,
                rec.created_at + chrono::Duration::seconds(1),
            )
            .unwrap();
        assert_eq!((stats.dictations, stats.api_requests), (0, 0));
        assert_eq!(stats.reported_cost_usd, 0.0);
        assert!(ctx.session_recording_id.lock().is_none());
    }

    #[test]
    fn cancel_during_stopping_hides_history_until_audio_cleanup_finishes() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = AppContext::initialize(dir.path().to_path_buf()).unwrap();
        let mut rec = new_recording("openai/gpt-transcribe".into());
        rec.status = RecordingStatus::Processing;
        rec.raw_audio_path = Some(format!("{}.raw.wav", rec.id));
        let raw_audio = audio_dir(&ctx.data_dir).join(rec.raw_audio_path.as_ref().unwrap());
        std::fs::write(&raw_audio, b"capture being stopped").unwrap();
        ctx.history.lock().insert(&rec).unwrap();
        *ctx.session_recording_id.lock() = Some(rec.id.clone());
        *ctx.state.lock() = SessionState::StoppingRecording;
        ctx.capture_stopping.store(true, Ordering::SeqCst);

        discard_stopping_recording_locked(&ctx);

        let tombstone = ctx.history.lock().get(&rec.id).unwrap().unwrap();
        assert_eq!(tombstone.last_error_code.as_deref(), Some("Cancelled"));
        assert_eq!(tombstone.transcript, None);
        assert_eq!(ctx.history.lock().count(None).unwrap(), 0);
        assert_eq!(
            ctx.session_recording_id.lock().as_deref(),
            Some(rec.id.as_str())
        );
        assert!(raw_audio.exists());
        let stats = ctx
            .history
            .lock()
            .get_usage_statistics(
                rec.created_at,
                rec.created_at + chrono::Duration::seconds(1),
            )
            .unwrap();
        assert_eq!((stats.dictations, stats.api_requests), (0, 0));

        ctx.capture_stopping.store(false, Ordering::SeqCst);
        assert!(discard_cancelled_recording(&ctx, &rec.id));
        assert!(ctx.history.lock().get(&rec.id).unwrap().is_none());
        clear_live_session(&ctx, &rec.id);
        assert!(ctx.session_recording_id.lock().is_none());
        assert!(!raw_audio.exists());
    }

    #[test]
    fn failed_cancelled_history_delete_keeps_hidden_tombstone_and_suppresses_usage() {
        let dir = tempfile::tempdir().unwrap();
        let database = dir.path().join("history.sqlite");
        let history = HistoryRepo::open(&database).unwrap();
        let mut rec = new_recording("openai/gpt-transcribe".into());
        rec.status = RecordingStatus::Processing;
        rec.raw_audio_path = Some(format!("{}.raw.wav", rec.id));
        history.insert(&rec).unwrap();
        let raw_audio = dir
            .path()
            .join("audio")
            .join(rec.raw_audio_path.as_ref().unwrap());
        let upload_audio = dir
            .path()
            .join("audio")
            .join(format!("{}.raw.stt.wav", rec.id));
        std::fs::create_dir_all(raw_audio.parent().unwrap()).unwrap();
        std::fs::write(&raw_audio, b"cancelled capture").unwrap();
        std::fs::write(&upload_audio, b"upload artifact").unwrap();
        Connection::open(&database)
            .unwrap()
            .execute_batch(
                "CREATE TRIGGER reject_cancelled_delete BEFORE DELETE ON recordings \
                 BEGIN SELECT RAISE(ABORT, 'injected cancelled delete failure'); END;",
            )
            .unwrap();

        discard_cancelled_recording_from_repo(&history, raw_audio.parent().unwrap(), &rec.id);

        let tombstone = history.get(&rec.id).unwrap().unwrap();
        assert_eq!(tombstone.last_error_code.as_deref(), Some("Cancelled"));
        assert_eq!(tombstone.transcript, None);
        assert_eq!(tombstone.raw_audio_path, None);
        assert!(!raw_audio.exists());
        assert!(!upload_audio.exists());
        let stats = history
            .get_usage_statistics(
                rec.created_at,
                rec.created_at + chrono::Duration::seconds(1),
            )
            .unwrap();
        assert_eq!((stats.dictations, stats.api_requests), (0, 0));

        Connection::open(&database)
            .unwrap()
            .execute_batch("DROP TRIGGER reject_cancelled_delete;")
            .unwrap();
        discard_cancelled_recording_from_repo(&history, raw_audio.parent().unwrap(), &rec.id);
        assert!(history.get(&rec.id).unwrap().is_none());
        assert!(!raw_audio.exists());
        assert!(!upload_audio.exists());
        let stats = history
            .get_usage_statistics(
                rec.created_at,
                rec.created_at + chrono::Duration::seconds(1),
            )
            .unwrap();
        assert_eq!((stats.dictations, stats.api_requests), (0, 0));
    }

    #[test]
    fn pending_capture_completion_preserves_audio_when_cancel_delete_fails() {
        let dir = tempfile::tempdir().unwrap();
        let data = dir.path().to_path_buf();
        let ctx = AppContext::initialize(data.clone()).unwrap();
        let database = data.join("history.sqlite");
        let mut rec = new_recording("openai/gpt-transcribe".into());
        rec.status = RecordingStatus::Processing;
        rec.raw_audio_path = Some(format!("{}.raw.wav", rec.id));
        ctx.history.lock().insert(&rec).unwrap();
        *ctx.session_recording_id.lock() = Some(rec.id.clone());
        let audio_root = audio_dir(&data);
        let raw_audio = audio_root.join(rec.raw_audio_path.as_ref().unwrap());
        let cleanup = CaptureCleanupHandle::new(Some(raw_audio.clone()));
        *ctx.capture_cleanup.lock() = Some(cleanup.clone());
        Connection::open(database)
            .unwrap()
            .execute_batch(
                "CREATE TRIGGER reject_cancelled_delete BEFORE DELETE ON recordings \
                 BEGIN SELECT RAISE(ABORT, 'injected cancelled delete failure'); END;",
            )
            .unwrap();

        discard_uncommitted_recording_locked(&ctx);
        std::fs::write(&raw_audio, b"late finalized capture").unwrap();
        std::fs::write(raw_audio.with_extension("wav.tmp"), b"late partial capture").unwrap();
        cleanup.mark_finished();

        assert!(ctx.history.lock().get(&rec.id).unwrap().is_some());
        assert!(raw_audio.is_file());
        assert!(raw_audio.with_extension("wav.tmp").is_file());

        Connection::open(data.join("history.sqlite"))
            .unwrap()
            .execute_batch("DROP TRIGGER reject_cancelled_delete;")
            .unwrap();
        assert!(discard_cancelled_recording(&ctx, &rec.id));
        assert!(ctx.history.lock().get(&rec.id).unwrap().is_none());
        assert!(!raw_audio.exists());
        assert!(!raw_audio.with_extension("wav.tmp").exists());
    }

    #[test]
    fn startup_marks_stale_processing_with_audio_as_interrupted() {
        let dir = tempfile::tempdir().unwrap();
        let data = dir.path().to_path_buf();
        let history = HistoryRepo::open(&data.join("history.sqlite")).unwrap();
        let audio = audio_dir(&data);
        std::fs::create_dir_all(&audio).unwrap();
        let mut rec = new_recording("openai/gpt-transcribe".into());
        rec.raw_audio_path = Some(format!("{}.wav", rec.id));
        rec.status = RecordingStatus::Processing;
        let wav = audio.join(rec.raw_audio_path.as_ref().unwrap());
        crate::audio::capture::write_pcm16_wav(&wav, 16_000, &[0.0; 160]).unwrap();
        history.insert(&rec).unwrap();
        drop(history);

        let ctx = AppContext::initialize(data).unwrap();
        let kept = ctx.history.lock().get(&rec.id).unwrap().unwrap();
        assert_eq!(kept.status, RecordingStatus::Interrupted);
        assert_eq!(kept.last_error_code.as_deref(), Some("Interrupted"));
        assert!(wav.exists());
    }

    #[test]
    fn startup_cancel_cleanup_keeps_retryable_tombstone_when_audio_delete_fails() {
        let dir = tempfile::tempdir().unwrap();
        let database = dir.path().join("history.sqlite");
        let audio = dir.path().join("audio");
        std::fs::create_dir_all(&audio).unwrap();
        let mut rec = new_recording("openai/gpt-transcribe".into());
        rec.status = RecordingStatus::Failed;
        rec.last_error_code = Some("Cancelled".into());
        rec.last_error_message = Some("Cancelled".into());
        rec.transcript = Some("private text must be removed".into());
        rec.usage_json = Some(r#"{"total_tokens":12}"#.into());
        rec.cost = Some(0.42);
        rec.raw_audio_path = Some(format!("{}.raw.wav", rec.id));
        let raw_audio = audio.join(rec.raw_audio_path.as_ref().unwrap());
        let blocked_stt_path = audio.join(format!("{}.raw.stt.wav", rec.id));
        let history = HistoryRepo::open(&database).unwrap();
        history.insert(&rec).unwrap();
        history.update(&rec).unwrap();
        std::fs::write(&raw_audio, b"private audio").unwrap();
        std::fs::create_dir(&blocked_stt_path).unwrap();
        Connection::open(&database)
            .unwrap()
            .execute("DELETE FROM usage_meta WHERE key='remove-cancelled-v2'", [])
            .unwrap();
        drop(history);

        let history = HistoryRepo::open(&database).unwrap();
        let tombstone = history.get(&rec.id).unwrap().unwrap();
        assert_eq!(tombstone.last_error_code.as_deref(), Some("Cancelled"));
        assert_eq!(tombstone.transcript, None);
        assert_eq!(tombstone.raw_audio_path, None);
        assert_eq!(tombstone.usage_json, None);
        assert_eq!(tombstone.cost, None);
        assert_eq!(history.count(None).unwrap(), 0);

        cleanup_startup_cancelled_recordings(&history, &audio);
        assert!(history.get(&rec.id).unwrap().is_some());
        assert!(!raw_audio.exists());
        assert!(blocked_stt_path.is_dir());
        drop(history);

        let history = HistoryRepo::open(&database).unwrap();
        assert!(history.get(&rec.id).unwrap().is_some());
        std::fs::remove_dir(&blocked_stt_path).unwrap();
        cleanup_startup_cancelled_recordings(&history, &audio);
        assert!(history.get(&rec.id).unwrap().is_none());
        assert_eq!(history.count(None).unwrap(), 0);
    }

    #[test]
    fn startup_restores_duration_for_interrupted_finalized_capture() {
        let dir = tempfile::tempdir().unwrap();
        let data = dir.path().to_path_buf();
        let history = HistoryRepo::open(&data.join("history.sqlite")).unwrap();
        let audio = audio_dir(&data);
        std::fs::create_dir_all(&audio).unwrap();
        let mut rec = new_recording("openai/gpt-transcribe".into());
        rec.status = RecordingStatus::Interrupted;
        rec.duration_ms = 0;
        rec.raw_audio_path = Some(format!("{}.raw.wav", rec.id));
        let wav = audio.join(rec.raw_audio_path.as_ref().unwrap());
        crate::audio::capture::write_pcm16_wav(&wav, 16_000, &[0.0; 1_600]).unwrap();
        history.insert(&rec).unwrap();
        drop(history);

        let ctx = AppContext::initialize(data).unwrap();

        let recovered = ctx.history.lock().get(&rec.id).unwrap().unwrap();
        assert_eq!(recovered.status, RecordingStatus::Interrupted);
        assert_eq!(recovered.duration_ms, 100);
        assert_eq!(recovered.last_error_code.as_deref(), Some("Interrupted"));
        assert!(wav.is_file());
    }

    #[test]
    fn startup_repairs_partial_wav_and_restores_capture_duration() {
        let dir = tempfile::tempdir().unwrap();
        let data = dir.path().to_path_buf();
        let history = HistoryRepo::open(&data.join("history.sqlite")).unwrap();
        let audio = audio_dir(&data);
        std::fs::create_dir_all(&audio).unwrap();
        let mut rec = new_recording("openai/gpt-transcribe".into());
        rec.raw_audio_path = Some(format!("{}.raw.wav", rec.id));
        rec.status = RecordingStatus::Processing;
        let wav = audio.join(rec.raw_audio_path.as_ref().unwrap());
        let mut bytes = Vec::from(&b"RIFF\0\0\0\0WAVEfmt "[..]);
        bytes.extend_from_slice(&16u32.to_le_bytes());
        bytes.extend_from_slice(&1u16.to_le_bytes());
        bytes.extend_from_slice(&1u16.to_le_bytes());
        bytes.extend_from_slice(&SAMPLE_RATE.to_le_bytes());
        bytes.extend_from_slice(&(SAMPLE_RATE * 2).to_le_bytes());
        bytes.extend_from_slice(&2u16.to_le_bytes());
        bytes.extend_from_slice(&16u16.to_le_bytes());
        bytes.extend_from_slice(b"data\0\0\0\0");
        bytes.extend_from_slice(&vec![0u8; 4_800 * 2]);
        let tmp = wav.with_extension("wav.tmp");
        std::fs::write(&tmp, bytes).unwrap();
        history.insert(&rec).unwrap();
        drop(history);

        let ctx = AppContext::initialize(data).unwrap();
        let kept = ctx.history.lock().get(&rec.id).unwrap().unwrap();
        assert_eq!(kept.status, RecordingStatus::Interrupted);
        assert_eq!(kept.duration_ms, 100);
        assert!(wav.exists());
        assert!(!tmp.exists());
    }

    #[test]
    fn shutdown_persists_finalized_capture_as_interrupted() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = AppContext::initialize(dir.path().to_path_buf()).unwrap();
        let mut rec = new_recording("openai/gpt-transcribe".into());
        rec.status = RecordingStatus::Processing;
        rec.raw_audio_path = Some(format!("{}.raw.wav", rec.id));
        let path = audio_dir(&ctx.data_dir).join(rec.raw_audio_path.as_ref().unwrap());
        crate::audio::capture::write_pcm16_wav(&path, SAMPLE_RATE, &[0.0; 4_800]).unwrap();
        ctx.history.lock().insert(&rec).unwrap();
        let result = crate::audio::capture::CaptureResult {
            path: path.clone(),
            duration_ms: 100,
            sample_rate: SAMPLE_RATE,
            samples: Vec::new(),
            truncated: false,
            capture_error: None,
        };

        persist_interrupted_capture(&ctx, &rec.id, &result);

        let kept = ctx.history.lock().get(&rec.id).unwrap().unwrap();
        assert_eq!(kept.status, RecordingStatus::Interrupted);
        assert_eq!(kept.duration_ms, 100);
        assert_eq!(
            kept.raw_audio_path.as_deref(),
            rec.raw_audio_path.as_deref()
        );
        assert_eq!(kept.last_error_code.as_deref(), Some("Interrupted"));
    }

    #[test]
    fn shutdown_persists_capture_device_error_instead_of_generic_interruption() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = AppContext::initialize(dir.path().to_path_buf()).unwrap();
        let mut rec = new_recording("openai/gpt-transcribe".into());
        rec.status = RecordingStatus::Processing;
        rec.raw_audio_path = Some(format!("{}.raw.wav", rec.id));
        ctx.history.lock().insert(&rec).unwrap();
        let result = crate::audio::capture::CaptureResult {
            path: audio_dir(&ctx.data_dir).join(rec.raw_audio_path.as_ref().unwrap()),
            duration_ms: 100,
            sample_rate: SAMPLE_RATE,
            samples: Vec::new(),
            truncated: false,
            capture_error: Some("input device disconnected".into()),
        };

        persist_interrupted_capture(&ctx, &rec.id, &result);

        let kept = ctx.history.lock().get(&rec.id).unwrap().unwrap();
        assert_eq!(kept.status, RecordingStatus::Failed);
        assert_eq!(
            kept.last_error_code.as_deref(),
            Some(AppError::AudioCaptureFailed("input device disconnected".into()).code())
        );
    }

    #[test]
    fn retry_audio_validation_rejects_zero_sample_wav() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("empty.wav");
        write_pcm16_wav(&path, SAMPLE_RATE, &[]).unwrap();

        let error = validate_retry_audio_path(&path).unwrap_err();

        assert_eq!(
            error,
            AppError::AudioProcessingFailed("capture contains no audio samples".into())
        );
    }

    #[test]
    fn fallback_microphone_write_increments_settings_sequence() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = AppContext::initialize(dir.path().to_path_buf()).unwrap();
        let mut preferred = ctx.settings.lock().clone();
        preferred.input_device = "preferred-microphone".into();
        preferred.write_seq = 4;
        preferred.save(&ctx.settings_path).unwrap();
        *ctx.settings.lock() = preferred;

        let saved = persist_fallback_microphone_settings(&ctx).unwrap().unwrap();

        assert_eq!(saved.input_device, "default");
        assert_eq!(saved.write_seq, 5);
        assert_eq!(ctx.settings.lock().write_seq, 5);
    }

    #[test]
    fn startup_drops_stale_processing_without_audio() {
        let dir = tempfile::tempdir().unwrap();
        let data = dir.path().to_path_buf();
        let history = HistoryRepo::open(&data.join("history.sqlite")).unwrap();
        let mut rec = new_recording("openai/gpt-transcribe".into());
        rec.status = RecordingStatus::Processing;
        rec.raw_audio_path = Some("missing.wav".into());
        history.insert(&rec).unwrap();
        drop(history);

        let ctx = AppContext::initialize(data).unwrap();
        assert!(ctx.history.lock().get(&rec.id).unwrap().is_none());
    }

    #[test]
    fn startup_preserves_stale_recording_and_usage_when_atomic_delete_fails() {
        let dir = tempfile::tempdir().unwrap();
        let data = dir.path().to_path_buf();
        let database = data.join("history.sqlite");
        let history = HistoryRepo::open(&database).unwrap();
        let mut rec = new_recording("openai/gpt-transcribe".into());
        rec.status = RecordingStatus::Processing;
        rec.raw_audio_path = Some("missing.wav".into());
        history.insert(&rec).unwrap();
        drop(history);
        Connection::open(&database)
            .unwrap()
            .execute_batch(
                "CREATE TRIGGER reject_stale_recording_delete BEFORE DELETE ON recordings \
                 BEGIN SELECT RAISE(ABORT, 'injected stale delete failure'); END;",
            )
            .unwrap();

        let ctx = AppContext::initialize(data).unwrap();

        assert!(ctx.history.lock().get(&rec.id).unwrap().is_some());
        let stats = ctx
            .history
            .lock()
            .get_usage_statistics(
                rec.created_at,
                rec.created_at + chrono::Duration::seconds(1),
            )
            .unwrap();
        assert_eq!(stats.dictations, 1);
    }

    #[test]
    fn late_capture_after_shutdown_restores_interrupted_recording_metadata() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = AppContext::initialize(dir.path().to_path_buf()).unwrap();
        let mut rec = new_recording("openai/gpt-transcribe".into());
        rec.status = RecordingStatus::Processing;
        rec.raw_audio_path = Some(format!("{}.raw.wav", rec.id));
        ctx.history.lock().insert(&rec).unwrap();

        mark_recording_failed(&ctx, &rec.id, &AppError::Interrupted).unwrap();
        let path = audio_dir(&ctx.data_dir).join(format!("{}.raw.wav", rec.id));
        crate::audio::capture::write_pcm16_wav(&path, SAMPLE_RATE, &[0.0; 4_800]).unwrap();
        let result = crate::audio::capture::CaptureResult {
            path: path.clone(),
            duration_ms: 300,
            sample_rate: SAMPLE_RATE,
            samples: Vec::new(),
            truncated: true,
            capture_error: None,
        };
        discard_stale_capture_completion(&ctx, Some(&rec.id), &result);

        let kept = ctx.history.lock().get(&rec.id).unwrap().unwrap();
        assert_eq!(kept.status, RecordingStatus::Interrupted);
        assert_eq!(kept.duration_ms, 300);
        assert_eq!(kept.last_error_code.as_deref(), Some("RecordingTruncated"));
        assert_eq!(
            kept.raw_audio_path.as_deref(),
            path.file_name().unwrap().to_str()
        );
        assert!(path.is_file());
    }

    #[test]
    fn cancel_during_live_capture_drops_history_row() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = AppContext::initialize(dir.path().to_path_buf()).unwrap();
        let mut rec = new_recording("openai/gpt-transcribe".into());
        rec.status = RecordingStatus::Processing;
        rec.raw_audio_path = Some(format!("{}.raw.wav", rec.id));
        rec.processed_audio_path = Some(format!("{}.processed.wav", rec.id));
        let audio_root = audio_dir(&ctx.data_dir);
        std::fs::create_dir_all(&audio_root).unwrap();
        let expected_files = [
            format!("{}.raw.wav", rec.id),
            format!("{}.processed.wav", rec.id),
            format!("{}.raw.stt.wav", rec.id),
            format!("{}.processed.stt.wav", rec.id),
        ];
        for name in &expected_files {
            std::fs::write(audio_root.join(name), b"audio").unwrap();
            std::fs::write(audio_root.join(name).with_extension("wav.tmp"), b"partial").unwrap();
        }
        ctx.history.lock().insert(&rec).unwrap();
        *ctx.session_recording_id.lock() = Some(rec.id.clone());
        discard_uncommitted_recording(&ctx);
        assert!(ctx.session_recording_id.lock().is_none());
        assert!(ctx.history.lock().get(&rec.id).unwrap().is_none());
        assert!(expected_files.iter().all(|name| {
            !audio_root.join(name).exists()
                && !audio_root.join(name).with_extension("wav.tmp").exists()
        }));

        // A capture start that finishes after cancellation must also remove its late WAV.
        let late_capture = audio_root.join(format!("{}.raw.wav", rec.id));
        std::fs::write(&late_capture, b"late capture").unwrap();
        std::fs::write(late_capture.with_extension("wav.tmp"), b"partial").unwrap();
        *ctx.session_recording_id.lock() = Some(rec.id.clone());
        discard_stale_capture_result(&ctx, Some(&rec.id), &late_capture);
        assert!(ctx.session_recording_id.lock().is_none());
        assert!(!late_capture.exists());
        assert!(!late_capture.with_extension("wav.tmp").exists());
        let stats = ctx
            .history
            .lock()
            .get_usage_statistics(
                rec.created_at,
                rec.created_at + chrono::Duration::seconds(1),
            )
            .unwrap();
        assert_eq!((stats.dictations, stats.api_requests), (0, 0));
    }

    #[test]
    fn stale_capture_cleanup_releases_history_lock_before_deleting() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = Arc::new(AppContext::initialize(dir.path().to_path_buf()).unwrap());
        let mut result_recording = new_recording("openai/gpt-transcribe".into());
        result_recording.status = RecordingStatus::Processing;
        let mut failure_recording = new_recording("openai/gpt-transcribe".into());
        failure_recording.status = RecordingStatus::Processing;
        ctx.history.lock().insert(&result_recording).unwrap();
        ctx.history.lock().insert(&failure_recording).unwrap();
        let capture_path =
            audio_dir(&ctx.data_dir).join(format!("{}.raw.wav", result_recording.id));
        std::fs::write(&capture_path, b"stale capture").unwrap();

        let worker_ctx = Arc::clone(&ctx);
        let result_id = result_recording.id.clone();
        let failure_id = failure_recording.id.clone();
        let worker_capture_path = capture_path.clone();
        let (finished, wait) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            discard_stale_capture_result(&worker_ctx, Some(&result_id), &worker_capture_path);
            discard_stale_capture_failure(&worker_ctx, &failure_id);
            let _ = finished.send(());
        });

        wait.recv_timeout(Duration::from_secs(2))
            .expect("stale capture cleanup deadlocked while deleting history");
        assert!(ctx
            .history
            .lock()
            .get(&result_recording.id)
            .unwrap()
            .is_none());
        assert!(ctx
            .history
            .lock()
            .get(&failure_recording.id)
            .unwrap()
            .is_none());
        assert!(!capture_path.exists());
    }

    #[test]
    fn late_pipeline_cleanup_removes_only_cancelled_take_audio() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = AppContext::initialize(dir.path().to_path_buf()).unwrap();
        let mut cancelled = new_recording("openai/gpt-transcribe".into());
        cancelled.raw_audio_path = Some(format!("{}.raw.wav", cancelled.id));
        ctx.history.lock().insert(&cancelled).unwrap();
        *ctx.session_recording_id.lock() = Some(cancelled.id.clone());
        let audio_root = audio_dir(&ctx.data_dir);
        let late_processed = audio_root.join(format!("{}.processed.wav", cancelled.id));

        // Cancellation removes existing artifacts, but the DSP worker may finish writing later.
        discard_uncommitted_recording(&ctx);
        std::fs::write(&late_processed, b"late DSP result").unwrap();
        std::fs::write(late_processed.with_extension("wav.tmp"), b"late temp").unwrap();

        let mut next_take = new_recording("openai/gpt-transcribe".into());
        next_take.raw_audio_path = Some(format!("{}.raw.wav", next_take.id));
        ctx.history.lock().insert(&next_take).unwrap();
        *ctx.session_recording_id.lock() = Some(next_take.id.clone());
        let next_take_audio = audio_root.join(next_take.raw_audio_path.as_ref().unwrap());
        std::fs::write(&next_take_audio, b"new take").unwrap();

        cleanup_stale_pipeline_audio(&ctx, &cancelled.id);

        assert!(!late_processed.exists());
        assert!(!late_processed.with_extension("wav.tmp").exists());
        assert!(next_take_audio.exists());
        assert!(ctx.history.lock().get(&next_take.id).unwrap().is_some());
        assert_eq!(
            ctx.session_recording_id.lock().as_deref(),
            Some(next_take.id.as_str())
        );
    }

    #[test]
    fn late_pipeline_cleanup_preserves_interrupted_audio_and_removes_upload_artifacts() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = AppContext::initialize(dir.path().to_path_buf()).unwrap();
        let mut rec = new_recording("openai/gpt-transcribe".into());
        rec.raw_audio_path = Some(format!("{}.raw.wav", rec.id));
        rec.processed_audio_path = Some(format!("{}.processed.wav", rec.id));
        let audio_root = audio_dir(&ctx.data_dir);
        let raw_path = audio_root.join(rec.raw_audio_path.as_ref().unwrap());
        let processed_path = audio_root.join(rec.processed_audio_path.as_ref().unwrap());
        let raw_upload_path = audio_root.join(format!("{}.raw.stt.wav", rec.id));
        let processed_upload_path = audio_root.join(format!("{}.processed.stt.wav", rec.id));
        std::fs::write(&raw_path, b"original capture").unwrap();
        std::fs::write(&processed_path, b"processed audio").unwrap();
        std::fs::write(&raw_upload_path, b"transcription upload").unwrap();
        std::fs::write(&processed_upload_path, b"transcription upload").unwrap();
        ctx.history.lock().insert(&rec).unwrap();

        persist_interrupted_capture(
            &ctx,
            &rec.id,
            &crate::audio::capture::CaptureResult {
                path: raw_path.clone(),
                duration_ms: 100,
                sample_rate: SAMPLE_RATE,
                samples: Vec::new(),
                truncated: false,
                capture_error: None,
            },
        );
        cleanup_stale_pipeline_audio(&ctx, &rec.id);

        let kept = ctx.history.lock().get(&rec.id).unwrap().unwrap();
        assert_eq!(kept.status, RecordingStatus::Interrupted);
        assert!(raw_path.exists());
        assert!(processed_path.exists());
        assert!(!raw_upload_path.exists());
        assert!(!processed_upload_path.exists());
    }

    #[test]
    fn stale_cleanup_cannot_clear_a_new_session_slot() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = Arc::new(AppContext::initialize(dir.path().to_path_buf()).unwrap());
        *ctx.session_recording_id.lock() = Some("old-take".into());
        let lifecycle = ctx.session_lifecycle.lock();

        let worker_ctx = Arc::clone(&ctx);
        let (started, wait_started) = std::sync::mpsc::channel();
        let (finished, wait_finished) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let _ = started.send(());
            clear_live_session(&worker_ctx, "old-take");
            let _ = finished.send(());
        });
        wait_started
            .recv_timeout(Duration::from_secs(2))
            .expect("cleanup worker did not start");
        assert!(
            wait_finished
                .recv_timeout(Duration::from_millis(100))
                .is_err(),
            "cleanup must wait while a new session slot is being published"
        );

        let next_take = "new-take".to_string();
        *ctx.session_recording_id.lock() = Some(next_take.clone());
        *ctx.captured_target.lock() = Some(CapturedTarget::from_hwnd(
            NativeHwnd { value: 31 },
            30,
            7,
            9,
            2,
        ));
        let (cancel_tx, cancel_rx) = tokio::sync::watch::channel(false);
        *ctx.cancel_tx.lock() = Some(cancel_tx);
        drop(lifecycle);

        wait_finished
            .recv_timeout(Duration::from_secs(2))
            .expect("stale cleanup did not finish");
        assert_eq!(
            ctx.session_recording_id.lock().as_deref(),
            Some(next_take.as_str())
        );
        assert_eq!(ctx.captured_target.lock().unwrap().generation, 2);
        assert!(!*cancel_rx.borrow());
    }

    #[test]
    fn stale_pipeline_failure_cannot_replace_a_new_generation_state() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = Arc::new(AppContext::initialize(dir.path().to_path_buf()).unwrap());
        ctx.session_generation.store(10, Ordering::SeqCst);
        *ctx.state.lock() = SessionState::Recording;
        let lifecycle = ctx.session_lifecycle.lock();

        let worker_ctx = Arc::clone(&ctx);
        let (started, wait_started) = std::sync::mpsc::channel();
        let (finished, wait_finished) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let _ = started.send(());
            if let Some(_lifecycle) = lock_current_session_generation(&worker_ctx, 10) {
                *worker_ctx.state.lock() = SessionState::Failed {
                    message: "old error".into(),
                    code: "OldError".into(),
                };
                let _ = finished.send(true);
            } else {
                let _ = finished.send(false);
            }
        });
        wait_started
            .recv_timeout(Duration::from_secs(2))
            .expect("pipeline worker did not start");
        assert!(wait_finished
            .recv_timeout(Duration::from_millis(100))
            .is_err());

        // A new recording is published before the stale pipeline can acquire its guard.
        ctx.session_generation.store(11, Ordering::SeqCst);
        *ctx.state.lock() = SessionState::Recording;
        drop(lifecycle);

        assert!(!wait_finished
            .recv_timeout(Duration::from_secs(2))
            .expect("stale pipeline worker did not finish"));
        assert_eq!(*ctx.state.lock(), SessionState::Recording);
    }

    #[test]
    fn write_stt_upload_downsamples_disk_wav() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("clip.processed.wav");
        let sine: Vec<f32> = (0..4800)
            .map(|i| (i as f32 * 440.0 * 2.0 * std::f32::consts::PI / 48_000.0).sin() * 0.2)
            .collect();
        crate::audio::capture::write_pcm16_wav(&path, SAMPLE_RATE, &sine).unwrap();
        let stt = write_stt_upload(&path).unwrap();
        let (pcm, rate) = read_pcm16_wav_with_rate(&stt).unwrap();
        assert_eq!(rate, STT_SAMPLE_RATE);
        assert!((pcm.len() as i32 - 1600).abs() <= 2);
        assert!(pcm.iter().all(|s| s.is_finite()));
    }

    #[test]
    fn delayed_pipeline_cannot_adopt_shutdown_or_new_session_generation() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = Arc::new(AppContext::initialize(dir.path().to_path_buf()).unwrap());
        let mut rec = new_recording("test/model".into());
        rec.status = RecordingStatus::Interrupted;
        ctx.history.lock().insert(&rec).unwrap();
        ctx.session_generation.store(10, Ordering::SeqCst);
        *ctx.session_recording_id.lock() = Some(rec.id.clone());
        let (tx, _rx) = tokio::sync::watch::channel(false);
        *ctx.cancel_tx.lock() = Some(tx);
        let (checked, checked_wait) = std::sync::mpsc::channel();
        let (resume, resume_wait) = std::sync::mpsc::channel();
        let worker_ctx = ctx.clone();
        let worker_id = rec.id.clone();
        let worker = thread::spawn(move || {
            assert_eq!(worker_ctx.session_generation.load(Ordering::SeqCst), 10);
            checked.send(()).unwrap();
            resume_wait.recv_timeout(Duration::from_secs(2)).unwrap();
            begin_live_transcription(&worker_ctx, &worker_id, 10)
        });
        checked_wait.recv_timeout(Duration::from_secs(2)).unwrap();
        {
            let _lifecycle = ctx.session_lifecycle.lock();
            ctx.shutdown_requested.store(true, Ordering::SeqCst);
            ctx.session_generation.fetch_add(1, Ordering::SeqCst);
            ctx.cancel_tx.lock().take();
        }
        resume.send(()).unwrap();
        assert!(matches!(worker.join().unwrap(), Err(AppError::Cancelled)));
        assert!(ctx.in_flight.lock().is_empty());
        assert!(ctx.cancel_tx.lock().is_none());
        assert_eq!(ctx.history.lock().get(&rec.id).unwrap().unwrap(), rec);
    }

    #[test]
    fn live_pipeline_requires_the_original_owner_and_cancel_sender() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = AppContext::initialize(dir.path().to_path_buf()).unwrap();
        ctx.session_generation.store(10, Ordering::SeqCst);
        *ctx.session_recording_id.lock() = Some("live".into());
        assert!(matches!(
            begin_live_transcription(&ctx, "live", 10),
            Err(AppError::Cancelled)
        ));
        let (tx, _rx) = tokio::sync::watch::channel(false);
        *ctx.cancel_tx.lock() = Some(tx.clone());
        assert!(matches!(
            begin_live_transcription(&ctx, "other", 10),
            Err(AppError::Cancelled)
        ));
        assert!(matches!(
            begin_live_transcription(&ctx, "live", 9),
            Err(AppError::Cancelled)
        ));
        ctx.shutdown_requested.store(true, Ordering::SeqCst);
        assert!(matches!(
            begin_live_transcription(&ctx, "live", 10),
            Err(AppError::Cancelled)
        ));
        ctx.shutdown_requested.store(false, Ordering::SeqCst);
        let cancel = begin_live_transcription(&ctx, "live", 10).unwrap();
        assert!(ctx.in_flight.lock().contains("live"));
        assert!(matches!(
            begin_live_transcription(&ctx, "live", 10),
            Err(AppError::TranscriptionInProgress)
        ));
        ctx.in_flight.lock().clear();
        tx.send(true).unwrap();
        assert!(*cancel.borrow());
        assert!(matches!(
            begin_live_transcription(&ctx, "live", 10),
            Err(AppError::Cancelled)
        ));
        assert!(ctx.in_flight.lock().is_empty());
    }

    #[test]
    fn late_live_metadata_cannot_revive_cancelled_or_interrupted_recordings() {
        for cancelled in [true, false] {
            let dir = tempfile::tempdir().unwrap();
            let ctx = AppContext::initialize(dir.path().to_path_buf()).unwrap();
            let rec = new_recording("test/model".into());
            ctx.history.lock().insert(&rec).unwrap();
            ctx.session_generation.store(10, Ordering::SeqCst);
            let (_tx, rx) = tokio::sync::watch::channel(false);
            let mut terminal = rec.clone();
            terminal.status = if cancelled {
                RecordingStatus::Failed
            } else {
                RecordingStatus::Interrupted
            };
            terminal.last_error_code = Some(
                if cancelled {
                    "Cancelled"
                } else {
                    "Interrupted"
                }
                .into(),
            );
            ctx.history.lock().update(&terminal).unwrap();
            ctx.session_generation.fetch_add(1, Ordering::SeqCst);
            let mut stale = rec.clone();
            stale.request_started_at = Some(chrono::Utc::now());
            assert_eq!(
                persist_live_recording(&ctx, 10, &rx, &stale),
                Err(AppError::Cancelled)
            );
            stale.status = RecordingStatus::Failed;
            stale.last_error_code = Some("InvalidApiKey".into());
            assert_eq!(
                persist_live_recording(&ctx, 10, &rx, &stale),
                Err(AppError::Cancelled)
            );
            assert_eq!(ctx.history.lock().get(&rec.id).unwrap().unwrap(), terminal);
        }
    }

    #[test]
    fn live_metadata_requires_current_generation_and_existing_row() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = AppContext::initialize(dir.path().to_path_buf()).unwrap();
        ctx.session_generation.store(10, Ordering::SeqCst);
        let (tx, rx) = tokio::sync::watch::channel(false);
        let mut rec = new_recording("test/model".into());
        assert!(matches!(
            persist_live_recording(&ctx, 10, &rx, &rec),
            Err(AppError::StorageFailed(_))
        ));
        ctx.history.lock().insert(&rec).unwrap();
        rec.request_started_at = Some(chrono::Utc::now());
        persist_live_recording(&ctx, 10, &rx, &rec).unwrap();
        tx.send(true).unwrap();
        rec.last_error_code = Some("stale".into());
        assert_eq!(
            persist_live_recording(&ctx, 10, &rx, &rec),
            Err(AppError::Cancelled)
        );
        assert!(ctx
            .history
            .lock()
            .get(&rec.id)
            .unwrap()
            .unwrap()
            .last_error_code
            .is_none());
    }

    #[test]
    fn completed_insertion_releases_lifecycle_before_waiting_for_ui_cancel() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = Arc::new(AppContext::initialize(dir.path().to_path_buf()).unwrap());
        let mut rec = new_recording("test/model".into());
        rec.status = RecordingStatus::Completed;
        rec.transcript = Some("Recoverable transcript".into());
        ctx.history.lock().insert(&rec).unwrap();
        *ctx.session_recording_id.lock() = Some(rec.id.clone());
        *ctx.state.lock() = SessionState::Transcribing { attempt: 1 };
        ctx.session_generation.store(10, Ordering::SeqCst);
        let (ui_request, ui_wait) = std::sync::mpsc::channel();
        let (ui_response, response_wait) = std::sync::mpsc::channel();
        let worker_ctx = ctx.clone();
        let worker = thread::spawn(move || {
            let lifecycle = worker_ctx.session_lifecycle.lock();
            let insertion = prepare_stt_insertion(&worker_ctx, lifecycle);
            ui_request.send(()).unwrap();
            // Model a synchronous HWND getter waiting for the UI thread.
            response_wait.recv_timeout(Duration::from_secs(2)).unwrap();
            assert_eq!(insertion.mode, "unicode");
            assert!(insert_should_abort(
                10,
                worker_ctx.session_generation.load(Ordering::SeqCst)
            ));
        });
        ui_wait.recv_timeout(Duration::from_secs(2)).unwrap();
        let lifecycle = ctx
            .session_lifecycle
            .try_lock_for(Duration::from_secs(1))
            .expect("UI cancellation must not wait for the worker's HWND response");
        assert_eq!(*ctx.state.lock(), SessionState::Idle);
        assert!(ctx.insert_in_flight.load(Ordering::SeqCst));
        assert!(crate::app::operations::system_busy(&ctx));
        assert!(ctx.session_recording_id.lock().is_none());
        ctx.session_generation.fetch_add(1, Ordering::SeqCst);
        fail_active_recording_locked(&ctx, &AppError::Cancelled);
        drop(lifecycle);
        ui_response.send(()).unwrap();
        worker.join().unwrap();
        let persisted = ctx.history.lock().get(&rec.id).unwrap().unwrap();
        assert_eq!(persisted.status, RecordingStatus::Completed);
        assert_eq!(persisted.transcript, rec.transcript);
    }

    #[test]
    fn finalized_capture_survives_lookup_failure_and_restart_recovery() {
        let dir = tempfile::tempdir().unwrap();
        let data = dir.path().to_path_buf();
        let ctx = AppContext::initialize(data.clone()).unwrap();
        let mut rec = new_recording("test/model".into());
        let raw_name = format!("{}.raw.wav", rec.id);
        rec.raw_audio_path = Some(raw_name.clone());
        ctx.history.lock().insert(&rec).unwrap();
        let raw = audio_dir(&data).join(&raw_name);
        write_pcm16_wav(&raw, SAMPLE_RATE, &[0.1; 4800]).unwrap();
        let result = crate::audio::capture::CaptureResult {
            path: raw.clone(),
            duration_ms: 100,
            sample_rate: SAMPLE_RATE,
            samples: vec![],
            truncated: false,
            capture_error: None,
        };
        let fault = rusqlite::Connection::open(data.join("history.sqlite")).unwrap();
        fault
            .execute_batch("ALTER TABLE recordings RENAME TO unavailable_recordings")
            .unwrap();
        assert!(persist_capture_result(&ctx.history.lock(), &rec.id, &result).is_err());
        assert!(
            raw.is_file(),
            "storage lookup failure must not discard capture"
        );
        fault
            .execute_batch("ALTER TABLE unavailable_recordings RENAME TO recordings")
            .unwrap();
        drop(fault);
        drop(ctx);
        let restarted = AppContext::initialize(data.clone()).unwrap();
        let recovered = restarted.history.lock().get(&rec.id).unwrap().unwrap();
        assert_eq!(recovered.status, RecordingStatus::Interrupted);
        assert_eq!(recovered.duration_ms, 100);
        assert_eq!(recovered.raw_audio_path.as_deref(), Some(raw_name.as_str()));
        assert!(can_retry_from_history(&recovered));
        assert!(raw.is_file());
        assert_eq!(read_pcm16_wav_with_rate(&raw).unwrap().0.len(), 4800);
    }

    #[test]
    fn finalized_capture_write_failure_preserves_audio_and_actual_storage_error() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = AppContext::initialize(dir.path().to_path_buf()).unwrap();
        let mut rec = new_recording("test/model".into());
        rec.raw_audio_path = Some(format!("{}.raw.wav", rec.id));
        ctx.history.lock().insert(&rec).unwrap();
        let raw = audio_dir(dir.path()).join(rec.raw_audio_path.as_ref().unwrap());
        write_pcm16_wav(&raw, SAMPLE_RATE, &[0.1; 4800]).unwrap();
        let result = crate::audio::capture::CaptureResult {
            path: raw.clone(),
            duration_ms: 100,
            sample_rate: SAMPLE_RATE,
            samples: vec![],
            truncated: false,
            capture_error: None,
        };
        let fault = rusqlite::Connection::open(dir.path().join("history.sqlite")).unwrap();
        fault.execute_batch("CREATE TRIGGER reject_capture BEFORE UPDATE ON recordings BEGIN SELECT RAISE(ABORT, 'capture-write-fault'); END;").unwrap();
        let err = persist_capture_result(&ctx.history.lock(), &rec.id, &result).unwrap_err();
        assert!(
            matches!(err, AppError::StorageFailed(ref detail) if detail.contains("capture-write-fault"))
        );
        assert!(raw.is_file());
        fault.execute_batch("DROP TRIGGER reject_capture").unwrap();
        let (saved, failure) =
            persist_capture_result(&ctx.history.lock(), &rec.id, &result).unwrap();
        assert!(failure.is_none());
        assert_eq!(saved.duration_ms, 100);
        cleanup_orphans(&audio_dir(dir.path()), &ctx.history.lock()).unwrap();
        assert!(raw.is_file());
    }

    #[test]
    fn finalized_capture_is_discarded_only_for_confirmed_deleted_recording() {
        let dir = tempfile::tempdir().unwrap();
        let history = HistoryRepo::open(&dir.path().join("history.sqlite")).unwrap();
        let raw = dir.path().join("deleted.raw.wav");
        write_pcm16_wav(&raw, SAMPLE_RATE, &[0.1; 48]).unwrap();
        let result = crate::audio::capture::CaptureResult {
            path: raw.clone(),
            duration_ms: 1,
            sample_rate: SAMPLE_RATE,
            samples: vec![],
            truncated: false,
            capture_error: None,
        };
        assert!(persist_capture_result(&history, "deleted", &result).is_err());
        assert!(!raw.exists());
    }

    #[test]
    fn processed_reference_commit_failures_keep_retry_audio_after_restart() {
        for reject_raw_cleanup in [false, true] {
            let dir = tempfile::tempdir().unwrap();
            let data = dir.path().to_path_buf();
            let ctx = AppContext::initialize(data.clone()).unwrap();
            let mut rec = new_recording("test/model".into());
            rec.raw_audio_path = Some(format!("{}.raw.wav", rec.id));
            ctx.history.lock().insert(&rec).unwrap();
            let audio = audio_dir(&data);
            let raw = audio.join(rec.raw_audio_path.as_ref().unwrap());
            write_pcm16_wav(&raw, SAMPLE_RATE, &[0.1; 4800]).unwrap();
            let processed_name = format!("{}.processed.wav", rec.id);
            let processed = audio.join(&processed_name);
            write_pcm16_wav(&processed, SAMPLE_RATE, &[0.2; 4800]).unwrap();
            rec.processed_audio_path = Some(processed_name.clone());
            let fault = rusqlite::Connection::open(data.join("history.sqlite")).unwrap();
            let condition = if reject_raw_cleanup {
                "NEW.raw_audio_path IS NULL"
            } else {
                "NEW.processed_audio_path IS NOT NULL"
            };
            fault.execute_batch(&format!("CREATE TRIGGER reject_audio_commit BEFORE UPDATE ON recordings WHEN {condition} BEGIN SELECT RAISE(ABORT, 'audio-commit-fault'); END;")).unwrap();
            assert!(persist_processed_audio(&ctx.history.lock(), &mut rec, false, &raw).is_err());
            let persisted = ctx.history.lock().get(&rec.id).unwrap().unwrap();
            if reject_raw_cleanup {
                assert_eq!(
                    persisted.processed_audio_path.as_deref(),
                    Some(processed_name.as_str())
                );
                assert!(!raw.exists());
                assert!(processed.is_file());
            } else {
                assert!(persisted.processed_audio_path.is_none());
                assert!(raw.is_file());
            }
            fault
                .execute_batch("DROP TRIGGER reject_audio_commit")
                .unwrap();
            drop(fault);
            drop(ctx);
            let restarted = AppContext::initialize(data).unwrap();
            let recovered = restarted.history.lock().get(&rec.id).unwrap().unwrap();
            assert_eq!(recovered.status, RecordingStatus::Interrupted);
            assert_eq!(recovered.duration_ms, 100);
            assert!(can_retry_from_history(&recovered));
            let retry_name =
                crate::history::repository::history_play_name(&recovered, false).unwrap();
            assert!(audio.join(retry_name).is_file());
        }
    }

    #[test]
    fn raw_cleanup_failure_keeps_both_references() {
        let dir = tempfile::tempdir().unwrap();
        let history = HistoryRepo::open(&dir.path().join("history.sqlite")).unwrap();
        let mut rec = new_recording("test/model".into());
        rec.raw_audio_path = Some("raw.wav".into());
        history.insert(&rec).unwrap();
        rec.processed_audio_path = Some("processed.wav".into());
        let raw = dir.path().join("raw.wav");
        // remove_file must fail for a directory, independent of host permissions.
        std::fs::create_dir(&raw).unwrap();
        persist_processed_audio(&history, &mut rec, false, &raw).unwrap();
        let saved = history.get(&rec.id).unwrap().unwrap();
        assert_eq!(saved.raw_audio_path.as_deref(), Some("raw.wav"));
        assert_eq!(saved.processed_audio_path.as_deref(), Some("processed.wav"));
        assert!(raw.is_dir());
    }

    #[test]
    fn dropping_original_removes_raw_file() {
        let dir = tempfile::tempdir().unwrap();
        let raw = dir.path().join("raw.wav");
        crate::audio::capture::write_pcm16_wav(&raw, SAMPLE_RATE, &[0.1; 48]).unwrap();
        let mut rec = new_recording("openai/gpt-transcribe".into());
        rec.raw_audio_path = Some("raw.wav".into());
        apply_raw_retention(&mut rec, false, &raw);
        assert!(rec.raw_audio_path.is_none());
        assert!(!raw.exists());
        let kept = dir.path().join("keep.wav");
        crate::audio::capture::write_pcm16_wav(&kept, SAMPLE_RATE, &[0.1; 48]).unwrap();
        rec.raw_audio_path = Some("keep.wav".into());
        apply_raw_retention(&mut rec, true, &kept);
        assert_eq!(rec.raw_audio_path.as_deref(), Some("keep.wav"));
        assert!(kept.exists());
    }

    #[test]
    fn prepare_local_stt_writes_processed_and_stt() {
        let dir = tempfile::tempdir().unwrap();
        let processed = dir.path().join("rec.processed.wav");
        let sine: Vec<f32> = (0..4800)
            .map(|i| (i as f32 * 440.0 * 2.0 * std::f32::consts::PI / 48_000.0).sin() * 0.2)
            .collect();
        let stt = prepare_local_stt(
            crate::dsp::pipeline::DspPreset::stt_fast(),
            sine,
            processed.clone(),
        )
        .unwrap();
        assert!(processed.exists());
        assert!(stt.exists());
        let (_, processed_rate) = read_pcm16_wav_with_rate(&processed).unwrap();
        let (_, stt_rate) = read_pcm16_wav_with_rate(&stt).unwrap();
        assert_eq!(processed_rate, SAMPLE_RATE);
        assert_eq!(stt_rate, STT_SAMPLE_RATE);
    }

    #[test]
    fn reprocess_original_writes_separate_dsp_audio_and_preserves_source() {
        let dir = tempfile::tempdir().unwrap();
        let raw = dir.path().join("recording.raw.wav");
        let processed = dir.path().join("recording.processed-new.wav");
        let samples: Vec<f32> = (0..4_800)
            .map(|i| (i as f32 * 440.0 * 2.0 * std::f32::consts::PI / SAMPLE_RATE as f32).sin())
            .collect();
        crate::audio::capture::write_pcm16_wav(&raw, SAMPLE_RATE, &samples).unwrap();

        reprocess_original_audio(
            &raw,
            &processed,
            crate::dsp::pipeline::DspPreset::stt_fast(),
        )
        .unwrap();

        assert!(raw.exists());
        assert!(processed.exists());
        let (_, rate) = read_pcm16_wav_with_rate(&processed).unwrap();
        assert_eq!(rate, SAMPLE_RATE);
    }

    #[test]
    fn persist_attempt_success_finishes_without_deadlock() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = Arc::new(AppContext::initialize(dir.path().to_path_buf()).unwrap());
        let rec = new_recording("openai/gpt-transcribe".into());
        ctx.history.lock().insert(&rec).unwrap();
        let (tx, rx) = std::sync::mpsc::channel();
        let worker = {
            let ctx = Arc::clone(&ctx);
            let rec_id = rec.id.clone();
            std::thread::spawn(move || {
                persist_attempt(
                    &ctx,
                    &rec_id,
                    "openai/gpt-transcribe",
                    1,
                    "success",
                    Some(200),
                    40,
                    None,
                    None,
                )
                .unwrap();
                persist_attempt(
                    &ctx,
                    &rec_id,
                    "openai/gpt-transcribe",
                    2,
                    "error",
                    Some(503),
                    12,
                    Some("retryable".into()),
                    None,
                )
                .unwrap();
                let _ = tx.send(());
            })
        };
        rx.recv_timeout(Duration::from_secs(2))
            .expect("persist_attempt deadlocked");
        worker.join().unwrap();
        let kept = ctx.history.lock().get(&rec.id).unwrap().unwrap();
        assert_eq!(kept.attempt_count, 2);
        assert_eq!(ctx.history.lock().list_attempts(&rec.id).unwrap().len(), 2);
    }

    #[test]
    fn retry_cancellation_restores_original_status_and_keeps_started_attempt() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = AppContext::initialize(dir.path().to_path_buf()).unwrap();
        let mut rec = new_recording("vendor/first-model".into());
        rec.status = RecordingStatus::Completed;
        rec.transcript = Some("previous transcript".into());
        rec.attempt_count = 1;
        rec.processed_audio_path = Some("recording.processed.wav".into());
        ctx.history.lock().insert(&rec).unwrap();
        ctx.history
            .lock()
            .record_attempt(
                &TranscriptionAttempt {
                    id: "retry-first-attempt".into(),
                    recording_id: rec.id.clone(),
                    attempt_number: 1,
                    started_at: rec.created_at,
                    ended_at: Some(rec.created_at),
                    outcome: "success".into(),
                    error_category: None,
                    http_status: Some(200),
                    latency_ms: Some(15),
                },
                Some(0.2),
            )
            .unwrap();

        let mut in_progress = ctx.history.lock().get(&rec.id).unwrap().unwrap();
        in_progress.status = RecordingStatus::Processing;
        in_progress.model = "vendor/reprocess-model".into();
        in_progress.processed_audio_path = Some("recording.processed-staged.wav".into());
        ctx.history.lock().update(&in_progress).unwrap();
        let staged_processed = dir.path().join("recording.processed-staged.wav");
        std::fs::write(&staged_processed, b"staged processed audio").unwrap();
        ctx.history
            .lock()
            .record_attempt(
                &TranscriptionAttempt {
                    id: "retry-started-then-cancelled".into(),
                    recording_id: rec.id.clone(),
                    attempt_number: 2,
                    started_at: rec.created_at + chrono::Duration::seconds(1),
                    ended_at: Some(rec.created_at + chrono::Duration::seconds(2)),
                    outcome: "cancelled".into(),
                    error_category: Some("cancelled".into()),
                    http_status: None,
                    latency_ms: Some(1_000),
                },
                None,
            )
            .unwrap();

        restore_cancelled_retry(&ctx, &rec.id, &rec, Some(&staged_processed)).unwrap();

        let kept = ctx.history.lock().get(&rec.id).unwrap().unwrap();
        assert_eq!(kept.status, RecordingStatus::Completed);
        assert_eq!(kept.model, "vendor/first-model");
        assert_eq!(
            kept.processed_audio_path.as_deref(),
            Some("recording.processed.wav")
        );
        assert!(!staged_processed.exists());
        assert_eq!(kept.transcript.as_deref(), Some("previous transcript"));
        assert_eq!(kept.attempt_count, 2);
        let attempts = ctx.history.lock().list_attempts(&rec.id).unwrap();
        assert_eq!(attempts.len(), 2);
        assert_eq!(attempts[1].outcome, "cancelled");
        let stats = ctx
            .history
            .lock()
            .get_usage_statistics(
                rec.created_at,
                rec.created_at + chrono::Duration::seconds(3),
            )
            .unwrap();
        assert_eq!(stats.api_requests, 2);
        assert_eq!(stats.unpriced_attempts, 1);
        assert!((stats.reported_cost_usd - 0.2).abs() < f64::EPSILON);
    }

    #[test]
    fn empty_or_unreadable_capture_is_preserved_and_recording_is_finalized() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = AppContext::initialize(dir.path().to_path_buf()).unwrap();
        let audio_root = audio_dir(&ctx.data_dir);
        let mut rec = new_recording("vendor/model".into());
        rec.raw_audio_path = Some(format!("{}.raw.wav", rec.id));
        ctx.history.lock().insert(&rec).unwrap();
        *ctx.session_recording_id.lock() = Some(rec.id.clone());
        let wav = audio_root.join(rec.raw_audio_path.as_ref().unwrap());
        crate::audio::capture::write_pcm16_wav(&wav, SAMPLE_RATE, &[]).unwrap();
        std::fs::write(wav.with_extension("wav.tmp"), b"partial").unwrap();

        assert!(read_capture_samples(Vec::new(), &wav).is_err());
        mark_recording_failed(
            &ctx,
            &rec.id,
            &AppError::AudioProcessingFailed("empty capture".into()),
        )
        .unwrap();
        clear_live_session(&ctx, &rec.id);

        let kept = ctx.history.lock().get(&rec.id).unwrap().unwrap();
        assert_eq!(kept.status, RecordingStatus::Failed);
        assert_eq!(
            kept.raw_audio_path.as_deref(),
            Some(rec.raw_audio_path.as_deref().unwrap())
        );
        assert!(ctx.session_recording_id.lock().is_none());
        assert!(wav.exists());
        assert!(wav.with_extension("wav.tmp").exists());
    }

    #[test]
    fn damaged_history_referenced_wav_is_preserved_after_read_failure() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = AppContext::initialize(dir.path().to_path_buf()).unwrap();
        let mut rec = new_recording("vendor/model".into());
        rec.raw_audio_path = Some(format!("{}.raw.wav", rec.id));
        ctx.history.lock().insert(&rec).unwrap();
        let wav = audio_dir(&ctx.data_dir).join(rec.raw_audio_path.as_ref().unwrap());
        let tmp = wav.with_extension("wav.tmp");
        std::fs::write(&wav, b"damaged wave data").unwrap();
        std::fs::write(&tmp, b"temporary output").unwrap();

        assert!(read_capture_samples(Vec::new(), &wav).is_err());
        cleanup_failed_wav_read(&ctx, &rec.id, &wav);

        assert!(
            wav.is_file(),
            "history-referenced source should remain recoverable"
        );
        assert!(!tmp.exists(), "temporary output should be cleaned");
        let persisted = ctx.history.lock().get(&rec.id).unwrap().unwrap();
        assert_eq!(
            persisted.raw_audio_path.as_deref(),
            rec.raw_audio_path.as_deref()
        );
    }

    #[test]
    fn capture_stream_error_persists_failed_row_and_terminal_session_state() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = AppContext::initialize(dir.path().to_path_buf()).unwrap();
        let mut rec = new_recording("vendor/model".into());
        rec.status = RecordingStatus::Processing;
        ctx.history.lock().insert(&rec).unwrap();
        let wav_path = audio_dir(&ctx.data_dir).join(format!("{}.raw.wav", rec.id));
        let result = crate::audio::capture::CaptureResult {
            path: wav_path,
            duration_ms: 125,
            sample_rate: SAMPLE_RATE,
            samples: Vec::new(),
            truncated: false,
            capture_error: Some("device disconnected".into()),
        };

        let filename = result
            .path
            .file_name()
            .unwrap()
            .to_string_lossy()
            .into_owned();
        let err = apply_capture_result_metadata(&mut rec, &result, filename).unwrap();
        ctx.history.lock().update(&rec).unwrap();

        let persisted = ctx.history.lock().get(&rec.id).unwrap().unwrap();
        assert_eq!(persisted.status, RecordingStatus::Failed);
        assert_eq!(persisted.duration_ms, 125);
        assert_eq!(
            persisted.last_error_code.as_deref(),
            Some("AudioCaptureFailed")
        );
        assert_eq!(persisted.raw_audio_path, rec.raw_audio_path);

        ctx.transition(SessionEvent::StartRequested).unwrap();
        ctx.transition(SessionEvent::CaptureReady).unwrap();
        ctx.transition(SessionEvent::StopRequested).unwrap();
        ctx.transition(SessionEvent::Saved).unwrap();
        ctx.transition(SessionEvent::SaveFailed(err)).unwrap();
        assert!(matches!(*ctx.state.lock(), SessionState::Failed { .. }));
    }

    #[test]
    fn failed_attempt_persistence_is_returned_to_caller() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = AppContext::initialize(dir.path().to_path_buf()).unwrap();
        let rec = new_recording("vendor/model".into());
        ctx.history.lock().insert(&rec).unwrap();
        Connection::open(ctx.data_dir.join("history.sqlite"))
            .unwrap()
            .execute_batch(
                "CREATE TRIGGER reject_attempt_insert BEFORE INSERT ON transcription_attempts BEGIN SELECT RAISE(ABORT, 'injected attempt persistence failure'); END;",
            )
            .unwrap();

        let result = persist_attempt(
            &ctx,
            &rec.id,
            "vendor/model",
            1,
            "success",
            Some(200),
            25,
            None,
            Some(0.1),
        );

        assert!(matches!(result, Err(AppError::StorageFailed(_))));
        assert!(ctx
            .history
            .lock()
            .list_attempts(&rec.id)
            .unwrap()
            .is_empty());
    }

    #[test]
    fn retry_attempt_is_attributed_to_the_model_used_for_the_request() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = AppContext::initialize(dir.path().to_path_buf()).unwrap();
        let rec = new_recording("vendor/original-model".into());
        ctx.history.lock().insert(&rec).unwrap();

        persist_attempt(
            &ctx,
            &rec.id,
            "vendor/retry-model",
            1,
            "error",
            Some(503),
            40,
            Some("retryable".into()),
            None,
        )
        .unwrap();

        let stats = ctx
            .history
            .lock()
            .get_usage_statistics(
                rec.created_at,
                rec.created_at + chrono::Duration::seconds(1),
            )
            .unwrap();
        let original_model = stats
            .models
            .iter()
            .find(|model| model.model == "vendor/original-model")
            .unwrap();
        let retried_model = stats
            .models
            .iter()
            .find(|model| model.model == "vendor/retry-model")
            .unwrap();
        assert_eq!(original_model.dictations, 1);
        assert_eq!(original_model.api_requests, 0);
        assert_eq!(retried_model.api_requests, 1);
        assert_eq!(retried_model.unpriced_attempts, 1);
    }

    #[test]
    fn startup_promotes_raw_tmp_to_interrupted() {
        let dir = tempfile::tempdir().unwrap();
        let data = dir.path().to_path_buf();
        let history = HistoryRepo::open(&data.join("history.sqlite")).unwrap();
        let audio = audio_dir(&data);
        std::fs::create_dir_all(&audio).unwrap();
        let mut rec = new_recording("openai/gpt-transcribe".into());
        rec.raw_audio_path = Some(format!("{}.raw.wav", rec.id));
        rec.status = RecordingStatus::Processing;
        let dest = audio.join(rec.raw_audio_path.as_ref().unwrap());
        let tmp = dest.with_extension("wav.tmp");
        crate::audio::capture::write_pcm16_wav(&tmp, SAMPLE_RATE, &[0.0; 160]).unwrap();
        history.insert(&rec).unwrap();
        drop(history);

        let ctx = AppContext::initialize(data).unwrap();
        let kept = ctx.history.lock().get(&rec.id).unwrap().unwrap();
        assert_eq!(kept.status, RecordingStatus::Interrupted);
        assert!(dest.exists());
        assert!(!tmp.exists());
    }

    #[test]
    fn failed_wav_read_preserves_source_when_history_lookup_fails() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = AppContext::initialize(dir.path().to_path_buf()).unwrap();
        let rec = new_recording("vendor/model".into());
        ctx.history.lock().insert(&rec).unwrap();
        let wav = audio_dir(&ctx.data_dir).join(format!("{}.raw.wav", rec.id));
        std::fs::write(&wav, b"recoverable source").unwrap();
        std::fs::write(wav.with_extension("wav.tmp"), b"temporary output").unwrap();
        let fault = Connection::open(ctx.data_dir.join("history.sqlite")).unwrap();
        fault
            .execute_batch("ALTER TABLE recordings RENAME TO unavailable_recordings;")
            .unwrap();

        cleanup_failed_wav_read(&ctx, &rec.id, &wav);

        assert_eq!(std::fs::read(&wav).unwrap(), b"recoverable source");
        assert!(!wav.with_extension("wav.tmp").exists());
        fault
            .execute_batch("ALTER TABLE unavailable_recordings RENAME TO recordings;")
            .unwrap();
        assert!(ctx.history.lock().get(&rec.id).unwrap().is_some());
    }

    #[test]
    fn failed_wav_read_removes_only_unreferenced_output() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = AppContext::initialize(dir.path().to_path_buf()).unwrap();
        let mut rec = new_recording("vendor/model".into());
        rec.raw_audio_path = Some(format!("{}.raw.wav", rec.id));
        ctx.history.lock().insert(&rec).unwrap();
        let audio = audio_dir(&ctx.data_dir);
        let retained = audio.join(rec.raw_audio_path.as_ref().unwrap());
        std::fs::write(&retained, b"original").unwrap();
        for id in [rec.id.clone(), uuid::Uuid::new_v4().to_string()] {
            let wav = audio.join(format!("{id}.unreferenced.wav"));
            std::fs::write(&wav, b"damaged output").unwrap();
            std::fs::write(wav.with_extension("wav.tmp"), b"temporary").unwrap();
            cleanup_failed_wav_read(&ctx, &id, &wav);
            assert!(!wav.exists());
            assert!(!wav.with_extension("wav.tmp").exists());
        }
        assert_eq!(std::fs::read(retained).unwrap(), b"original");
        assert!(ctx.history.lock().get(&rec.id).unwrap().is_some());
    }

    #[test]
    fn cancellation_cleanup_checks_all_artifacts_without_touching_other_recordings() {
        let dir = tempfile::tempdir().unwrap();
        let recording_id = uuid::Uuid::new_v4().to_string();
        let other_id = uuid::Uuid::new_v4().to_string();
        let other = dir.path().join(format!("{other_id}.raw.wav"));
        std::fs::write(&other, b"unrelated source").unwrap();
        for suffix in [
            "raw.wav",
            "processed.wav",
            "raw.stt.wav",
            "processed.stt.wav",
        ] {
            let path = dir.path().join(format!("{recording_id}.{suffix}"));
            std::fs::write(&path, b"discarded").unwrap();
            std::fs::write(path.with_extension("wav.tmp"), b"unfinished").unwrap();
        }
        let pending = dir
            .path()
            .join(format!("{recording_id}.raw.wav.voxely-delete-pending-test"));
        std::fs::write(&pending, b"staged deletion").unwrap();
        assert!(!recording_audio_cleanup_complete(dir.path(), &recording_id));

        cleanup_recording_audio(dir.path(), &recording_id);

        assert!(recording_audio_cleanup_complete(dir.path(), &recording_id));
        assert!(!pending.exists());
        assert_eq!(std::fs::read(&other).unwrap(), b"unrelated source");
        cleanup_recording_audio(dir.path(), "../invalid");
        assert!(!recording_audio_cleanup_complete(dir.path(), "../invalid"));
        assert!(other.is_file());
        assert!(recording_audio_cleanup_complete(
            &dir.path().join("absent"),
            &recording_id
        ));
    }

    #[test]
    fn stale_pipeline_lookup_failure_preserves_all_audio_and_new_session_owner() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = AppContext::initialize(dir.path().to_path_buf()).unwrap();
        let rec = new_recording("vendor/model".into());
        ctx.history.lock().insert(&rec).unwrap();
        let audio = audio_dir(&ctx.data_dir);
        let wav = audio.join(format!("{}.raw.wav", rec.id));
        let upload = audio.join(format!("{}.processed.stt.wav", rec.id));
        std::fs::write(&wav, b"raw").unwrap();
        std::fs::write(&upload, b"upload").unwrap();
        let current_id = uuid::Uuid::new_v4().to_string();
        *ctx.session_recording_id.lock() = Some(current_id.clone());
        let fault = Connection::open(ctx.data_dir.join("history.sqlite")).unwrap();
        fault
            .execute_batch("ALTER TABLE recordings RENAME TO unavailable_recordings;")
            .unwrap();

        cleanup_stale_pipeline_audio(&ctx, &rec.id);
        discard_stale_capture_result(&ctx, Some(&rec.id), &wav);
        discard_stale_capture_failure(&ctx, &rec.id);

        assert!(wav.is_file());
        assert!(upload.is_file());
        assert_eq!(
            ctx.session_recording_id.lock().as_deref(),
            Some(current_id.as_str())
        );
        fault
            .execute_batch("ALTER TABLE unavailable_recordings RENAME TO recordings;")
            .unwrap();
        assert!(ctx.history.lock().get(&rec.id).unwrap().is_some());
    }

    #[test]
    fn failure_metadata_retains_existing_and_partial_audio_but_clears_missing_paths() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = AppContext::initialize(dir.path().to_path_buf()).unwrap();
        let audio = audio_dir(&ctx.data_dir);
        let mut rec = new_recording("vendor/model".into());
        rec.raw_audio_path = Some(format!("{}.raw.wav", rec.id));
        rec.processed_audio_path = Some(format!("{}.processed.wav", rec.id));
        ctx.history.lock().insert(&rec).unwrap();
        let raw = audio.join(rec.raw_audio_path.as_ref().unwrap());
        let processed = audio.join(rec.processed_audio_path.as_ref().unwrap());
        std::fs::write(raw.with_extension("wav.tmp"), b"unfinished capture").unwrap();
        std::fs::write(&processed, b"processed output").unwrap();
        mark_recording_failed(&ctx, &rec.id, &AppError::Interrupted).unwrap();
        let interrupted = ctx.history.lock().get(&rec.id).unwrap().unwrap();
        assert_eq!(interrupted.status, RecordingStatus::Interrupted);
        assert_eq!(interrupted.raw_audio_path, rec.raw_audio_path);
        assert_eq!(interrupted.processed_audio_path, rec.processed_audio_path);
        std::fs::remove_file(raw.with_extension("wav.tmp")).unwrap();
        std::fs::remove_file(&processed).unwrap();

        mark_recording_failed(
            &ctx,
            &rec.id,
            &AppError::AudioCaptureFailed("device failed".into()),
        )
        .unwrap();

        let failed = ctx.history.lock().get(&rec.id).unwrap().unwrap();
        assert_eq!(failed.status, RecordingStatus::Failed);
        assert_eq!(
            failed.last_error_code.as_deref(),
            Some("AudioCaptureFailed")
        );
        assert_eq!(
            failed.last_error_message,
            Some(AppError::AudioCaptureFailed("device failed".into()).user_message())
        );
        assert!(failed.raw_audio_path.is_none());
        assert!(failed.processed_audio_path.is_none());
        mark_recording_failed(&ctx, "already-deleted", &AppError::Interrupted).unwrap();
    }

    #[test]
    fn late_shutdown_capture_failure_preserves_retry_audio_and_releases_live_slot() {
        for native_start_error in [false, true] {
            let dir = tempfile::tempdir().unwrap();
            let ctx = AppContext::initialize(dir.path().to_path_buf()).unwrap();
            let mut rec = new_recording("vendor/model".into());
            rec.raw_audio_path = Some(format!("{}.raw.wav", rec.id));
            ctx.history.lock().insert(&rec).unwrap();
            let wav = audio_dir(&ctx.data_dir).join(rec.raw_audio_path.as_ref().unwrap());
            write_pcm16_wav(&wav, SAMPLE_RATE, &[0.25; 480]).unwrap();
            *ctx.session_recording_id.lock() = Some(rec.id.clone());
            ctx.shutdown_requested.store(true, Ordering::SeqCst);
            let error = AppError::AudioCaptureFailed("late native failure".into());

            if native_start_error {
                finish_stale_capture_start_error(&ctx, &rec.id, &error);
            } else {
                finish_stale_capture_start_result(&ctx, &rec.id, Err(error));
            }

            let persisted = ctx.history.lock().get(&rec.id).unwrap().unwrap();
            assert_eq!(persisted.status, RecordingStatus::Interrupted);
            assert_eq!(persisted.last_error_code.as_deref(), Some("Interrupted"));
            assert_eq!(persisted.raw_audio_path, rec.raw_audio_path);
            assert!(wav.is_file());
            assert!(ctx.session_recording_id.lock().is_none());
        }
    }

    #[test]
    fn cancelled_retry_storage_failure_keeps_staged_audio_until_restore_is_durable() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = AppContext::initialize(dir.path().to_path_buf()).unwrap();
        let mut original = new_recording("vendor/original".into());
        original.status = RecordingStatus::Completed;
        original.transcript = Some("previous result".into());
        ctx.history.lock().insert(&original).unwrap();
        let mut pending = original.clone();
        pending.model = "vendor/retry".into();
        persist_retry_processing(&ctx, &mut pending).unwrap();
        let staged = audio_dir(&ctx.data_dir).join(format!("{}.processed.wav", original.id));
        std::fs::write(&staged, b"retry processed audio").unwrap();
        std::fs::write(staged.with_extension("wav.tmp"), b"partial output").unwrap();
        let fault = Connection::open(ctx.data_dir.join("history.sqlite")).unwrap();
        fault.execute_batch("CREATE TRIGGER reject_retry_restore BEFORE UPDATE ON recordings WHEN NEW.status = 'completed' BEGIN SELECT RAISE(ABORT, 'retry restore failed'); END;").unwrap();

        assert!(matches!(
            restore_cancelled_retry(&ctx, &original.id, &original, Some(&staged)),
            Err(AppError::StorageFailed(_))
        ));
        assert!(staged.is_file());
        assert!(staged.with_extension("wav.tmp").is_file());
        assert_eq!(
            ctx.history
                .lock()
                .get(&original.id)
                .unwrap()
                .unwrap()
                .status,
            RecordingStatus::Processing
        );
        fault
            .execute_batch("DROP TRIGGER reject_retry_restore;")
            .unwrap();
        restore_cancelled_retry(&ctx, &original.id, &original, Some(&staged)).unwrap();
        let restored = ctx.history.lock().get(&original.id).unwrap().unwrap();
        assert_eq!(restored.status, RecordingStatus::Completed);
        assert_eq!(restored.model, original.model);
        assert_eq!(restored.transcript, original.transcript);
        assert!(!staged.exists());
        assert!(!staged.with_extension("wav.tmp").exists());
    }

    #[test]
    fn reprocessing_resamples_source_rate_and_propagates_read_and_write_failures() {
        let dir = tempfile::tempdir().unwrap();
        let raw = dir.path().join("source.wav");
        let processed = dir.path().join("processed.wav");
        write_pcm16_wav(&raw, 16_000, &[0.25; 1_600]).unwrap();
        reprocess_original_audio(
            &raw,
            &processed,
            crate::dsp::pipeline::DspPreset::stt_fast(),
        )
        .unwrap();
        let (samples, rate) = read_pcm16_wav_with_rate(&processed).unwrap();
        assert_eq!(rate, SAMPLE_RATE);
        assert_eq!(samples.len(), 4_800);
        assert_eq!(read_pcm16_wav_with_rate(&raw).unwrap().1, 16_000);
        assert!(reprocess_original_audio(
            &raw,
            &dir.path().join("missing-parent/output.wav"),
            crate::dsp::pipeline::DspPreset::stt_fast()
        )
        .is_err());
        let empty = dir.path().join("empty.wav");
        write_pcm16_wav(&empty, SAMPLE_RATE, &[]).unwrap();
        assert!(matches!(
            reprocess_original_audio(
                &empty,
                &processed,
                crate::dsp::pipeline::DspPreset::stt_fast()
            ),
            Err(AppError::AudioProcessingFailed(_))
        ));
        assert!(reprocess_original_audio(
            &dir.path().join("missing.wav"),
            &processed,
            crate::dsp::pipeline::DspPreset::stt_fast()
        )
        .is_err());
        assert!(prepare_processed_audio(
            crate::dsp::pipeline::DspPreset::stt_fast(),
            vec![0.25; 480],
            dir.path().join("missing-parent/output.wav")
        )
        .is_err());
        assert!(write_stt_pcm(&[0.25; 480], &dir.path().join("absent/output.wav")).is_err());
    }

    #[test]
    fn retry_attempt_offset_preserves_provider_metadata_and_saturates_numbers() {
        assert!(matches!(
            offset_attempt_number(SttProgress::Attempt(3), 4),
            SttProgress::Attempt(7)
        ));
        match offset_attempt_number(
            SttProgress::Waiting {
                attempt: 2,
                delay: Duration::from_millis(700),
            },
            5,
        ) {
            SttProgress::Waiting { attempt, delay } => {
                assert_eq!(attempt, 7);
                assert_eq!(delay, Duration::from_millis(700));
            }
            _ => panic!("waiting progress changed kind"),
        }
        match offset_attempt_number(
            SttProgress::Finished {
                attempt: u32::MAX,
                outcome: "error",
                http_status: Some(429),
                latency_ms: 123,
                category: Some("rate_limit".into()),
                cost_usd: Some(0.125),
            },
            1,
        ) {
            SttProgress::Finished {
                attempt,
                outcome,
                http_status,
                latency_ms,
                category,
                cost_usd,
            } => {
                assert_eq!(attempt, u32::MAX);
                assert_eq!(outcome, "error");
                assert_eq!(http_status, Some(429));
                assert_eq!(latency_ms, 123);
                assert_eq!(category.as_deref(), Some("rate_limit"));
                assert_eq!(cost_usd, Some(0.125));
            }
            _ => panic!("finished progress changed kind"),
        }
    }

    #[test]
    fn shutdown_cancels_every_history_retry_without_consuming_senders() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = AppContext::initialize(dir.path().to_path_buf()).unwrap();
        let (first_tx, first_rx) = tokio::sync::watch::channel(false);
        let (second_tx, second_rx) = tokio::sync::watch::channel(false);
        ctx.retry_cancel.lock().insert("first".into(), first_tx);
        ctx.retry_cancel.lock().insert("second".into(), second_tx);
        cancel_history_retries(&ctx);
        assert!(*first_rx.borrow());
        assert!(*second_rx.borrow());
        assert_eq!(ctx.retry_cancel.lock().len(), 2);
        cancel_history_retries(&ctx);
        assert!(*first_rx.borrow());
        assert!(*second_rx.borrow());
    }

    #[test]
    fn idle_stop_does_not_continue_without_id() {
        assert!(!finish_stop_may_continue(&SessionState::Idle, false));
        assert!(!finish_stop_may_continue(&SessionState::Idle, true));
        assert!(!finish_stop_may_continue(
            &SessionState::StoppingRecording,
            false
        ));
        assert!(finish_stop_may_continue(
            &SessionState::StoppingRecording,
            true
        ));
    }

    #[test]
    fn truncated_capture_auto_stops_only_while_recording() {
        assert!(should_auto_stop_capture(&SessionState::Recording, true));
        assert!(!should_auto_stop_capture(&SessionState::Recording, false));
        assert!(!should_auto_stop_capture(
            &SessionState::StoppingRecording,
            true
        ));
        assert!(!should_auto_stop_capture(&SessionState::Idle, true));
    }

    #[test]
    fn history_retry_cancel_signals_matching_id() {
        let (tx, rx) = tokio::sync::watch::channel(false);
        let senders = Mutex::new(HashMap::from([("rec-1".to_string(), tx)]));
        assert!(signal_retry_cancel(&senders, "rec-1"));
        assert!(*rx.borrow());
        assert!(!signal_retry_cancel(&senders, "missing"));
    }
}
