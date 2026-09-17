use parking_lot::Mutex;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
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
    read_pcm16_wav_with_rate, write_pcm16_wav, CaptureSession, MeterSample,
};
use crate::audio::devices::list_input_devices;
use crate::dsp::metrics::{SAMPLE_RATE, STT_SAMPLE_RATE};
use crate::dsp::pipeline::{prepare_listen_audio, samples_for_stt};
use crate::error::AppError;
use crate::history::repository::{
    audio_dir, can_retry_from_history, new_recording, HistoryRepo, RecordingStatus,
};
use crate::history::retention::{
    apply_retention, cleanup_orphans, cleanup_orphans_except, Retention,
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

pub struct AppContext {
    pub settings_path: PathBuf,
    pub data_dir: PathBuf,
    pub settings: Mutex<AppSettings>,
    pub state: Mutex<SessionState>,
    pub capture: Mutex<Option<CaptureSession>>,
    pub history: Mutex<HistoryRepo>,
    pub captured_target: Mutex<Option<CapturedTarget>>,
    pub overlay: Mutex<OverlayLifecycle>,
    pub in_flight: Mutex<HashSet<String>>,
    pub transport: Mutex<OpenRouterTransport>,
    pub overlay_timeline: Mutex<OverlayTimeline>,
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
    pub meter_monitor: Mutex<Option<CaptureSession>>,
    pub update_gate: tokio::sync::Mutex<bool>,
    pub update_installing: AtomicBool,
    pub settings_recovery: Mutex<bool>,
    pub filter_recording: AtomicBool,
}

impl AppContext {
    pub fn initialize(data_dir: PathBuf) -> Result<Self, AppError> {
        std::fs::create_dir_all(&data_dir).map_err(|e| AppError::StorageFailed(e.to_string()))?;
        let settings_path = data_dir.join("settings.json");
        let (settings, recovered) = AppSettings::load_with_recovery(&settings_path)?;
        let history = HistoryRepo::open(&data_dir.join("history.sqlite"))?;
        let audio = audio_dir(&data_dir);
        std::fs::create_dir_all(&audio).map_err(|e| AppError::StorageFailed(e.to_string()))?;
        recover_stale_processing(&history, &audio)?;
        cleanup_orphans(&audio, &history)?;
        apply_retention(
            &history,
            &audio,
            Retention::from_setting(&settings.retention),
            settings.storage_limit_bytes(),
        )?;
        let transport = OpenRouterTransport::new(settings.retry.to_policy().connect_timeout)?;
        Ok(Self {
            settings_path,
            data_dir,
            settings: Mutex::new(settings),
            state: Mutex::new(SessionState::Idle),
            capture: Mutex::new(None),
            history: Mutex::new(history),
            captured_target: Mutex::new(None),
            overlay: Mutex::new(OverlayLifecycle::default()),
            in_flight: Mutex::new(HashSet::new()),
            transport: Mutex::new(transport),
            overlay_timeline: Mutex::new(OverlayTimeline::default()),
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
            meter_monitor: Mutex::new(None),
            update_gate: tokio::sync::Mutex::new(false),
            update_installing: AtomicBool::new(false),
            settings_recovery: Mutex::new(recovered),
            filter_recording: AtomicBool::new(false),
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
        ToggleHotkeyAction::Cancel => cancel_recording(app),
        ToggleHotkeyAction::Ignore => Ok(()),
    }
}

pub fn cancel_recording(app: &AppHandle) -> Result<(), AppError> {
    let ctx = app.state::<Arc<AppContext>>();
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
            if let Some(session) = ctx.capture.lock().take() {
                thread::spawn(move || {
                    if let Ok(result) = session.stop() {
                        let _ = std::fs::remove_file(result.path);
                    }
                });
            }
            *ctx.session_recording_id.lock() = None;
            ctx.abort_start.store(true, Ordering::SeqCst);
            *ctx.captured_target.lock() = None;
            ctx.emit_state(app);
            play_cancel_cue(&ctx);
            hide_overlay_now(app);
            Ok(())
        }
        SessionState::StoppingRecording
        | SessionState::Saving
        | SessionState::ProcessingAudio
        | SessionState::Transcribing { .. }
        | SessionState::RetryWaiting { .. } => {
            fail_active_recording(&ctx, &AppError::Cancelled);
            ctx.emit_history(app);
            if ctx.transition(SessionEvent::Cancelled).is_err() {
                return Ok(());
            }
            ctx.emit_state(app);
            play_cancel_cue(&ctx);
            hide_overlay_now(app);
            Ok(())
        }
        _ => Ok(()),
    }
}

fn cancel_history_retries(ctx: &AppContext) {
    let senders: Vec<_> = ctx.retry_cancel.lock().values().cloned().collect();
    for tx in senders {
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
    if !ctx.in_flight.lock().is_empty() {
        return Err(AppError::TranscriptionInProgress);
    }
    ctx.abort_start.store(false, Ordering::SeqCst);
    ctx.session_generation.fetch_add(1, Ordering::SeqCst);
    if let Some(preview) = ctx.preview_capture.lock().take() {
        ctx.filter_recording.store(false, Ordering::SeqCst);
        let _ = app.emit_to("main", "filter://sample", false);
        thread::spawn(move || {
            if let Ok(result) = preview.stop() {
                let _ = std::fs::remove_file(result.path);
            }
        });
    }
    crate::app::compare::steal_compare_capture(app);
    release_meter_monitor(&ctx);
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
    *ctx.session_recording_id.lock() = Some(id);
    ctx.emit_history(app);
    let device = if settings.input_device == "default" {
        None
    } else {
        Some(settings.input_device.clone())
    };
    let app_handle = app.clone();
    thread::spawn(move || {
        let started = CaptureSession::start(device.as_deref(), dest);
        let handle = app_handle.clone();
        let _ = app_handle.run_on_main_thread(move || {
            finish_capture_start(&handle, started, generation);
        });
    });
    Ok(())
}

fn finish_capture_start(
    app: &AppHandle,
    started: Result<CaptureSession, AppError>,
    generation: u64,
) {
    let ctx = app.state::<Arc<AppContext>>();
    if !crate::app::operations::lease_matches(
        generation,
        ctx.session_generation.load(Ordering::SeqCst),
    ) || ctx.abort_start.load(Ordering::SeqCst)
    {
        if let Ok(session) = started {
            thread::spawn(move || {
                let _ = session.stop();
            });
        }
        return;
    }
    match started {
        Ok(session) => {
            if session.used_fallback_device() {
                persist_default_microphone(&ctx, app);
            }
            *ctx.capture.lock() = Some(session);
            if ctx.transition(SessionEvent::CaptureReady).is_err() {
                if let Some(session) = ctx.capture.lock().take() {
                    thread::spawn(move || {
                        let _ = session.stop();
                    });
                }
                return;
            }
            ctx.emit_state(app);
            spawn_openrouter_prewarm(&ctx);
            schedule_capture_limit_watch(app, generation);
        }
        Err(err) => {
            tracing::error!(error = %err, "capture start failed");
            if let Some(id) = ctx.session_recording_id.lock().clone() {
                mark_recording_failed(&ctx, &id, &err);
                ctx.emit_history(app);
            }
            let _ = ctx.transition(SessionEvent::CaptureFailed(err.clone()));
            ctx.emit_state(app);
            crate::notify::show_error(app, &err);
            schedule_error_overlay_hide(app.clone(), Duration::from_millis(2000), generation);
        }
    }
}

fn persist_default_microphone(ctx: &AppContext, app: &AppHandle) {
    let mut settings = ctx.settings.lock().clone();
    if settings.input_device == "default" {
        return;
    }
    settings.input_device = "default".into();
    if settings.save(&ctx.settings_path).is_err() {
        return;
    }
    *ctx.settings.lock() = settings.clone();
    let _ = app.emit_to("main", "settings://changed", settings.clone());
    let _ = app.emit_to("overlay", "settings://changed", settings);
    crate::notify::show_error(app, &AppError::MicrophoneUnavailable);
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

fn stop_recording(app: &AppHandle) -> Result<(), AppError> {
    let ctx = app.state::<Arc<AppContext>>();
    let Some(session) = ctx.capture.lock().take() else {
        ctx.abort_start.store(true, Ordering::SeqCst);
        let _ = ctx.transition(SessionEvent::Cancelled);
        ctx.emit_state(app);
        hide_overlay_now(app);
        return Ok(());
    };
    ctx.transition(SessionEvent::StopRequested)?;
    ctx.emit_state(app);
    refresh_insert_target(app);
    if ctx.settings.lock().notifications {
        crate::audio::cue::play_dictation_cue(crate::audio::cue::CueKind::Stop);
    }
    let generation = ctx.session_generation.load(Ordering::SeqCst);
    let app_handle = app.clone();
    thread::spawn(move || match session.stop() {
        Ok(result) => {
            let handle = app_handle.clone();
            let _ = app_handle.run_on_main_thread(move || {
                finish_stop(&handle, result, generation);
            });
        }
        Err(err) => {
            let handle = app_handle.clone();
            let _ = app_handle.run_on_main_thread(move || {
                let ctx = handle.state::<Arc<AppContext>>();
                if !crate::app::operations::lease_matches(
                    generation,
                    ctx.session_generation.load(Ordering::SeqCst),
                ) {
                    return;
                }
                let _ = ctx.transition(SessionEvent::SaveFailed(err.clone()));
                ctx.emit_state(&handle);
                crate::notify::show_error(&handle, &err);
                schedule_error_overlay_hide(handle, Duration::from_millis(2000), generation);
            });
        }
    });
    Ok(())
}

fn finish_stop(app: &AppHandle, result: crate::audio::capture::CaptureResult, generation: u64) {
    let ctx = app.state::<Arc<AppContext>>();
    if !crate::app::operations::lease_matches(
        generation,
        ctx.session_generation.load(Ordering::SeqCst),
    ) {
        return;
    }
    let (can_continue, id) = {
        let state = ctx.state.lock().clone();
        let id = ctx.session_recording_id.lock().clone();
        (finish_stop_may_continue(&state, id.is_some()), id)
    };
    if !can_continue {
        return;
    }
    let Some(id) = id else {
        return;
    };
    if ctx.transition(SessionEvent::Saved).is_err() {
        return;
    }
    ctx.emit_state(app);
    let Ok(Some(mut rec)) = ctx.history.lock().get(&id) else {
        return;
    };
    rec.duration_ms = result.duration_ms as i64;
    rec.raw_audio_path = Some(
        result
            .path
            .file_name()
            .unwrap()
            .to_string_lossy()
            .to_string(),
    );
    rec.updated_at = chrono::Utc::now();
    if result.truncated {
        rec.last_error_code = Some(AppError::RecordingTooLarge.code().into());
        rec.last_error_message = Some("Recording truncated".into());
        ctx.overlay.lock().set_limit_reached(true);
    }
    if ctx.history.lock().update(&rec).ok() != Some(true) {
        let err = AppError::StorageFailed("recording missing".into());
        let _ = ctx.transition(SessionEvent::SaveFailed(err.clone()));
        ctx.emit_state(app);
        crate::notify::show_error(app, &err);
        schedule_error_overlay_hide(app.clone(), Duration::from_millis(2000), generation);
        return;
    }
    ctx.emit_history(app);
    let _ = ctx.transition(SessionEvent::Saved);
    ctx.emit_state(app);
    let app_handle = app.clone();
    let wav_path = result.path.clone();
    let inline_samples = result.samples;
    let recording_id = rec.id.clone();
    tauri::async_runtime::spawn(async move {
        let io_app = app_handle.clone();
        let samples = tokio::task::spawn_blocking(move || {
            let ctx = io_app.state::<Arc<AppContext>>();
            if !crate::app::operations::lease_matches(
                generation,
                ctx.session_generation.load(Ordering::SeqCst),
            ) {
                return Vec::new();
            }
            let _ = apply_configured_retention(ctx.as_ref());
            if inline_samples.is_empty() {
                crate::audio::capture::read_pcm16_wav(&wav_path).unwrap_or_default()
            } else {
                inline_samples
            }
        })
        .await
        .unwrap_or_default();
        let ctx = app_handle.state::<Arc<AppContext>>();
        if !crate::app::operations::lease_matches(
            generation,
            ctx.session_generation.load(Ordering::SeqCst),
        ) {
            return;
        }
        match process_and_transcribe(app_handle.clone(), recording_id.clone(), samples).await {
            Ok(()) => {}
            Err(AppError::Cancelled) => {
                let ctx = app_handle.state::<Arc<AppContext>>();
                mark_recording_failed(&ctx, &recording_id, &AppError::Cancelled);
                ctx.emit_history(&app_handle);
                hide_overlay_for_generation(&app_handle, generation);
            }
            Err(err) => {
                tracing::error!(error = %err, "pipeline failed");
                let ctx = app_handle.state::<Arc<AppContext>>();
                mark_recording_failed(&ctx, &recording_id, &err);
                ctx.emit_history(&app_handle);
                let _ = ctx.transition(SessionEvent::Failed(err.clone()));
                ctx.emit_state(&app_handle);
                crate::notify::show_error(&app_handle, &err);
                schedule_error_overlay_hide(app_handle, Duration::from_millis(2000), generation);
            }
        }
    });
}

async fn process_and_transcribe(
    app: AppHandle,
    recording_id: String,
    samples: Vec<f32>,
) -> Result<(), AppError> {
    let ctx = app.state::<Arc<AppContext>>();
    begin_transcription(&ctx, &recording_id)?;
    let finish = || {
        ctx.in_flight.lock().remove(&recording_id);
    };
    let outcome = process_and_transcribe_inner(&app, &recording_id, samples).await;
    finish();
    outcome
}

async fn process_and_transcribe_inner(
    app: &AppHandle,
    recording_id: &str,
    samples: Vec<f32>,
) -> Result<(), AppError> {
    let ctx = app.state::<Arc<AppContext>>();
    let started_generation = ctx.session_generation.load(Ordering::SeqCst);
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
    let rx = {
        let mut slot = ctx.cancel_tx.lock();
        if let Some(tx) = slot.as_ref() {
            tx.subscribe()
        } else {
            let (tx, rx) = tokio::sync::watch::channel(false);
            *slot = Some(tx);
            rx
        }
    };
    let processed_name = format!("{recording_id}.processed.wav");
    let processed_path = audio_root.join(&processed_name);
    let keep_original = settings.keep_original_recordings;
    let processed_pcm = tokio::task::spawn_blocking({
        let processed_path = processed_path.clone();
        move || prepare_processed_audio(preset, samples, processed_path)
    })
    .await
    .map_err(|e| AppError::AudioProcessingFailed(e.to_string()))??;
    if *rx.borrow()
        || !may_commit_session(
            started_generation,
            ctx.session_generation.load(Ordering::SeqCst),
            false,
        )
    {
        return Err(AppError::Cancelled);
    }
    let mut rec = rec;
    rec.processed_audio_path = Some(processed_name);
    rec.updated_at = chrono::Utc::now();
    apply_raw_retention(&mut rec, keep_original, &raw_path);
    ctx.history.lock().update(&rec)?;
    ctx.emit_history(app);
    let _ = ctx.transition(SessionEvent::Processed);
    ctx.emit_state(app);

    let key = match get_api_key()? {
        Some(key) => key,
        None => {
            rec.status = RecordingStatus::Failed;
            rec.last_error_message = Some(AppError::InvalidApiKey.user_message());
            rec.last_error_code = Some("InvalidApiKey".into());
            rec.updated_at = chrono::Utc::now();
            ctx.history.lock().update(&rec)?;
            ctx.emit_history(app);
            return Err(AppError::InvalidApiKey);
        }
    };
    let language = if settings.language == "auto" {
        None
    } else {
        Some(settings.language.as_str())
    };
    rec.request_started_at = Some(chrono::Utc::now());
    ctx.history.lock().update(&rec)?;
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
        rx.clone(),
    )
    .await;
    let _ = std::fs::remove_file(&stt_path);
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
            rec.status = RecordingStatus::Completed;
            rec.transcript = Some(success.text.clone());
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
            publish_stt_success(app, ctx.as_ref(), started_generation, success.text.clone());
            Ok(())
        }
        Err(AppError::Cancelled) => Err(AppError::Cancelled),
        Err(err) => {
            rec.status = RecordingStatus::Failed;
            rec.last_error_message = Some(err.user_message());
            rec.last_error_code = Some(err.code().to_string());
            rec.updated_at = chrono::Utc::now();
            ctx.history.lock().update(&rec)?;
            Err(err)
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SttSuccessStep {
    PersistCompleted,
    EmitHistory,
    HideOverlay,
    PublishIdle,
    SpawnInsert,
}

pub fn live_stt_success_path() -> [SttSuccessStep; 5] {
    [
        SttSuccessStep::PersistCompleted,
        SttSuccessStep::EmitHistory,
        SttSuccessStep::HideOverlay,
        SttSuccessStep::PublishIdle,
        SttSuccessStep::SpawnInsert,
    ]
}

pub fn stt_success_steps() -> [SttSuccessStep; 4] {
    [
        SttSuccessStep::EmitHistory,
        SttSuccessStep::HideOverlay,
        SttSuccessStep::PublishIdle,
        SttSuccessStep::SpawnInsert,
    ]
}

fn publish_stt_success(app: &AppHandle, ctx: &AppContext, started_generation: u64, text: String) {
    debug_assert_eq!(
        stt_success_steps(),
        [
            SttSuccessStep::EmitHistory,
            SttSuccessStep::HideOverlay,
            SttSuccessStep::PublishIdle,
            SttSuccessStep::SpawnInsert,
        ]
    );
    hide_overlay_for_generation(app, started_generation);
    let _ = ctx.transition(SessionEvent::Succeeded);
    let _ = ctx.transition(SessionEvent::Dismiss);
    ctx.emit_state(app);
    *ctx.session_recording_id.lock() = None;
    let mode = ctx.settings.lock().insertion_mode.clone();
    let hotkey = ctx.settings.lock().hotkey.clone();
    let captured = *ctx.captured_target.lock();
    *ctx.captured_target.lock() = None;
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
        let notify = app_clone.clone();
        let _ = app_clone.run_on_main_thread(move || {
            crate::notify::notify_insert_outcome(&notify, outcome);
            if let Some(event) = insert_outcome_event(outcome) {
                let _ = notify.emit("session://insert", event);
            }
        });
    });
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

pub fn shutdown_session(app: &AppHandle) {
    let ctx = app.state::<Arc<AppContext>>();
    ctx.session_generation.fetch_add(1, Ordering::SeqCst);
    ctx.abort_start.store(true, Ordering::SeqCst);
    if let Some(tx) = ctx.cancel_tx.lock().take() {
        let _ = tx.send(true);
    }
    if let Some(session) = ctx.capture.lock().take() {
        thread::spawn(move || {
            let _ = session.stop();
        });
    }
    if let Some(id) = ctx.session_recording_id.lock().clone() {
        mark_recording_failed(&ctx, &id, &AppError::Interrupted);
    }
    *ctx.session_recording_id.lock() = None;
    *ctx.captured_target.lock() = None;
    hide_overlay_now(app);
    let _ = ctx.transition(SessionEvent::Shutdown);
    ctx.emit_state(app);
}

fn recover_stale_processing(
    history: &HistoryRepo,
    audio_root: &std::path::Path,
) -> Result<(), AppError> {
    for rec in history.list_all()? {
        if rec.status != RecordingStatus::Processing {
            continue;
        }
        let mut rec = rec;
        salvage_raw_tmp(audio_root, &mut rec);
        let has_audio = [&rec.raw_audio_path, &rec.processed_audio_path]
            .into_iter()
            .flatten()
            .any(|name| audio_root.join(name).is_file());
        if has_audio {
            rec.status = RecordingStatus::Interrupted;
            rec.last_error_code = Some(AppError::Interrupted.code().into());
            rec.last_error_message = Some(AppError::Interrupted.user_message());
        } else {
            rec.status = RecordingStatus::Failed;
            rec.last_error_code = Some(
                AppError::StorageFailed("audio missing".into())
                    .code()
                    .into(),
            );
            rec.last_error_message =
                Some(AppError::StorageFailed("audio missing".into()).user_message());
        }
        rec.updated_at = chrono::Utc::now();
        history.update(&rec)?;
    }
    Ok(())
}

fn salvage_raw_tmp(audio_root: &Path, rec: &mut crate::history::repository::Recording) {
    let Some(name) = rec.raw_audio_path.clone() else {
        return;
    };
    let dest = audio_root.join(&name);
    if dest.is_file() {
        return;
    }
    let tmp = dest.with_extension("wav.tmp");
    if tmp.is_file() {
        if std::fs::rename(&tmp, &dest).is_ok() {
            return;
        }
        let _ = std::fs::copy(&tmp, &dest);
    }
}

fn begin_transcription(ctx: &AppContext, recording_id: &str) -> Result<(), AppError> {
    if !ctx.in_flight.lock().insert(recording_id.to_string()) {
        return Err(AppError::TranscriptionInProgress);
    }
    Ok(())
}

fn fail_active_recording(ctx: &AppContext, err: &AppError) {
    if let Some(id) = ctx.session_recording_id.lock().clone() {
        mark_recording_failed(ctx, &id, err);
    }
    *ctx.session_recording_id.lock() = None;
}

fn emit_stt_progress(ctx: &AppContext, app: &AppHandle, recording_id: &str, progress: SttProgress) {
    match &progress {
        SttProgress::Finished {
            attempt,
            outcome,
            http_status,
            latency_ms,
            category,
        } => {
            persist_attempt(
                ctx,
                recording_id,
                *attempt,
                outcome,
                *http_status,
                *latency_ms,
                category.clone(),
            );
        }
        SttProgress::Attempt(_) | SttProgress::Waiting { .. } => {}
    }
    if ctx.session_recording_id.lock().as_deref() != Some(recording_id)
        && !ctx.in_flight.lock().contains(recording_id)
    {
        return;
    }
    let event = match progress {
        SttProgress::Attempt(attempt) => SessionEvent::TranscriptAttemptStarted { attempt },
        SttProgress::Waiting { attempt, delay } => SessionEvent::RetryScheduled { attempt, delay },
        SttProgress::Finished { .. } => return,
    };
    if ctx.session_recording_id.lock().as_deref() == Some(recording_id)
        && ctx.transition(event).is_ok()
    {
        ctx.emit_state(app);
    }
}

fn persist_attempt(
    ctx: &AppContext,
    recording_id: &str,
    attempt: u32,
    outcome: &str,
    http_status: Option<u16>,
    latency_ms: u128,
    category: Option<String>,
) {
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
    if let Err(err) = ctx.history.lock().record_attempt(&row) {
        tracing::error!(error = %err, recording_id, "persist transcription attempt failed");
    }
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
    cancel: tokio::sync::watch::Receiver<bool>,
) -> Result<crate::transcription::openrouter::TranscriptionSuccess, AppError> {
    let client = {
        let ctx = app.state::<Arc<AppContext>>();
        ctx.clone_transport_client()?
    };
    let recording_id_owned = recording_id.to_string();
    let app_for_progress = app.clone();
    transcribe_file_with_progress(
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
            emit_stt_progress(&ctx, &app_for_progress, &recording_id_owned, progress);
        },
    )
    .await
}

fn mark_recording_failed(ctx: &AppContext, recording_id: &str, err: &AppError) {
    let history = ctx.history.lock();
    let Ok(Some(mut rec)) = history.get(recording_id) else {
        return;
    };
    rec.status = RecordingStatus::Failed;
    rec.last_error_message = Some(err.user_message());
    rec.last_error_code = Some(err.code().to_string());
    rec.updated_at = chrono::Utc::now();
    let _ = history.update(&rec);
}

pub fn apply_configured_retention(ctx: &AppContext) -> Result<Vec<String>, AppError> {
    let settings = ctx.settings.lock().clone();
    let protected = crate::app::operations::protected_recording_ids(ctx);
    let audio = audio_dir(&ctx.data_dir);
    let names = crate::app::operations::protected_audio_names(ctx);
    let history = ctx.history.lock();
    cleanup_orphans_except(&audio, &history, &names)?;
    crate::history::retention::apply_retention_except(
        &history,
        &audio,
        Retention::from_setting(&settings.retention),
        settings.storage_limit_bytes(),
        &protected,
    )
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

pub fn release_meter_monitor(ctx: &AppContext) {
    if let Some(session) = ctx.meter_monitor.lock().take() {
        thread::spawn(move || session.discard());
    }
}

pub fn start_meter_monitor(ctx: &AppContext) -> Result<(), AppError> {
    if ctx.capture.lock().is_some()
        || ctx.preview_capture.lock().is_some()
        || ctx.compare_capture.lock().is_some()
    {
        return Ok(());
    }
    if ctx.meter_monitor.lock().is_some() {
        return Ok(());
    }
    let settings = ctx.settings.lock().clone();
    let device = if settings.input_device == "default" {
        None
    } else {
        Some(settings.input_device.clone())
    };
    let session = CaptureSession::start_monitor(device.as_deref())?;
    *ctx.meter_monitor.lock() = Some(session);
    Ok(())
}

pub fn devices() -> Result<Vec<crate::audio::devices::InputDeviceInfo>, AppError> {
    list_input_devices().map_err(|_| AppError::MicrophoneUnavailable)
}

pub async fn manual_retry(
    app: AppHandle,
    recording_id: String,
) -> Result<crate::history::repository::Recording, AppError> {
    let ctx = app.state::<Arc<AppContext>>();
    begin_transcription(&ctx, &recording_id)?;
    let result = manual_retry_inner(&app, &recording_id).await;
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
) -> Result<crate::history::repository::Recording, AppError> {
    let ctx = app.state::<Arc<AppContext>>();
    history_retry_allowed(&ctx.state.lock())?;
    let settings = ctx.settings.lock().clone();
    let mut rec = ctx
        .history
        .lock()
        .get(recording_id)?
        .ok_or_else(|| AppError::StorageFailed("recording missing".into()))?;
    let audio_root = audio_dir(&ctx.data_dir);
    if !can_retry_from_history(&rec) {
        return Err(AppError::StorageFailed("audio missing".into()));
    }
    let processed_path = rec
        .processed_audio_path
        .as_ref()
        .or(rec.raw_audio_path.as_ref())
        .map(|name| audio_root.join(name))
        .ok_or_else(|| AppError::StorageFailed("audio missing".into()))?;
    let key = get_api_key()?.ok_or(AppError::InvalidApiKey)?;
    let language = if settings.language == "auto" {
        None
    } else {
        Some(settings.language.as_str())
    };
    rec.status = RecordingStatus::Processing;
    rec.updated_at = chrono::Utc::now();
    ctx.history.lock().update(&rec)?;
    ctx.emit_history(app);
    dismiss_overlay_if_session_idle(app);
    let (tx, rx) = tokio::sync::watch::channel(false);
    ctx.retry_cancel.lock().insert(recording_id.to_string(), tx);
    let stt_path = write_stt_upload(&processed_path)?;
    let transcribed = run_transcription(
        app,
        recording_id,
        &key,
        &settings.model,
        language,
        &stt_path,
        settings.retry.to_policy(),
        Duration::from_millis(rec.duration_ms as u64),
        rx,
    )
    .await;
    let _ = std::fs::remove_file(&stt_path);
    match transcribed {
        Ok(success) => {
            rec.status = RecordingStatus::Completed;
            rec.model = settings.model.clone();
            rec.transcript = Some(success.text);
            rec.attempt_count = success.attempt as i64;
            rec.usage_json = success.usage_json;
            rec.cost = success.cost;
            rec.generation_id = success.generation_id;
            rec.latency_ms = Some(success.latency_ms as i64);
            rec.completed_at = Some(chrono::Utc::now());
            rec.last_error_code = None;
            rec.last_error_message = None;
            rec.updated_at = chrono::Utc::now();
            ctx.history.lock().update(&rec)?;
            ctx.emit_history(app);
            release_retry_session(ctx.inner(), app, recording_id, None);
            Ok(rec)
        }
        Err(err) => {
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
        let _ = std::fs::remove_file(raw_path);
        rec.raw_audio_path = None;
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
    use super::*;
    use crate::history::repository::{new_recording, RecordingStatus};

    #[test]
    fn live_stt_success_notifies_history_ui() {
        assert_eq!(
            live_stt_success_path(),
            [
                SttSuccessStep::PersistCompleted,
                SttSuccessStep::EmitHistory,
                SttSuccessStep::HideOverlay,
                SttSuccessStep::PublishIdle,
                SttSuccessStep::SpawnInsert,
            ]
        );
        assert_eq!(live_stt_success_path()[0], SttSuccessStep::PersistCompleted);
        assert_eq!(stt_success_steps()[0], SttSuccessStep::EmitHistory);
        assert_eq!(stt_success_steps()[3], SttSuccessStep::SpawnInsert);
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
    fn cancel_keeps_audio_and_marks_failed() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = AppContext::initialize(dir.path().to_path_buf()).unwrap();
        let mut rec = new_recording("openai/gpt-transcribe".into());
        rec.processed_audio_path = Some(format!("{}.processed.wav", rec.id));
        let audio_path = audio_dir(&ctx.data_dir).join(rec.processed_audio_path.as_ref().unwrap());
        crate::audio::capture::write_pcm16_wav(&audio_path, 16_000, &[0.0; 160]).unwrap();
        ctx.history.lock().insert(&rec).unwrap();
        *ctx.session_recording_id.lock() = Some(rec.id.clone());
        fail_active_recording(&ctx, &AppError::Cancelled);
        let kept = ctx.history.lock().get(&rec.id).unwrap().unwrap();
        assert_eq!(kept.status, RecordingStatus::Failed);
        assert_eq!(kept.last_error_code.as_deref(), Some("Cancelled"));
        assert!(audio_path.exists());
        assert!(ctx.session_recording_id.lock().is_none());
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
    fn startup_fails_stale_processing_without_audio() {
        let dir = tempfile::tempdir().unwrap();
        let data = dir.path().to_path_buf();
        let history = HistoryRepo::open(&data.join("history.sqlite")).unwrap();
        let mut rec = new_recording("openai/gpt-transcribe".into());
        rec.status = RecordingStatus::Processing;
        rec.raw_audio_path = Some("missing.wav".into());
        history.insert(&rec).unwrap();
        drop(history);

        let ctx = AppContext::initialize(data).unwrap();
        let kept = ctx.history.lock().get(&rec.id).unwrap().unwrap();
        assert_eq!(kept.status, RecordingStatus::Failed);
        assert_eq!(kept.last_error_code.as_deref(), Some("StorageFailed"));
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
                persist_attempt(&ctx, &rec_id, 1, "success", Some(200), 40, None);
                persist_attempt(
                    &ctx,
                    &rec_id,
                    2,
                    "error",
                    Some(503),
                    12,
                    Some("retryable".into()),
                );
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
        crate::audio::capture::write_pcm16_wav(&tmp, 16_000, &[0.0; 160]).unwrap();
        history.insert(&rec).unwrap();
        drop(history);

        let ctx = AppContext::initialize(data).unwrap();
        let kept = ctx.history.lock().get(&rec.id).unwrap().unwrap();
        assert_eq!(kept.status, RecordingStatus::Interrupted);
        assert!(dest.exists());
        assert!(!tmp.exists());
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
