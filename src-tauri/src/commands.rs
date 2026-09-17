use crate::app::lifecycle::configure_tray;
use crate::app::machine::{is_cancellable, SessionState};
use crate::app::overlay::OverlayTimingReport;
use crate::app::overlay_controller::OverlaySnapshot;
use crate::app::session::{
    current_meter, devices, release_meter_monitor, start_meter_monitor, AppContext,
};
use crate::audio::capture::{read_pcm16_wav, write_pcm16_wav, CaptureSession};
use crate::audio::devices::InputDeviceInfo;
use crate::dsp::metrics::SAMPLE_RATE;
use crate::dsp::pipeline::prepare_listen_preview;
use crate::error::AppError;
use crate::history::repository::Recording;
use crate::history::retention::delete_recording;
use crate::settings::AppSettings;
use crate::transcription::openrouter::{list_transcription_models, SttModel};
use crate::windows_int::credentials::{delete_api_key, has_api_key, set_api_key};
use crate::windows_int::text_injector::native;
use std::path::Path;
use std::sync::Arc;
use tauri::{AppHandle, Emitter, State};

#[tauri::command]
pub fn get_session_state(ctx: State<'_, Arc<AppContext>>) -> SessionState {
    ctx.state.lock().clone()
}

#[tauri::command]
pub fn get_overlay_snapshot(ctx: State<'_, Arc<AppContext>>) -> OverlaySnapshot {
    ctx.overlay_snapshot()
}

#[tauri::command]
pub fn get_settings(ctx: State<'_, Arc<AppContext>>) -> AppSettings {
    ctx.settings.lock().clone()
}

#[tauri::command]
pub fn save_settings(
    app: AppHandle,
    ctx: State<'_, Arc<AppContext>>,
    settings: AppSettings,
) -> Result<AppSettings, AppError> {
    persist_settings(&app, ctx.inner().as_ref(), settings)
}

fn persist_settings(
    app: &AppHandle,
    ctx: &AppContext,
    settings: AppSettings,
) -> Result<AppSettings, AppError> {
    persist_settings_inner(app, ctx, settings, true)
}

fn persist_settings_inner(
    app: &AppHandle,
    ctx: &AppContext,
    mut settings: AppSettings,
    run_retention: bool,
) -> Result<AppSettings, AppError> {
    settings.retry.validate()?;
    settings.mic_tune.validate()?;
    validate_settings_graph(&settings)?;
    let previous = ctx.settings.lock().clone();
    if settings.write_seq != 0 && settings.write_seq < previous.write_seq {
        return Ok(previous);
    }
    if settings.hotkey != previous.hotkey {
        crate::app::shortcuts::parse_hotkey(&settings.hotkey)?;
    }
    let hotkey_changed = settings.hotkey != previous.hotkey;
    settings.write_seq = previous.write_seq.saturating_add(1);
    settings.save(&ctx.settings_path)?;
    *ctx.settings.lock() = settings.clone();
    *ctx.settings_recovery.lock() = false;
    if hotkey_changed {
        if let Err(err) = crate::app::shortcuts::sync_shortcuts(app) {
            let _ = previous.save(&ctx.settings_path);
            *ctx.settings.lock() = previous.clone();
            let _ = crate::app::shortcuts::sync_shortcuts(app);
            return Err(err);
        }
    }
    if let Err(err) = crate::app::lifecycle::sync_autostart(app, settings.start_with_windows) {
        settings.start_with_windows = previous.start_with_windows;
        let _ = settings.save(&ctx.settings_path);
        *ctx.settings.lock() = settings.clone();
        return Err(err);
    }
    let _ = app.emit_to("main", "settings://changed", settings.clone());
    let _ = app.emit_to("overlay", "settings://changed", settings.clone());
    if run_retention {
        let _ = crate::app::session::apply_configured_retention(ctx);
    }
    let _ = configure_tray(app);
    ctx.transport
        .lock()
        .sync(settings.retry.to_policy().connect_timeout)?;
    if settings.debug_logging != previous.debug_logging {
        crate::logging::apply_debug_logging(settings.debug_logging);
    }
    Ok(settings)
}

fn validate_settings_graph(settings: &AppSettings) -> Result<(), AppError> {
    if settings.model.trim().is_empty() {
        return Err(AppError::RequestValidationFailed("model required".into()));
    }
    if !settings
        .presets
        .iter()
        .any(|preset| preset.id == settings.active_preset_id)
    {
        return Err(AppError::RequestValidationFailed(
            "active preset is missing".into(),
        ));
    }
    for preset in &settings.presets {
        let mut kinds = std::collections::HashSet::new();
        for slot in &preset.order {
            if !kinds.insert(slot.kind) {
                return Err(AppError::RequestValidationFailed(
                    "preset has duplicate filter kinds".into(),
                ));
            }
        }
    }
    if !matches!(settings.insertion_mode.as_str(), "unicode" | "clipboard") {
        return Err(AppError::RequestValidationFailed(
            "insertion mode must be unicode or clipboard".into(),
        ));
    }
    Ok(())
}

#[tauri::command]
pub fn list_history(ctx: State<'_, Arc<AppContext>>) -> Result<Vec<Recording>, AppError> {
    ctx.history.lock().list(500)
}

#[tauri::command]
pub fn list_history_summaries(
    ctx: State<'_, Arc<AppContext>>,
    cursor: Option<String>,
    limit: Option<i64>,
    query: Option<String>,
) -> Result<crate::history::repository::HistorySummaryPage, AppError> {
    ctx.history
        .lock()
        .list_summaries_page(cursor.as_deref(), limit.unwrap_or(50), query.as_deref())
}

#[tauri::command]
pub fn search_history(
    ctx: State<'_, Arc<AppContext>>,
    query: String,
    cursor: Option<String>,
    limit: Option<i64>,
) -> Result<crate::history::repository::HistorySummaryPage, AppError> {
    ctx.history.lock().list_summaries_page(
        cursor.as_deref(),
        limit.unwrap_or(50),
        Some(query.as_str()),
    )
}

#[tauri::command]
pub fn get_recording(
    ctx: State<'_, Arc<AppContext>>,
    id: String,
) -> Result<Option<Recording>, AppError> {
    ctx.history.lock().get(&id)
}

#[tauri::command]
pub fn delete_history_item(
    app: AppHandle,
    ctx: State<'_, Arc<AppContext>>,
    id: String,
) -> Result<(), AppError> {
    let rec = ctx
        .history
        .lock()
        .get(&id)?
        .ok_or_else(|| AppError::StorageFailed("not found".into()))?;
    if crate::app::operations::protected_recording_ids(ctx.inner()).contains(&id)
        && !crate::app::operations::user_may_delete_recording(&rec.status, true)
    {
        return Err(AppError::TranscriptionInProgress);
    }
    ctx.in_flight.lock().remove(&id);
    if !is_cancellable(&ctx.state.lock())
        && ctx.session_recording_id.lock().as_deref() == Some(id.as_str())
    {
        *ctx.session_recording_id.lock() = None;
    }
    let root = crate::history::repository::audio_dir(&ctx.data_dir);
    delete_recording(&ctx.history.lock(), &root, &rec)?;
    ctx.emit_history(&app);
    Ok(())
}

#[tauri::command]
pub fn delete_all_history(
    app: AppHandle,
    ctx: State<'_, Arc<AppContext>>,
) -> Result<crate::history::retention::DeleteAllResult, AppError> {
    let root = crate::history::repository::audio_dir(&ctx.data_dir);
    let protected = crate::app::operations::protected_recording_ids(ctx.inner());
    let result =
        crate::history::retention::delete_all_recordings(&ctx.history.lock(), &root, &protected)?;
    ctx.emit_history(&app);
    Ok(result)
}

#[tauri::command]
pub fn list_microphones(ctx: State<'_, Arc<AppContext>>) -> Result<Vec<InputDeviceInfo>, AppError> {
    let mut list = devices()?;
    let selected = ctx.settings.lock().input_device.clone();
    crate::audio::devices::annotate_missing_selection(&mut list, &selected);
    Ok(list)
}

#[tauri::command]
pub fn get_meter(app: AppHandle) -> crate::audio::capture::MeterSample {
    current_meter(&app)
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DspPreview {
    pub original_path: String,
    pub processed_path: String,
    pub peak: f32,
    pub rms: f32,
    pub clip_count: u32,
    pub nonce: u64,
}

fn preview_from_original(ctx: &AppContext, original: &Path) -> Result<DspPreview, AppError> {
    let samples = read_pcm16_wav(original)?;
    let preset = ctx.settings.lock().active_preset();
    let (original_listen, processed, metrics, _) = prepare_listen_preview(preset, samples)?;
    let audio_root = crate::history::repository::audio_dir(&ctx.data_dir);
    let original_preview = audio_root.join("filter-preview-original.wav");
    let processed_path = audio_root.join("filter-preview.wav");
    write_pcm16_wav(&original_preview, SAMPLE_RATE, &original_listen)?;
    write_pcm16_wav(&processed_path, SAMPLE_RATE, &processed)?;
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|value| value.as_millis() as u64)
        .unwrap_or(0);
    Ok(DspPreview {
        original_path: original_preview.to_string_lossy().into_owned(),
        processed_path: processed_path.to_string_lossy().into_owned(),
        peak: metrics.peak,
        rms: metrics.rms,
        clip_count: metrics.clip_count,
        nonce,
    })
}

#[tauri::command]
pub async fn preview_dsp(ctx: State<'_, Arc<AppContext>>) -> Result<DspPreview, AppError> {
    let ctx = ctx.inner().clone();
    tokio::task::spawn_blocking(move || {
        ctx.settings.lock().mic_tune.validate()?;
        let audio_root = crate::history::repository::audio_dir(&ctx.data_dir);
        let original = audio_root.join("filter-sample.wav");
        if !original.exists() {
            return Err(AppError::StorageFailed("no filter sample".into()));
        }
        preview_from_original(&ctx, &original)
    })
    .await
    .map_err(|e| AppError::AudioProcessingFailed(e.to_string()))?
}

#[tauri::command]
pub async fn start_filter_sample(
    app: AppHandle,
    ctx: State<'_, Arc<AppContext>>,
) -> Result<(), AppError> {
    if is_cancellable(&ctx.state.lock()) {
        return Err(AppError::IllegalTransition(
            "dictation is already running".into(),
        ));
    }
    if ctx.compare_capture.lock().is_some() {
        return Err(AppError::IllegalTransition(
            "compare is already recording".into(),
        ));
    }
    release_meter_monitor(ctx.inner());
    if ctx.preview_capture.lock().is_some() {
        let _ = app.emit_to("main", "filter://sample", true);
        return Ok(());
    }
    ctx.settings.lock().mic_tune.validate()?;
    let dest = crate::history::repository::audio_dir(&ctx.data_dir).join("filter-sample.wav");
    let settings = ctx.settings.lock().clone();
    let device = if settings.input_device == "default" {
        None
    } else {
        Some(settings.input_device.clone())
    };
    let ctx = ctx.inner().clone();
    tokio::task::spawn_blocking(move || {
        let session = CaptureSession::start(device.as_deref(), dest)?;
        *ctx.preview_capture.lock() = Some(session);
        ctx.filter_recording
            .store(true, std::sync::atomic::Ordering::SeqCst);
        Ok(())
    })
    .await
    .map_err(|e| AppError::AudioCaptureFailed(e.to_string()))??;
    let _ = app.emit_to("main", "filter://sample", true);
    Ok(())
}

#[tauri::command]
pub async fn stop_filter_sample(
    app: AppHandle,
    ctx: State<'_, Arc<AppContext>>,
) -> Result<DspPreview, AppError> {
    let ctx = ctx.inner().clone();
    let preview = tokio::task::spawn_blocking(move || {
        ctx.filter_recording
            .store(false, std::sync::atomic::Ordering::SeqCst);
        let session = ctx
            .preview_capture
            .lock()
            .take()
            .ok_or_else(|| AppError::AudioCaptureFailed("sample not started".into()))?;
        let result = session.stop()?;
        preview_from_original(&ctx, &result.path)
    })
    .await
    .map_err(|e| AppError::AudioProcessingFailed(e.to_string()))?;
    let _ = app.emit_to("main", "filter://sample", false);
    preview
}

#[tauri::command]
pub async fn start_input_meter(ctx: State<'_, Arc<AppContext>>) -> Result<(), AppError> {
    let ctx = ctx.inner().clone();
    tokio::task::spawn_blocking(move || start_meter_monitor(&ctx))
        .await
        .map_err(|e| AppError::AudioCaptureFailed(e.to_string()))?
}

#[tauri::command]
pub fn stop_input_meter(ctx: State<'_, Arc<AppContext>>) -> Result<(), AppError> {
    release_meter_monitor(ctx.inner());
    Ok(())
}

#[tauri::command]
pub async fn check_for_updates(app: AppHandle) -> Result<String, AppError> {
    let outcome = crate::updates::check_updates(&app, true).await;
    Ok(outcome.as_str().into())
}

#[tauri::command]
pub async fn install_update(app: AppHandle) -> Result<String, AppError> {
    let outcome = crate::updates::install_available_update(&app).await;
    Ok(outcome.as_str().into())
}

#[tauri::command]
pub fn show_system_notification(
    app: AppHandle,
    title: String,
    body: String,
) -> Result<(), AppError> {
    crate::notify::show_if_enabled(&app, &title, &body)
}

#[tauri::command]
pub fn api_key_configured() -> bool {
    has_api_key()
}

#[tauri::command]
pub fn store_api_key(key: String) -> Result<bool, AppError> {
    if key.trim().len() < 8 {
        return Err(AppError::InvalidApiKey);
    }
    set_api_key(&key)?;
    Ok(true)
}

#[tauri::command]
pub async fn test_openrouter(ctx: State<'_, Arc<AppContext>>) -> Result<u32, AppError> {
    let key = crate::windows_int::credentials::get_api_key()?.ok_or(AppError::InvalidApiKey)?;
    let models = list_transcription_models(&ctx.clone_transport_client()?, &key).await?;
    Ok(models.len() as u32)
}

#[tauri::command]
pub async fn discover_models(ctx: State<'_, Arc<AppContext>>) -> Result<Vec<SttModel>, AppError> {
    let key = crate::windows_int::credentials::get_api_key()?.ok_or(AppError::InvalidApiKey)?;
    list_transcription_models(&ctx.clone_transport_client()?, &key).await
}

#[tauri::command]
pub fn toggle_dictation(app: AppHandle) -> Result<(), AppError> {
    crate::app::shortcuts::toggle_dictation(&app)
}

#[tauri::command]
pub fn cancel_dictation(app: AppHandle) -> Result<(), AppError> {
    crate::app::shortcuts::cancel_dictation(&app)
}

#[tauri::command]
pub fn set_hotkey_capture(app: AppHandle, capturing: bool) -> Result<(), AppError> {
    crate::app::shortcuts::set_hotkeys_suspended(&app, capturing)
}

#[tauri::command]
pub async fn retry_recording(app: AppHandle, id: String) -> Result<Recording, AppError> {
    crate::app::session::manual_retry(app, id).await
}

#[tauri::command]
pub fn cancel_history_retry(app: AppHandle, id: String) -> Result<(), AppError> {
    crate::app::session::cancel_history_retry(&app, &id)
}

#[tauri::command]
pub fn open_logs(ctx: State<'_, Arc<AppContext>>) -> Result<(), AppError> {
    let dir = ctx.data_dir.join("logs");
    std::fs::create_dir_all(&dir).map_err(|e| AppError::StorageFailed(e.to_string()))?;
    tauri_plugin_opener::open_path(dir, None::<&str>)
        .map_err(|e| AppError::StorageFailed(e.to_string()))
}

#[tauri::command]
pub fn open_settings_dir(ctx: State<'_, Arc<AppContext>>) -> Result<(), AppError> {
    let dir = ctx
        .settings_path
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| ctx.data_dir.clone());
    std::fs::create_dir_all(&dir).map_err(|e| AppError::StorageFailed(e.to_string()))?;
    tauri_plugin_opener::open_path(dir, None::<&str>)
        .map_err(|e| AppError::StorageFailed(e.to_string()))
}

#[tauri::command]
pub fn reset_settings(
    app: AppHandle,
    ctx: State<'_, Arc<AppContext>>,
    wipe_api_key: bool,
) -> Result<AppSettings, AppError> {
    if wipe_api_key {
        delete_api_key()?;
    }
    let keep_first_run = !wipe_api_key && ctx.settings.lock().first_run_complete;
    let current = ctx.settings.lock().clone();
    let mut reset = AppSettings::reset_user_settings(keep_first_run);
    reset.retention = current.retention;
    reset.storage_limit = current.storage_limit;
    persist_settings_inner(&app, ctx.inner().as_ref(), reset, false)
}

#[tauri::command]
pub fn open_audio_dir(ctx: State<'_, Arc<AppContext>>) -> Result<(), AppError> {
    let dir = crate::history::repository::audio_dir(&ctx.data_dir);
    std::fs::create_dir_all(&dir).map_err(|e| AppError::StorageFailed(e.to_string()))?;
    tauri_plugin_opener::open_path(dir, None::<&str>)
        .map_err(|e| AppError::StorageFailed(e.to_string()))
}

#[tauri::command]
pub fn copy_transcript(text: String) -> Result<(), AppError> {
    native::clipboard_copy(&text)
}

#[tauri::command]
pub fn open_github(page: Option<String>) -> Result<(), AppError> {
    tauri_plugin_opener::open_url(
        crate::app::lifecycle::github_page_url(page.as_deref()),
        None::<&str>,
    )
    .map_err(|e| AppError::StorageFailed(e.to_string()))
}

#[tauri::command]
pub fn run_retention(ctx: State<'_, Arc<AppContext>>) -> Result<Vec<String>, AppError> {
    crate::app::session::apply_configured_retention(ctx.as_ref())
}

#[tauri::command]
pub fn overlay_timing(ctx: State<'_, Arc<AppContext>>) -> Option<u128> {
    ctx.overlay_timeline.lock().elapsed_ms()
}

#[tauri::command]
pub fn overlay_timeline(ctx: State<'_, Arc<AppContext>>) -> OverlayTimingReport {
    ctx.overlay_timeline.lock().report()
}

#[tauri::command]
pub fn overlay_mark_frame(ctx: State<'_, Arc<AppContext>>, phase: String) {
    ctx.overlay_timeline.lock().mark_phase(&phase);
}

#[tauri::command]
pub fn recording_audio_url(
    ctx: State<'_, Arc<AppContext>>,
    id: String,
) -> Result<Option<String>, AppError> {
    let rec = ctx
        .history
        .lock()
        .get(&id)?
        .ok_or_else(|| AppError::StorageFailed("not found".into()))?;
    let Some(name) = crate::history::repository::history_play_name(
        &rec,
        ctx.settings.lock().keep_original_recordings,
    ) else {
        return Ok(None);
    };
    if name.contains("..") || Path::new(name).is_absolute() {
        return Err(AppError::StorageFailed("invalid audio path".into()));
    }
    let root = crate::history::repository::audio_dir(&ctx.data_dir);
    let path = root.join(name);
    if !path.is_file() {
        return Ok(None);
    }
    let canonical = path
        .canonicalize()
        .map_err(|e| AppError::StorageFailed(e.to_string()))?;
    let root_canonical = root
        .canonicalize()
        .map_err(|e| AppError::StorageFailed(e.to_string()))?;
    if !canonical.starts_with(&root_canonical) {
        return Err(AppError::StorageFailed("audio path escaped".into()));
    }
    if !canonical.exists() {
        return Ok(None);
    }
    Ok(Some(canonical.to_string_lossy().to_string()))
}

#[tauri::command]
pub fn start_model_compare(app: AppHandle) -> Result<(), AppError> {
    crate::app::compare::start_model_compare(&app)
}

#[tauri::command]
pub fn stop_model_compare(app: AppHandle) -> Result<crate::app::compare::CompareState, AppError> {
    crate::app::compare::stop_model_compare(&app)
}

#[tauri::command]
pub async fn run_model_compare(
    app: AppHandle,
) -> Result<crate::app::compare::CompareState, AppError> {
    crate::app::compare::run_model_compare(app).await
}

#[tauri::command]
pub fn get_model_compare(app: AppHandle) -> crate::app::compare::CompareState {
    crate::app::compare::get_model_compare(&app)
}

#[tauri::command]
pub fn clear_model_compare(app: AppHandle) -> Result<(), AppError> {
    crate::app::compare::clear_model_compare(&app)
}

#[tauri::command]
pub fn cancel_model_compare(app: AppHandle) -> Result<crate::app::compare::CompareState, AppError> {
    crate::app::compare::cancel_model_compare(&app)
}

#[tauri::command]
pub fn get_runtime_info(ctx: State<'_, Arc<AppContext>>) -> crate::app::lifecycle::RuntimeInfo {
    crate::app::lifecycle::runtime_info_with_recovery(*ctx.settings_recovery.lock())
}

#[tauri::command]
pub fn play_cue(kind: String) -> Result<(), AppError> {
    if !crate::app::lifecycle::is_local_build() {
        return Err(AppError::RequestValidationFailed("debug only".into()));
    }
    crate::audio::cue::play_dictation_cue(crate::audio::cue::parse_cue_kind(&kind)?);
    Ok(())
}

#[tauri::command]
pub fn preview_error_notification(app: AppHandle) -> Result<(), AppError> {
    if !crate::app::lifecycle::is_local_build() {
        return Err(AppError::RequestValidationFailed("debug only".into()));
    }
    crate::notify::show_preview(&app)
}
