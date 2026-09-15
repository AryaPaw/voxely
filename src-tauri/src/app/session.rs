use parking_lot::Mutex;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use tauri::{
    AppHandle, Emitter, LogicalSize, Manager, PhysicalPosition, Position, Size, WebviewUrl,
    WebviewWindow, WebviewWindowBuilder,
};

use crate::app::machine::{apply_event, may_commit_session, SessionEvent, SessionState};
use crate::app::overlay::{
    overlay_physical_position, OverlayTimeline, WorkArea, OVERLAY_GAP_PX, OVERLAY_HEIGHT,
    OVERLAY_WIDTH,
};
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
use crate::history::retention::{apply_retention, cleanup_orphans, Retention};
use crate::settings::AppSettings;
use crate::transcription::openrouter::{transcribe_file_with_progress, SttProgress};
use crate::windows_int::credentials::get_api_key;
use crate::windows_int::overlay::{work_area_for_cursor, work_area_for_hwnd};
use crate::windows_int::text_injector::{
    insert_should_abort, insert_transcript_now, native, resolve_insert_target, NativeHwnd,
};

pub struct AppContext {
    pub settings_path: PathBuf,
    pub data_dir: PathBuf,
    pub settings: Mutex<AppSettings>,
    pub state: Mutex<SessionState>,
    pub capture: Mutex<Option<CaptureSession>>,
    pub history: Mutex<HistoryRepo>,
    pub captured_hwnd: Mutex<Option<NativeHwnd>>,
    pub in_flight: Mutex<HashSet<String>>,
    pub client: reqwest::Client,
    pub overlay_timeline: Mutex<OverlayTimeline>,
    pub overlay_epoch: Mutex<u64>,
    pub cancel_tx: Mutex<Option<tokio::sync::watch::Sender<bool>>>,
    pub session_recording_id: Mutex<Option<String>>,
    pub hotkeys_suspended: Mutex<bool>,
    pub abort_start: AtomicBool,
    pub session_generation: AtomicU64,
    pub shortcut_sync_generation: AtomicU64,
    pub preview_capture: Mutex<Option<CaptureSession>>,
    pub compare_capture: Mutex<Option<CaptureSession>>,
    pub compare_cancel: Mutex<Option<tokio::sync::watch::Sender<bool>>>,
    pub compare_state: Mutex<crate::app::compare::CompareState>,
    pub compare_running: AtomicBool,
    pub meter_monitor: Mutex<Option<CaptureSession>>,
    pub update_gate: tokio::sync::Mutex<bool>,
}

impl AppContext {
    pub fn initialize(data_dir: PathBuf) -> Result<Self, AppError> {
        std::fs::create_dir_all(&data_dir).map_err(|e| AppError::StorageFailed(e.to_string()))?;
        let settings_path = data_dir.join("settings.json");
        let settings = AppSettings::load(&settings_path)?;
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
        let client = reqwest::Client::builder()
            .use_rustls_tls()
            .build()
            .map_err(|e| AppError::ConnectionFailed(e.to_string()))?;
        Ok(Self {
            settings_path,
            data_dir,
            settings: Mutex::new(settings),
            state: Mutex::new(SessionState::Idle),
            capture: Mutex::new(None),
            history: Mutex::new(history),
            captured_hwnd: Mutex::new(None),
            in_flight: Mutex::new(HashSet::new()),
            client,
            overlay_timeline: Mutex::new(OverlayTimeline::default()),
            overlay_epoch: Mutex::new(0),
            cancel_tx: Mutex::new(None),
            session_recording_id: Mutex::new(None),
            hotkeys_suspended: Mutex::new(false),
            abort_start: AtomicBool::new(false),
            session_generation: AtomicU64::new(0),
            shortcut_sync_generation: AtomicU64::new(0),
            preview_capture: Mutex::new(None),
            compare_capture: Mutex::new(None),
            compare_cancel: Mutex::new(None),
            compare_state: Mutex::new(crate::app::compare::CompareState::default()),
            compare_running: AtomicBool::new(false),
            meter_monitor: Mutex::new(None),
            update_gate: tokio::sync::Mutex::new(false),
        })
    }

    pub fn emit_state(&self, app: &AppHandle) {
        let state = self.state.lock().clone();
        let _ = app.emit("session://state", state);
        crate::app::shortcuts::schedule_sync(app);
    }

    pub fn emit_history(&self, app: &AppHandle) {
        let _ = app.emit("history://changed", ());
    }

    pub fn transition(&self, event: SessionEvent) -> Result<SessionState, AppError> {
        let mut guard = self.state.lock();
        let next = apply_event(guard.clone(), event)
            .map_err(|e| AppError::IllegalTransition(e.to_string()))?;
        *guard = next.clone();
        Ok(next)
    }
}

pub fn toggle_recording(app: &AppHandle) -> Result<(), AppError> {
    let ctx = app.state::<Arc<AppContext>>();
    let state = ctx.state.lock().clone();
    match state {
        SessionState::Idle | SessionState::Failed { .. } | SessionState::Completed => {
            start_recording(app)
        }
        SessionState::Recording | SessionState::StartingRecording => stop_recording(app),
        _ => Ok(()),
    }
}

pub fn cancel_recording(app: &AppHandle) -> Result<(), AppError> {
    let ctx = app.state::<Arc<AppContext>>();
    ctx.session_generation.fetch_add(1, Ordering::SeqCst);
    if let Some(tx) = ctx.cancel_tx.lock().as_ref() {
        let _ = tx.send(true);
    }
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
            ctx.emit_state(app);
            play_cancel_cue(&ctx);
            hide_overlay_later(app.clone(), Duration::from_millis(16));
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
            hide_overlay_later(app.clone(), Duration::from_millis(16));
            Ok(())
        }
        _ => Ok(()),
    }
}

fn play_cancel_cue(ctx: &AppContext) {
    if ctx.settings.lock().notifications {
        crate::audio::cue::play_dictation_cue(crate::audio::cue::CueKind::Cancel);
    }
}

fn start_recording(app: &AppHandle) -> Result<(), AppError> {
    let ctx = app.state::<Arc<AppContext>>();
    ctx.abort_start.store(false, Ordering::SeqCst);
    ctx.session_generation.fetch_add(1, Ordering::SeqCst);
    if let Some(preview) = ctx.preview_capture.lock().take() {
        thread::spawn(move || {
            if let Ok(result) = preview.stop() {
                let _ = std::fs::remove_file(result.path);
            }
        });
    }
    crate::app::compare::steal_compare_capture(&ctx);
    release_meter_monitor(&ctx);
    let (tx, _) = tokio::sync::watch::channel(false);
    *ctx.cancel_tx.lock() = Some(tx);
    ctx.transition(SessionEvent::StartRequested)?;
    let skip = voxely_window_roots(app);
    *ctx.captured_hwnd.lock() = native::capture_target_excluding(&skip);
    ctx.overlay_timeline.lock().begin_show();
    show_overlay(app);
    ctx.emit_state(app);
    let settings = ctx.settings.lock().clone();
    if settings.notifications {
        crate::audio::cue::play_dictation_cue(crate::audio::cue::CueKind::Start);
    }
    let id = uuid::Uuid::new_v4().to_string();
    let dest = audio_dir(&ctx.data_dir).join(format!("{id}.raw.wav"));
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
            finish_capture_start(&handle, started);
        });
    });
    Ok(())
}

fn finish_capture_start(app: &AppHandle, started: Result<CaptureSession, AppError>) {
    let ctx = app.state::<Arc<AppContext>>();
    if ctx.abort_start.load(Ordering::SeqCst) {
        if let Ok(session) = started {
            thread::spawn(move || {
                if let Ok(result) = session.stop() {
                    let _ = std::fs::remove_file(result.path);
                }
            });
        }
        return;
    }
    match started {
        Ok(session) => {
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
        }
        Err(err) => {
            tracing::error!(error = %err, "capture start failed");
            let _ = ctx.transition(SessionEvent::CaptureFailed(err.clone()));
            ctx.emit_state(app);
            crate::notify::show_error(app, &err);
            hide_overlay_later(app.clone(), Duration::from_millis(2000));
        }
    }
}

fn stop_recording(app: &AppHandle) -> Result<(), AppError> {
    let ctx = app.state::<Arc<AppContext>>();
    native::wait_for_keys_up(
        &crate::windows_int::text_injector::hotkey_keys_to_release(&ctx.settings.lock().hotkey),
        Duration::from_millis(300),
    );
    let Some(session) = ctx.capture.lock().take() else {
        ctx.abort_start.store(true, Ordering::SeqCst);
        let _ = ctx.transition(SessionEvent::Cancelled);
        ctx.emit_state(app);
        hide_overlay_now(app);
        return Ok(());
    };
    ctx.transition(SessionEvent::StopRequested)?;
    ctx.emit_state(app);
    if ctx.settings.lock().notifications {
        crate::audio::cue::play_dictation_cue(crate::audio::cue::CueKind::Stop);
    }
    let app_handle = app.clone();
    thread::spawn(move || match session.stop() {
        Ok(result) => {
            let handle = app_handle.clone();
            let _ = app_handle.run_on_main_thread(move || {
                finish_stop(&handle, result);
            });
        }
        Err(err) => {
            let handle = app_handle.clone();
            let _ = app_handle.run_on_main_thread(move || {
                let ctx = handle.state::<Arc<AppContext>>();
                let _ = ctx.transition(SessionEvent::SaveFailed(err.clone()));
                ctx.emit_state(&handle);
                crate::notify::show_error(&handle, &err);
                hide_overlay_later(handle, Duration::from_millis(2000));
            });
        }
    });
    Ok(())
}

fn finish_stop(app: &AppHandle, result: crate::audio::capture::CaptureResult) {
    let ctx = app.state::<Arc<AppContext>>();
    if matches!(*ctx.state.lock(), SessionState::Idle) {
        let _ = std::fs::remove_file(&result.path);
        return;
    }
    if ctx.transition(SessionEvent::Saved).is_err() {
        return;
    }
    ctx.emit_state(app);
    let settings = ctx.settings.lock().clone();
    let mut rec = new_recording(settings.model.clone());
    rec.duration_ms = result.duration_ms as i64;
    rec.raw_audio_path = Some(
        result
            .path
            .file_name()
            .unwrap()
            .to_string_lossy()
            .to_string(),
    );
    *ctx.session_recording_id.lock() = Some(rec.id.clone());
    if ctx.history.lock().insert(&rec).is_err() {
        return;
    }
    ctx.emit_history(app);
    let _ = apply_configured_retention(ctx.as_ref());
    let _ = ctx.transition(SessionEvent::Saved);
    ctx.emit_state(app);
    let app_handle = app.clone();
    let samples = result.samples;
    tauri::async_runtime::spawn(async move {
        match process_and_transcribe(app_handle.clone(), rec.id.clone(), samples).await {
            Ok(()) => {}
            Err(AppError::Cancelled) => {
                let ctx = app_handle.state::<Arc<AppContext>>();
                mark_recording_failed(&ctx, &rec.id, &AppError::Cancelled);
                ctx.emit_history(&app_handle);
                hide_overlay_now(&app_handle);
            }
            Err(err) => {
                tracing::error!(error = %err, "pipeline failed");
                let ctx = app_handle.state::<Arc<AppContext>>();
                mark_recording_failed(&ctx, &rec.id, &err);
                ctx.emit_history(&app_handle);
                let _ = ctx.transition(SessionEvent::Failed(err.clone()));
                ctx.emit_state(&app_handle);
                crate::notify::show_error(&app_handle, &err);
                hide_overlay_later(app_handle, Duration::from_millis(2000));
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
            tracing::info!(stt_http_ms = success.latency_ms, "stt http");
            let _ = ctx.transition(SessionEvent::Succeeded);
            ctx.emit_state(app);
            *ctx.session_recording_id.lock() = None;
            let mode = ctx.settings.lock().insertion_mode.clone();
            let hotkey = ctx.settings.lock().hotkey.clone();
            let start_hwnd = *ctx.captured_hwnd.lock();
            let text = success.text.clone();
            let app_clone = app.clone();
            let commit_generation = started_generation;
            let _ = app.clone().run_on_main_thread(move || {
                let ctx = app_clone.state::<Arc<AppContext>>();
                if !may_commit_session(
                    commit_generation,
                    ctx.session_generation.load(Ordering::SeqCst),
                    false,
                ) {
                    return;
                }
                hide_overlay_now(&app_clone);
                let overlay = overlay_native_hwnd(&app_clone);
                let main = native_hwnd_for_label(&app_clone, "main");
                let skip = voxely_window_roots(&app_clone);
                let _ = ctx.transition(SessionEvent::Dismiss);
                ctx.emit_state(&app_clone);
                let insert_app = app_clone.clone();
                thread::spawn(move || {
                    std::thread::sleep(Duration::from_millis(16));
                    let abort_app = insert_app.clone();
                    let abort = {
                        let abort_app = abort_app.clone();
                        move || {
                            let ctx = abort_app.state::<Arc<AppContext>>();
                            insert_should_abort(
                                commit_generation,
                                ctx.session_generation.load(Ordering::SeqCst),
                            )
                        }
                    };
                    if abort() {
                        return;
                    }
                    let live = native::capture_target_excluding(&skip);
                    let captured = resolve_insert_target(start_hwnd, live, overlay, main);
                    let result =
                        insert_transcript_now(&mode, captured, overlay, &text, abort, &hotkey);
                    let notify = insert_app.clone();
                    let _ = insert_app.run_on_main_thread(move || match result {
                        Ok("copied") => {
                            let _ = notify.emit("session://insert", "copied");
                        }
                        Ok(_) => {}
                        Err(AppError::Cancelled) => {}
                        Err(err) => {
                            crate::notify::show_error(&notify, &err);
                            let _ = notify.emit("session://insert", err.code());
                        }
                    });
                });
            });
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

fn bump_overlay_epoch(ctx: &AppContext) {
    *ctx.overlay_epoch.lock() += 1;
}

fn overlay_work_area(window: &WebviewWindow) -> WorkArea {
    let fallback = WorkArea {
        left: 0,
        top: 0,
        right: 1920,
        bottom: 1040,
    };
    if let Some(ctx) = window.try_state::<Arc<AppContext>>() {
        if let Some(hwnd) = *ctx.captured_hwnd.lock() {
            if let Some(work) = work_area_for_hwnd(hwnd.value as isize) {
                return work;
            }
        }
    }
    work_area_for_cursor().unwrap_or(fallback)
}

fn position_overlay(window: &WebviewWindow) {
    let scale = window.scale_factor().unwrap_or(1.0);
    let width = (OVERLAY_WIDTH * scale).round() as u32;
    let height = (OVERLAY_HEIGHT * scale).round() as u32;
    let gap = (f64::from(OVERLAY_GAP_PX) * scale).round() as i32;
    let work = overlay_work_area(window);
    let (x, y) = overlay_physical_position(work, width, height, gap);
    let _ = window.set_size(Size::Logical(LogicalSize::new(
        OVERLAY_WIDTH,
        OVERLAY_HEIGHT,
    )));
    let _ = window.set_position(Position::Physical(PhysicalPosition::new(x, y)));
}

fn show_overlay(app: &AppHandle) {
    let ctx = app.state::<Arc<AppContext>>();
    bump_overlay_epoch(&ctx);
    ctx.overlay_timeline.lock().mark_window_created();
    if let Some(window) = app.get_webview_window("overlay") {
        position_overlay(&window);
        decorate_overlay(&window);
        let _ = window.show();
        return;
    }
    match build_overlay_window(app, overlay_url()) {
        Ok(window) => {
            position_overlay(&window);
            decorate_overlay(&window);
            let _ = window.show();
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

fn hide_overlay_now(app: &AppHandle) {
    let ctx = app.state::<Arc<AppContext>>();
    bump_overlay_epoch(&ctx);
    if let Some(window) = app.get_webview_window("overlay") {
        let _ = window.hide();
    }
    ctx.overlay_timeline.lock().hide();
}

fn hide_overlay_later(app: AppHandle, delay: Duration) {
    let ctx = app.state::<Arc<AppContext>>();
    let expected = *ctx.overlay_epoch.lock();
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(delay).await;
        let ctx = app.state::<Arc<AppContext>>();
        if *ctx.overlay_epoch.lock() != expected {
            return;
        }
        if let Some(window) = app.get_webview_window("overlay") {
            let _ = window.hide();
        }
        ctx.overlay_timeline.lock().hide();
        let _ = ctx.transition(SessionEvent::Dismiss);
        ctx.emit_state(&app);
    });
}

fn recover_stale_processing(
    history: &HistoryRepo,
    audio_root: &std::path::Path,
) -> Result<(), AppError> {
    for rec in history.list(10_000)? {
        if rec.status != RecordingStatus::Processing {
            continue;
        }
        let has_audio = [&rec.raw_audio_path, &rec.processed_audio_path]
            .into_iter()
            .flatten()
            .any(|name| audio_root.join(name).is_file());
        let mut rec = rec;
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
    if ctx.session_recording_id.lock().as_deref() != Some(recording_id) {
        return;
    }
    let event = match progress {
        SttProgress::Attempt(attempt) => SessionEvent::TranscriptAttemptStarted { attempt },
        SttProgress::Waiting { attempt, delay } => SessionEvent::RetryScheduled { attempt, delay },
    };
    if ctx.transition(event).is_ok() {
        ctx.emit_state(app);
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
    let client = crate::transcription::openrouter::build_stt_client(policy.connect_timeout)?;
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
    let Ok(Some(mut rec)) = ctx.history.lock().get(recording_id) else {
        return;
    };
    rec.status = RecordingStatus::Failed;
    rec.last_error_message = Some(err.user_message());
    rec.last_error_code = Some(err.code().to_string());
    rec.updated_at = chrono::Utc::now();
    let _ = ctx.history.lock().update(&rec);
}

pub fn apply_configured_retention(ctx: &AppContext) -> Result<Vec<String>, AppError> {
    let settings = ctx.settings.lock().clone();
    apply_retention(
        &ctx.history.lock(),
        &audio_dir(&ctx.data_dir),
        Retention::from_setting(&settings.retention),
        settings.storage_limit_bytes(),
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
    let (_tx, rx) = tokio::sync::watch::channel(false);
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
            Ok(rec)
        }
        Err(err) => {
            rec.status = RecordingStatus::Failed;
            rec.last_error_message = Some(err.user_message());
            rec.last_error_code = Some(err.code().to_string());
            rec.updated_at = chrono::Utc::now();
            ctx.history.lock().update(&rec)?;
            ctx.emit_history(app);
            Err(err)
        }
    }
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
}
