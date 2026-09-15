use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager};

use crate::app::machine::is_cancellable;
use crate::app::session::{release_meter_monitor, AppContext};
use crate::audio::capture::{read_pcm16_wav_with_rate, write_pcm16_wav, CaptureSession};
use crate::dsp::metrics::{SAMPLE_RATE, STT_SAMPLE_RATE};
use crate::dsp::pipeline::{prepare_listen_audio, samples_for_stt};
use crate::error::AppError;
use crate::history::repository::audio_dir;
use crate::transcription::openrouter::{build_stt_client, transcribe_file, TranscriptionSuccess};
use crate::transcription::retry::RetryPolicy;
use crate::windows_int::credentials::get_api_key;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct CompareSlot {
    pub model: String,
    pub status: String,
    pub text: Option<String>,
    pub error: Option<String>,
    pub attempt: u32,
    pub cost: Option<f64>,
    pub latency_ms: Option<u128>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct CompareState {
    pub recording: bool,
    pub running: bool,
    pub nonce: u64,
    pub listen_path: Option<String>,
    pub stt_path: Option<String>,
    pub run_id: Option<String>,
    pub slots: Vec<CompareSlot>,
}

pub fn compare_listen_name(nonce: u64) -> String {
    format!("model-compare.{nonce}.listen.wav")
}

pub fn compare_stt_name(nonce: u64) -> String {
    format!("model-compare.{nonce}.stt.wav")
}

pub fn compare_raw_name(nonce: u64) -> String {
    format!("model-compare.{nonce}.raw.wav")
}

pub fn validate_compare_models(models: &[String]) -> Result<Vec<String>, AppError> {
    let trimmed: Vec<String> = models
        .iter()
        .map(|model| model.trim().to_string())
        .filter(|model| !model.is_empty())
        .collect();
    if !(2..=4).contains(&trimmed.len()) {
        return Err(AppError::RequestValidationFailed(
            "compare needs 2-4 models".into(),
        ));
    }
    let mut unique = trimmed.clone();
    unique.sort();
    unique.dedup();
    if unique.len() != trimmed.len() {
        return Err(AppError::RequestValidationFailed(
            "compare models must be unique".into(),
        ));
    }
    Ok(trimmed)
}

pub fn keep_compare_wav(name: &str) -> bool {
    name == "filter-sample.wav"
        || name.starts_with("filter-preview")
        || name.starts_with("model-compare")
}

pub fn compare_busy(ctx: &AppContext) -> bool {
    ctx.compare_capture.lock().is_some()
        || ctx
            .compare_running
            .load(std::sync::atomic::Ordering::SeqCst)
}

fn emit_compare(app: &AppHandle) {
    let ctx = app.state::<Arc<AppContext>>();
    let state = ctx.compare_state.lock().clone();
    let _ = app.emit("compare://state", state);
}

pub fn start_model_compare(app: &AppHandle) -> Result<(), AppError> {
    let ctx = app.state::<Arc<AppContext>>();
    if is_cancellable(&ctx.state.lock()) {
        return Err(AppError::IllegalTransition(
            "dictation is already running".into(),
        ));
    }
    if ctx.preview_capture.lock().is_some() {
        return Err(AppError::IllegalTransition(
            "filter sample is running".into(),
        ));
    }
    if ctx.compare_capture.lock().is_some() {
        return Err(AppError::IllegalTransition(
            "compare is already recording".into(),
        ));
    }
    if ctx
        .compare_running
        .load(std::sync::atomic::Ordering::SeqCst)
    {
        return Err(AppError::TranscriptionInProgress);
    }
    release_meter_monitor(&ctx);
    let nonce = {
        let mut state = ctx.compare_state.lock();
        state.nonce = state.nonce.saturating_add(1).max(1);
        state.nonce
    };
    let dest = audio_dir(&ctx.data_dir).join(compare_raw_name(nonce));
    let settings = ctx.settings.lock().clone();
    let device = if settings.input_device == "default" {
        None
    } else {
        Some(settings.input_device.clone())
    };
    let session = CaptureSession::start(device.as_deref(), dest)?;
    *ctx.compare_capture.lock() = Some(session);
    ctx.compare_state.lock().recording = true;
    emit_compare(app);
    Ok(())
}

pub fn stop_model_compare(app: &AppHandle) -> Result<CompareState, AppError> {
    let ctx = app.state::<Arc<AppContext>>();
    let session = ctx
        .compare_capture
        .lock()
        .take()
        .ok_or_else(|| AppError::AudioCaptureFailed("compare not started".into()))?;
    let result = session.stop()?;
    let (pcm, rate) = if result.samples.is_empty() {
        read_pcm16_wav_with_rate(&result.path)?
    } else {
        (result.samples, result.sample_rate)
    };
    let preset = ctx.settings.lock().active_preset();
    let listen = prepare_listen_audio(preset, pcm)?;
    let stt = samples_for_stt(&listen, rate);
    let audio = audio_dir(&ctx.data_dir);
    let nonce = ctx.compare_state.lock().nonce.max(1);
    let listen_path = audio.join(compare_listen_name(nonce));
    let stt_path = audio.join(compare_stt_name(nonce));
    write_pcm16_wav(&listen_path, SAMPLE_RATE, &listen)?;
    write_pcm16_wav(&stt_path, STT_SAMPLE_RATE, &stt)?;
    let mut state = ctx.compare_state.lock();
    state.recording = false;
    state.nonce = nonce;
    state.listen_path = Some(listen_path.to_string_lossy().into_owned());
    state.stt_path = Some(stt_path.to_string_lossy().into_owned());
    let snapshot = state.clone();
    drop(state);
    emit_compare(app);
    Ok(snapshot)
}

pub fn get_model_compare(app: &AppHandle) -> CompareState {
    app.state::<Arc<AppContext>>().compare_state.lock().clone()
}

pub fn clear_model_compare(app: &AppHandle) -> Result<(), AppError> {
    let ctx = app.state::<Arc<AppContext>>();
    if ctx
        .compare_running
        .load(std::sync::atomic::Ordering::SeqCst)
    {
        return Err(AppError::TranscriptionInProgress);
    }
    if let Some(tx) = ctx.compare_cancel.lock().as_ref() {
        let _ = tx.send(true);
    }
    *ctx.compare_state.lock() = CompareState::default();
    emit_compare(app);
    Ok(())
}

pub fn cancel_model_compare(ctx: &AppContext) {
    if let Some(tx) = ctx.compare_cancel.lock().as_ref() {
        let _ = tx.send(true);
    }
}

pub async fn run_model_compare(app: AppHandle) -> Result<CompareState, AppError> {
    let ctx = app.state::<Arc<AppContext>>();
    if ctx
        .compare_running
        .swap(true, std::sync::atomic::Ordering::SeqCst)
    {
        return Err(AppError::TranscriptionInProgress);
    }
    let result = run_compare_inner(app.clone()).await;
    ctx.compare_running
        .store(false, std::sync::atomic::Ordering::SeqCst);
    result
}

async fn run_compare_inner(app: AppHandle) -> Result<CompareState, AppError> {
    let ctx = app.state::<Arc<AppContext>>();
    let settings = ctx.settings.lock().clone();
    let models = validate_compare_models(&settings.compare_models)?;
    let key = get_api_key()?.ok_or(AppError::InvalidApiKey)?;
    let stt_src = ctx
        .compare_state
        .lock()
        .stt_path
        .clone()
        .ok_or_else(|| AppError::StorageFailed("no compare clip".into()))?;
    let run_id = uuid::Uuid::new_v4().to_string();
    let frozen = audio_dir(&ctx.data_dir).join(format!("model-compare.{run_id}.stt.wav"));
    std::fs::copy(&stt_src, &frozen).map_err(|e| AppError::StorageFailed(e.to_string()))?;
    let (tx, _) = tokio::sync::watch::channel(false);
    *ctx.compare_cancel.lock() = Some(tx.clone());
    {
        let mut state = ctx.compare_state.lock();
        state.running = true;
        state.run_id = Some(run_id.clone());
        state.stt_path = Some(frozen.to_string_lossy().into_owned());
        state.slots = models
            .iter()
            .map(|model| CompareSlot {
                model: model.clone(),
                status: "running".into(),
                text: None,
                error: None,
                attempt: 0,
                cost: None,
                latency_ms: None,
            })
            .collect();
    }
    emit_compare(&app);
    let policy = compare_retry_policy(settings.retry.to_policy());
    let language = if settings.language == "auto" {
        None
    } else {
        Some(settings.language.clone())
    };
    let base = crate::transcription::openrouter::default_base_url().to_string();
    let mut set = tokio::task::JoinSet::new();
    for (index, model) in models.into_iter().enumerate() {
        let key = key.clone();
        let frozen = frozen.clone();
        let language = language.clone();
        let policy = policy.clone();
        let rx = tx.subscribe();
        let base = base.clone();
        set.spawn(async move {
            let client = match build_stt_client(policy.connect_timeout) {
                Ok(client) => client,
                Err(err) => return (index, model, Err(err)),
            };
            let outcome = transcribe_file(
                &client,
                &base,
                &key,
                &model,
                language.as_deref(),
                &frozen,
                policy,
                Duration::from_secs(1),
                rx,
            )
            .await;
            (index, model, outcome)
        });
    }
    while let Some(joined) = set.join_next().await {
        match joined {
            Ok((index, model, Ok(success))) => {
                apply_slot_success(&app, index, &model, success);
            }
            Ok((index, model, Err(err))) => {
                apply_slot_error(&app, index, &model, err);
            }
            Err(err) => {
                tracing::error!(error = %err, "compare join failed");
            }
        }
        emit_compare(&app);
    }
    let mut state = ctx.compare_state.lock();
    state.running = false;
    let snapshot = state.clone();
    drop(state);
    emit_compare(&app);
    Ok(snapshot)
}

fn compare_retry_policy(mut policy: RetryPolicy) -> RetryPolicy {
    policy.additional_retries = policy.additional_retries.min(1);
    policy.total_operation_timeout = Duration::from_secs(90);
    policy
}

fn apply_slot_success(app: &AppHandle, index: usize, model: &str, success: TranscriptionSuccess) {
    let ctx = app.state::<Arc<AppContext>>();
    let mut state = ctx.compare_state.lock();
    if let Some(slot) = state.slots.get_mut(index) {
        slot.model = model.into();
        slot.status = "done".into();
        slot.text = Some(success.text);
        slot.error = None;
        slot.attempt = success.attempt;
        slot.cost = success.cost;
        slot.latency_ms = Some(success.latency_ms);
    }
}

fn apply_slot_error(app: &AppHandle, index: usize, model: &str, err: AppError) {
    let ctx = app.state::<Arc<AppContext>>();
    let mut state = ctx.compare_state.lock();
    if let Some(slot) = state.slots.get_mut(index) {
        slot.model = model.into();
        slot.status = "error".into();
        slot.error = Some(err.code().to_string());
        slot.text = None;
    }
}

pub fn steal_compare_capture(ctx: &AppContext) {
    if let Some(session) = ctx.compare_capture.lock().take() {
        std::thread::spawn(move || {
            if let Ok(result) = session.stop() {
                let _ = std::fs::remove_file(result.path);
            }
        });
    }
    ctx.compare_state.lock().recording = false;
}

pub fn frozen_stt_copy(audio_root: &Path, run_id: &str, src: &Path) -> Result<PathBuf, AppError> {
    let dest = audio_root.join(format!("model-compare.{run_id}.stt.wav"));
    std::fs::copy(src, &dest).map_err(|e| AppError::StorageFailed(e.to_string()))?;
    Ok(dest)
}

#[cfg(test)]
mod tests {
    use super::*;
    use httpmock::prelude::*;
    use std::time::Duration;

    #[test]
    fn validate_compare_models_bounds() {
        assert!(validate_compare_models(&["a".into()]).is_err());
        assert!(validate_compare_models(&["a".into(), "a".into()]).is_err());
        assert!(validate_compare_models(&[
            "a".into(),
            "b".into(),
            "c".into(),
            "d".into(),
            "e".into()
        ])
        .is_err());
        assert_eq!(
            validate_compare_models(&[" a ".into(), "b".into()]).unwrap(),
            vec!["a", "b"]
        );
    }

    #[test]
    fn keep_list_covers_compare_and_filter() {
        assert!(keep_compare_wav("filter-sample.wav"));
        assert!(keep_compare_wav("filter-preview.wav"));
        assert!(keep_compare_wav("model-compare.stt.wav"));
        assert!(keep_compare_wav("model-compare.abc.stt.wav"));
        assert!(!keep_compare_wav("orphan.wav"));
        assert!(keep_compare_wav(&compare_listen_name(9)));
    }

    #[test]
    fn compare_clip_paths_change_across_takes() {
        assert_eq!(compare_listen_name(1), "model-compare.1.listen.wav");
        assert_ne!(compare_listen_name(1), compare_listen_name(2));
        assert_ne!(compare_stt_name(1), compare_stt_name(2));
        assert_ne!(compare_raw_name(1), compare_raw_name(2));
    }

    #[test]
    fn frozen_copy_survives_raw_overwrite() {
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("model-compare.stt.wav");
        std::fs::write(&src, b"one").unwrap();
        let frozen = frozen_stt_copy(dir.path(), "run1", &src).unwrap();
        std::fs::write(&src, b"two").unwrap();
        assert_eq!(std::fs::read(&frozen).unwrap(), b"one");
        assert_ne!(
            std::fs::read(&src).unwrap(),
            std::fs::read(&frozen).unwrap()
        );
    }

    #[tokio::test]
    async fn parallel_slots_do_not_share_failure() {
        let server = MockServer::start();
        server.mock(|when, then| {
            when.method(POST)
                .path("/audio/transcriptions")
                .body_contains("model-a");
            then.status(401)
                .json_body(serde_json::json!({"error":"no"}));
        });
        server.mock(|when, then| {
            when.method(POST)
                .path("/audio/transcriptions")
                .body_contains("model-b");
            then.status(200)
                .json_body(serde_json::json!({"text":"hello"}));
        });
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("clip.wav");
        crate::audio::capture::write_pcm16_wav(&path, 16_000, &[0.1; 1600]).unwrap();
        let client = reqwest::Client::new();
        let (_tx, rx_a) = tokio::sync::watch::channel(false);
        let rx_b = rx_a.clone();
        let policy = compare_retry_policy(RetryPolicy {
            automatic_retries: false,
            additional_retries: 0,
            connect_timeout: Duration::from_secs(2),
            request_timeout: Duration::from_secs(5),
            initial_retry_delay: Duration::from_millis(10),
            max_retry_delay: Duration::from_secs(1),
            total_operation_timeout: Duration::from_secs(10),
        });
        let a = transcribe_file(
            &client,
            &server.base_url(),
            "k",
            "model-a",
            None,
            &path,
            policy.clone(),
            Duration::from_secs(1),
            rx_a,
        )
        .await;
        let b = transcribe_file(
            &client,
            &server.base_url(),
            "k",
            "model-b",
            None,
            &path,
            policy,
            Duration::from_secs(1),
            rx_b,
        )
        .await;
        assert!(a.is_err());
        assert_eq!(b.unwrap().text, "hello");
    }
}
