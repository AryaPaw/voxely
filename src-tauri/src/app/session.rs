use parking_lot::Mutex;
use std::collections::HashSet;
use std::path::PathBuf;
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
    overlay_physical_position, WorkArea, OVERLAY_GAP_PX, OVERLAY_HEIGHT, OVERLAY_WIDTH,
};
use crate::audio::capture::{read_pcm16_wav, write_pcm16_wav, CaptureSession, MeterSample};
use crate::audio::devices::list_input_devices;
use crate::dsp::pipeline::prepare_transcription;
use crate::error::AppError;
use crate::history::repository::{audio_dir, new_recording, HistoryRepo, RecordingStatus};
use crate::history::retention::{apply_retention, cleanup_orphans, delete_recording, Retention};
use crate::settings::AppSettings;
use crate::transcription::openrouter::transcribe_file;
use crate::windows_int::credentials::get_api_key;
use crate::windows_int::overlay::work_area_for_cursor;
use crate::windows_int::text_injector::{native, NativeHwnd};

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
    pub overlay_shown_at: Mutex<Option<Instant>>,
    pub overlay_epoch: Mutex<u64>,
    pub cancel_tx: Mutex<Option<tokio::sync::watch::Sender<bool>>>,
    pub session_recording_id: Mutex<Option<String>>,
    pub hotkeys_suspended: Mutex<bool>,
    pub abort_start: AtomicBool,
    pub session_generation: AtomicU64,
    pub preview_capture: Mutex<Option<CaptureSession>>,
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
            overlay_shown_at: Mutex::new(None),
            overlay_epoch: Mutex::new(0),
            cancel_tx: Mutex::new(None),
            session_recording_id: Mutex::new(None),
            hotkeys_suspended: Mutex::new(false),
            abort_start: AtomicBool::new(false),
            session_generation: AtomicU64::new(0),
            preview_capture: Mutex::new(None),
            update_gate: tokio::sync::Mutex::new(false),
        })
    }

    pub fn emit_state(&self, app: &AppHandle) {
        let state = self.state.lock().clone();
        let _ = app.emit("session://state", state);
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
            let _ = ctx.transition(SessionEvent::Cancelled);
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
            hide_overlay_later(app.clone(), Duration::from_millis(16));
            Ok(())
        }
        SessionState::StoppingRecording
        | SessionState::Saving
        | SessionState::ProcessingAudio
        | SessionState::Transcribing { .. }
        | SessionState::RetryWaiting { .. } => {
            if let Some(id) = ctx.session_recording_id.lock().clone() {
                if let Ok(Some(rec)) = ctx.history.lock().get(&id) {
                    let root = audio_dir(&ctx.data_dir);
                    let _ = delete_recording(&ctx.history.lock(), &root, &rec);
                }
            }
            *ctx.session_recording_id.lock() = None;
            let _ = ctx.transition(SessionEvent::Cancelled);
            ctx.emit_state(app);
            hide_overlay_later(app.clone(), Duration::from_millis(16));
            Ok(())
        }
        _ => Ok(()),
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
    let (tx, _) = tokio::sync::watch::channel(false);
    *ctx.cancel_tx.lock() = Some(tx);
    ctx.transition(SessionEvent::StartRequested)?;
    *ctx.captured_hwnd.lock() = native::capture_target();
    *ctx.overlay_shown_at.lock() = Some(Instant::now());
    show_overlay(app);
    ctx.emit_state(app);
    let settings = ctx.settings.lock().clone();
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
            hide_overlay_later(app.clone(), Duration::from_millis(2000));
        }
    }
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
    let _ = apply_configured_retention(ctx.as_ref());
    let _ = ctx.transition(SessionEvent::Saved);
    ctx.emit_state(app);
    let app_handle = app.clone();
    tauri::async_runtime::spawn(async move {
        match process_and_transcribe(app_handle.clone(), rec.id.clone()).await {
            Ok(()) => {}
            Err(AppError::Cancelled) => hide_overlay_now(&app_handle),
            Err(err) => {
                tracing::error!(error = %err, "pipeline failed");
                let ctx = app_handle.state::<Arc<AppContext>>();
                mark_recording_failed(&ctx, &rec.id, &err);
                let _ = ctx.transition(SessionEvent::Failed(err));
                ctx.emit_state(&app_handle);
                hide_overlay_later(app_handle, Duration::from_millis(2000));
            }
        }
    });
}

async fn process_and_transcribe(app: AppHandle, recording_id: String) -> Result<(), AppError> {
    let ctx = app.state::<Arc<AppContext>>();
    if !ctx.in_flight.lock().insert(recording_id.clone()) {
        return Err(AppError::RequestValidationFailed(
            "transcription already running".into(),
        ));
    }
    let finish = || {
        ctx.in_flight.lock().remove(&recording_id);
    };
    let outcome = process_and_transcribe_inner(&app, &recording_id).await;
    finish();
    outcome
}

async fn process_and_transcribe_inner(app: &AppHandle, recording_id: &str) -> Result<(), AppError> {
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
    let samples = read_pcm16_wav(&raw_path)?;
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
    let processed = tokio::task::spawn_blocking(move || prepare_transcription(preset, samples))
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
    let (processed, rate) = processed;
    let processed_name = format!("{recording_id}.processed.wav");
    let processed_path = audio_root.join(&processed_name);
    write_pcm16_wav(&processed_path, rate, &processed)?;
    let mut rec = rec;
    rec.processed_audio_path = Some(processed_name);
    rec.updated_at = chrono::Utc::now();
    if !settings.keep_original_recordings {
        let _ = std::fs::remove_file(&raw_path);
        rec.raw_audio_path = None;
    }
    ctx.history.lock().update(&rec)?;
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
    match transcribe_file(
        &ctx.client,
        crate::transcription::openrouter::default_base_url(),
        &key,
        &settings.model,
        language,
        &processed_path,
        settings.retry.to_policy(),
        Duration::from_millis(rec.duration_ms as u64),
        rx.clone(),
    )
    .await
    {
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
            let _ = ctx.transition(SessionEvent::Succeeded);
            ctx.emit_state(app);
            *ctx.session_recording_id.lock() = None;
            let mode = ctx.settings.lock().insertion_mode.clone();
            let captured = *ctx.captured_hwnd.lock();
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
                let _ = ctx.transition(SessionEvent::Dismiss);
                ctx.emit_state(&app_clone);
                insert_transcript_now(&mode, captured, &text);
            });
            Ok(())
        }
        Err(AppError::Cancelled) => Err(AppError::Cancelled),
        Err(err) => {
            rec.status = RecordingStatus::Failed;
            rec.last_error_message = Some(err.user_message());
            rec.last_error_code = Some(format!("{err:?}"));
            rec.updated_at = chrono::Utc::now();
            ctx.history.lock().update(&rec)?;
            Err(err)
        }
    }
}

fn insert_transcript_now(mode: &str, captured: Option<NativeHwnd>, text: &str) {
    std::thread::sleep(std::time::Duration::from_millis(30));
    match mode {
        "clipboard" => {
            if let Err(err) = native::clipboard_paste(text) {
                tracing::warn!(error = %err, "clipboard paste failed");
            }
        }
        "sendinput" => {
            let Some(hwnd) = captured else {
                tracing::warn!("no captured window for insert");
                return;
            };
            if let Err(err) = native::insert_unicode(hwnd, text) {
                tracing::warn!(error = %err, "unicode insert failed");
            }
        }
        _ => {
            let Some(hwnd) = captured else {
                tracing::warn!("no captured window for insert");
                return;
            };
            if let Err(err) = native::insert_into_window(hwnd, text) {
                tracing::warn!(error = %err, "insert into captured window failed");
            }
        }
    }
}

fn bump_overlay_epoch(ctx: &AppContext) {
    *ctx.overlay_epoch.lock() += 1;
}

fn position_overlay(window: &WebviewWindow) {
    let scale = window.scale_factor().unwrap_or(1.0);
    let width = (OVERLAY_WIDTH * scale).round() as u32;
    let height = (OVERLAY_HEIGHT * scale).round() as u32;
    let gap = (f64::from(OVERLAY_GAP_PX) * scale).round() as i32;
    let work = work_area_for_cursor().unwrap_or(WorkArea {
        left: 0,
        top: 0,
        right: 1920,
        bottom: 1040,
    });
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
    if let Some(window) = app.get_webview_window("overlay") {
        position_overlay(&window);
        decorate_overlay(&window);
        let _ = window.show();
        return;
    }
    let builder = WebviewWindowBuilder::new(
        app,
        "overlay",
        WebviewUrl::App("index.html?overlay=1".into()),
    )
    .title("Voxely Overlay")
    .decorations(false)
    .always_on_top(true)
    .skip_taskbar(true)
    .focused(false)
    .visible(false)
    .resizable(false)
    .transparent(true)
    .shadow(false)
    .inner_size(OVERLAY_WIDTH, OVERLAY_HEIGHT);
    if let Ok(window) = builder.build() {
        position_overlay(&window);
        decorate_overlay(&window);
        let _ = window.show();
    }
}

fn decorate_overlay(window: &WebviewWindow) {
    let _ = window.set_ignore_cursor_events(false);
    #[cfg(windows)]
    if let Ok(hwnd) = window.hwnd() {
        crate::windows_int::overlay::apply_overlay_exstyle(hwnd.0 as isize);
    }
}

fn hide_overlay_now(app: &AppHandle) {
    let ctx = app.state::<Arc<AppContext>>();
    bump_overlay_epoch(&ctx);
    if let Some(window) = app.get_webview_window("overlay") {
        let _ = window.hide();
    }
    *ctx.overlay_shown_at.lock() = None;
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
        *ctx.overlay_shown_at.lock() = None;
        let _ = ctx.transition(SessionEvent::Dismiss);
        ctx.emit_state(&app);
    });
}

fn mark_recording_failed(ctx: &AppContext, recording_id: &str, err: &AppError) {
    let Ok(Some(mut rec)) = ctx.history.lock().get(recording_id) else {
        return;
    };
    rec.status = RecordingStatus::Failed;
    rec.last_error_message = Some(err.user_message());
    rec.last_error_code = Some(format!("{err:?}"));
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
    let preview = ctx
        .preview_capture
        .lock()
        .as_ref()
        .map(CaptureSession::meter)
        .unwrap_or_else(MeterSample::silent);
    preview
}

pub fn devices() -> Result<Vec<crate::audio::devices::InputDeviceInfo>, AppError> {
    list_input_devices().map_err(|_| AppError::MicrophoneUnavailable)
}

pub async fn manual_retry(
    app: AppHandle,
    recording_id: String,
) -> Result<crate::history::repository::Recording, AppError> {
    let ctx = app.state::<Arc<AppContext>>();
    if !ctx.in_flight.lock().insert(recording_id.clone()) {
        return Err(AppError::RequestValidationFailed(
            "transcription already running".into(),
        ));
    }
    let result = manual_retry_inner(&app, &recording_id).await;
    ctx.in_flight.lock().remove(&recording_id);
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
    match transcribe_file(
        &ctx.client,
        crate::transcription::openrouter::default_base_url(),
        &key,
        &settings.model,
        language,
        &processed_path,
        settings.retry.to_policy(),
        Duration::from_millis(rec.duration_ms as u64),
        rx,
    )
    .await
    {
        Ok(success) => {
            rec.status = RecordingStatus::Completed;
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
            Ok(rec)
        }
        Err(err) => {
            rec.status = RecordingStatus::Failed;
            rec.last_error_message = Some(err.user_message());
            rec.last_error_code = Some(format!("{err:?}"));
            rec.updated_at = chrono::Utc::now();
            ctx.history.lock().update(&rec)?;
            Err(err)
        }
    }
}
