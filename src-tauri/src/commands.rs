use crate::app::lifecycle::configure_tray;
use crate::app::machine::{is_cancellable, SessionState};
use crate::app::session::{current_meter, devices, AppContext};
use crate::audio::capture::{read_pcm16_wav, write_pcm16_wav, CaptureSession};
use crate::audio::devices::InputDeviceInfo;
use crate::dsp::metrics::SAMPLE_RATE;
use crate::dsp::obs_mapping::{parse_scene_collection, preset_from_preview, ObsImportPreview};
use crate::dsp::pipeline::prepare_listen;
use crate::dsp::pipeline::DspPreset;
use crate::error::AppError;
use crate::history::repository::Recording;
use crate::history::retention::delete_recording;
use crate::settings::AppSettings;
use crate::transcription::openrouter::{list_transcription_models, SttModel};
use crate::windows_int::credentials::{has_api_key, set_api_key};
use crate::windows_int::text_injector::native;
use base64::engine::general_purpose::STANDARD;
use base64::Engine;
use std::path::Path;
use std::sync::Arc;
use tauri::{AppHandle, Manager, State};
use tauri_plugin_autostart::ManagerExt;

#[tauri::command]
pub fn get_session_state(ctx: State<'_, Arc<AppContext>>) -> SessionState {
    ctx.state.lock().clone()
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
    settings.retry.validate()?;
    settings.mic_tune.validate()?;
    settings.save(&ctx.settings_path)?;
    *ctx.settings.lock() = settings.clone();
    let _ = crate::app::session::apply_configured_retention(ctx.as_ref());
    if let Err(e) = crate::app::shortcuts::sync_shortcuts(&app) {
        tracing::error!(error = %e, "hotkey register failed");
    }
    let autostart = app.autolaunch();
    if settings.start_with_windows {
        let _ = autostart.enable();
    } else {
        let _ = autostart.disable();
    }
    let _ = configure_tray(&app);
    Ok(settings)
}

#[tauri::command]
pub fn list_history(ctx: State<'_, Arc<AppContext>>) -> Result<Vec<Recording>, AppError> {
    ctx.history.lock().list(500)
}

#[tauri::command]
pub fn get_recording(
    ctx: State<'_, Arc<AppContext>>,
    id: String,
) -> Result<Option<Recording>, AppError> {
    ctx.history.lock().get(&id)
}

#[tauri::command]
pub fn delete_history_item(ctx: State<'_, Arc<AppContext>>, id: String) -> Result<(), AppError> {
    let rec = ctx
        .history
        .lock()
        .get(&id)?
        .ok_or_else(|| AppError::StorageFailed("not found".into()))?;
    let root = crate::history::repository::audio_dir(&ctx.data_dir);
    delete_recording(&ctx.history.lock(), &root, &rec)?;
    Ok(())
}

#[tauri::command]
pub fn delete_all_history(ctx: State<'_, Arc<AppContext>>) -> Result<(), AppError> {
    let root = crate::history::repository::audio_dir(&ctx.data_dir);
    let items = ctx.history.lock().list(20_000)?;
    for rec in items {
        let _ = delete_recording(&ctx.history.lock(), &root, &rec);
    }
    ctx.history.lock().delete_all()?;
    Ok(())
}

#[tauri::command]
pub fn list_microphones() -> Result<Vec<InputDeviceInfo>, AppError> {
    devices()
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
    pub original_data_url: String,
    pub processed_data_url: String,
    pub peak: f32,
    pub rms: f32,
    pub clip_count: u32,
    pub nonce: u64,
}

fn wav_data_url(path: &Path) -> Result<String, AppError> {
    let bytes = std::fs::read(path).map_err(|e| AppError::StorageFailed(e.to_string()))?;
    Ok(format!("data:audio/wav;base64,{}", STANDARD.encode(bytes)))
}

fn preview_from_original(ctx: &AppContext, original: &Path) -> Result<DspPreview, AppError> {
    let samples = read_pcm16_wav(original)?;
    let preset = ctx.settings.lock().active_preset();
    let (processed, metrics) = prepare_listen(preset, samples)?;
    let audio_root = crate::history::repository::audio_dir(&ctx.data_dir);
    let processed_path = audio_root.join("filter-preview.wav");
    write_pcm16_wav(&processed_path, SAMPLE_RATE, &processed)?;
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|value| value.as_millis() as u64)
        .unwrap_or(0);
    Ok(DspPreview {
        original_path: original.to_string_lossy().into_owned(),
        processed_path: processed_path.to_string_lossy().into_owned(),
        original_data_url: wav_data_url(original)?,
        processed_data_url: wav_data_url(&processed_path)?,
        peak: metrics.peak,
        rms: metrics.rms,
        clip_count: metrics.clip_count,
        nonce,
    })
}

#[tauri::command]
pub fn preview_dsp(ctx: State<'_, Arc<AppContext>>) -> Result<DspPreview, AppError> {
    ctx.settings.lock().mic_tune.validate()?;
    let rec = ctx
        .history
        .lock()
        .list(20)?
        .into_iter()
        .find(|item| item.raw_audio_path.is_some())
        .ok_or_else(|| AppError::StorageFailed("no recording to preview".into()))?;
    let raw_name = rec
        .raw_audio_path
        .ok_or_else(|| AppError::StorageFailed("raw missing".into()))?;
    let audio_root = crate::history::repository::audio_dir(&ctx.data_dir);
    let original = audio_root.join(raw_name);
    if !original.exists() {
        return Err(AppError::StorageFailed("no recording to preview".into()));
    }
    preview_from_original(ctx.inner(), &original)
}

#[tauri::command]
pub fn start_filter_sample(ctx: State<'_, Arc<AppContext>>) -> Result<(), AppError> {
    if is_cancellable(&ctx.state.lock()) {
        return Err(AppError::IllegalTransition(
            "dictation is already running".into(),
        ));
    }
    if ctx.preview_capture.lock().is_some() {
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
    let session = CaptureSession::start(device.as_deref(), dest)?;
    *ctx.preview_capture.lock() = Some(session);
    Ok(())
}

#[tauri::command]
pub fn stop_filter_sample(ctx: State<'_, Arc<AppContext>>) -> Result<DspPreview, AppError> {
    let session = ctx
        .preview_capture
        .lock()
        .take()
        .ok_or_else(|| AppError::AudioCaptureFailed("sample not started".into()))?;
    let result = session.stop()?;
    preview_from_original(ctx.inner(), &result.path)
}

#[tauri::command]
pub async fn check_for_updates(app: AppHandle) -> Result<String, AppError> {
    let outcome = crate::updates::check_and_maybe_install(&app, true).await;
    Ok(outcome.as_str().into())
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
pub async fn test_openrouter(ctx: State<'_, Arc<AppContext>>) -> Result<String, AppError> {
    let key = crate::windows_int::credentials::get_api_key()?.ok_or(AppError::InvalidApiKey)?;
    let models = list_transcription_models(&ctx.client, &key).await?;
    Ok(format!("{} models", models.len()))
}

#[tauri::command]
pub async fn discover_models(ctx: State<'_, Arc<AppContext>>) -> Result<Vec<SttModel>, AppError> {
    let key = crate::windows_int::credentials::get_api_key()?.ok_or(AppError::InvalidApiKey)?;
    list_transcription_models(&ctx.client, &key).await
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
pub fn open_logs(ctx: State<'_, Arc<AppContext>>) -> Result<(), AppError> {
    let dir = ctx.data_dir.join("logs");
    std::fs::create_dir_all(&dir).map_err(|e| AppError::StorageFailed(e.to_string()))?;
    tauri_plugin_opener::open_path(dir, None::<&str>)
        .map_err(|e| AppError::StorageFailed(e.to_string()))
}

#[tauri::command]
pub fn open_audio_dir(ctx: State<'_, Arc<AppContext>>) -> Result<(), AppError> {
    let dir = crate::history::repository::audio_dir(&ctx.data_dir);
    std::fs::create_dir_all(&dir).map_err(|e| AppError::StorageFailed(e.to_string()))?;
    tauri_plugin_opener::open_path(dir, None::<&str>)
        .map_err(|e| AppError::StorageFailed(e.to_string()))
}

#[tauri::command]
pub fn preview_obs_import() -> Result<Vec<ObsImportPreview>, AppError> {
    crate::obs::load_local_previews()
}

#[tauri::command]
pub fn import_obs_preset(
    ctx: State<'_, Arc<AppContext>>,
    source_name: String,
    preset_name: String,
) -> Result<DspPreset, AppError> {
    let previews = crate::obs::load_local_previews()?;
    let preview = previews
        .into_iter()
        .find(|p| p.source_name == source_name)
        .ok_or_else(|| AppError::RequestValidationFailed("source not found".into()))?;
    let preset = preset_from_preview(&preview, preset_name);
    let mut settings = ctx.settings.lock().clone();
    settings.presets.push(preset.clone());
    settings.save(&ctx.settings_path)?;
    *ctx.settings.lock() = settings;
    Ok(preset)
}

#[tauri::command]
pub fn parse_obs_json(json: String) -> Result<Vec<ObsImportPreview>, AppError> {
    parse_scene_collection(&json).map_err(AppError::RequestValidationFailed)
}

#[tauri::command]
pub fn copy_transcript(text: String) -> Result<(), AppError> {
    native::clipboard_copy(&text)
}

#[tauri::command]
pub fn insert_transcript(app: AppHandle, text: String) -> Result<(), AppError> {
    let ctx = app.state::<Arc<AppContext>>();
    let mode = ctx.settings.lock().insertion_mode.clone();
    let captured = *ctx.captured_hwnd.lock();
    match mode.as_str() {
        "clipboard" => native::clipboard_paste(&text).map(|_| ()),
        "sendinput" => captured
            .or_else(native::foreground_hwnd)
            .ok_or_else(|| AppError::TextInsertionFailed("no window".into()))
            .and_then(|hwnd| native::insert_unicode(hwnd, &text)),
        _ => captured
            .ok_or_else(|| AppError::TextInsertionFailed("no captured window".into()))
            .and_then(|hwnd| native::insert_into_window(hwnd, &text)),
    }
}

#[tauri::command]
pub fn run_retention(ctx: State<'_, Arc<AppContext>>) -> Result<Vec<String>, AppError> {
    crate::app::session::apply_configured_retention(ctx.as_ref())
}

#[tauri::command]
pub fn overlay_timing(ctx: State<'_, Arc<AppContext>>) -> Option<u128> {
    ctx.overlay_shown_at
        .lock()
        .as_ref()
        .map(|t| t.elapsed().as_millis())
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
    let Some(name) = rec
        .raw_audio_path
        .as_ref()
        .or(rec.processed_audio_path.as_ref())
    else {
        return Ok(None);
    };
    if name.contains("..") || Path::new(name).is_absolute() {
        return Err(AppError::StorageFailed("invalid audio path".into()));
    }
    let root = crate::history::repository::audio_dir(&ctx.data_dir);
    let path = root.join(name);
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
