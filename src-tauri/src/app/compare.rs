use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager};

use crate::app::machine::is_cancellable;
use crate::app::session::{release_meter_monitor, AppContext};
use crate::audio::capture::{
    read_pcm16_wav_with_rate, validate_capture_result, write_pcm16_wav, CaptureSession,
    CaptureStopOutcome,
};
use crate::dsp::metrics::{SAMPLE_RATE, STT_SAMPLE_RATE};
use crate::dsp::pipeline::{prepare_listen_audio, samples_for_stt};
use crate::error::AppError;
use crate::history::repository::audio_dir;
use crate::transcription::openrouter::{transcribe_file, TranscriptionSuccess};
use crate::transcription::retry::RetryPolicy;
use crate::windows_int::credentials::get_api_key;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct CompareSlot {
    pub slot_id: String,
    pub model: String,
    pub status: String,
    pub text: Option<String>,
    pub error: Option<String>,
    pub attempt: u32,
    pub cost: Option<f64>,
    pub latency_ms: Option<u128>,
    pub clip_nonce: u64,
    pub run_id: Option<String>,
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
    if !(2..=12).contains(&trimmed.len()) {
        return Err(AppError::RequestValidationFailed(
            "compare needs 2-12 models".into(),
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
    crate::history::retention::keep_temp_wav(name)
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
    let _ = app.emit_to("main", "compare://state", state);
}

pub fn start_model_compare(app: &AppHandle) -> Result<(), AppError> {
    let ctx = app.state::<Arc<AppContext>>();
    let _admission = crate::app::operations::lock_admission(&ctx);
    if crate::app::operations::system_busy(&ctx) {
        return Err(AppError::TranscriptionInProgress);
    }
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
    if !release_meter_monitor(&ctx) {
        return Err(AppError::TranscriptionInProgress);
    }
    let nonce = {
        let state = ctx.compare_state.lock();
        state.nonce.saturating_add(1).max(1)
    };
    let dest = audio_dir(&ctx.data_dir).join(compare_raw_name(nonce));
    let settings = ctx.settings.lock().clone();
    let device = if settings.input_device == "default" {
        None
    } else {
        Some(settings.input_device.clone())
    };
    {
        let mut state = ctx.compare_state.lock();
        state.nonce = nonce;
        state.recording = true;
        state.slots.clear();
        state.run_id = None;
        state.listen_path = None;
        state.stt_path = None;
    }
    ctx.pending_native_capture_starts
        .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    let late_ctx = ctx.inner().clone();
    let late_app = app.clone();
    let started = CaptureSession::start_with_late_completion(device.as_deref(), dest, move || {
        let _admission = crate::app::operations::lock_admission(&late_ctx);
        release_compare_capture_reservation(&late_ctx, nonce);
        crate::app::session::release_pending_native_capture_start(&late_ctx);
        drop(_admission);
        emit_compare(&late_app);
        crate::app::session::schedule_deferred_shutdown_exit(&late_app, &late_ctx);
    });
    let native_worker_pending = started
        .as_ref()
        .err()
        .is_some_and(|failure| failure.native_worker_pending);
    let session = match started {
        Ok(session) => session,
        Err(failure) => {
            if !native_worker_pending {
                release_compare_capture_reservation(&ctx, nonce);
                emit_compare(app);
                crate::app::session::release_pending_native_capture_start(&ctx);
                crate::app::operations::with_admission_released(_admission, || {
                    crate::app::session::schedule_deferred_shutdown_exit(app, &ctx)
                });
            }
            return Err(failure.error);
        }
    };
    *ctx.compare_capture.lock() = Some(session);
    crate::app::session::release_pending_native_capture_start(&ctx);
    crate::app::operations::with_admission_released(_admission, || {
        crate::app::session::schedule_deferred_shutdown_exit(app, &ctx)
    });
    emit_compare(app);
    let watcher_app = app.clone();
    tauri::async_runtime::spawn(async move {
        loop {
            tokio::time::sleep(Duration::from_millis(100)).await;
            let ctx = watcher_app.state::<Arc<AppContext>>();
            if ctx.compare_state.lock().nonce != nonce {
                break;
            }
            let finished = ctx
                .compare_capture
                .lock()
                .as_ref()
                .map(CaptureSession::is_finished);
            match finished {
                Some(true) => {
                    if let Err(err) = stop_model_compare(&watcher_app) {
                        let _ = watcher_app.emit_to("main", "compare://error", err.code());
                    }
                    break;
                }
                None => break,
                _ => {}
            }
        }
    });
    Ok(())
}

pub fn stop_model_compare(app: &AppHandle) -> Result<CompareState, AppError> {
    let ctx = app.state::<Arc<AppContext>>();
    let (session, nonce) = {
        let _admission = crate::app::operations::lock_admission(&ctx);
        let session = ctx
            .compare_capture
            .lock()
            .take()
            .ok_or_else(|| AppError::AudioCaptureFailed("compare not started".into()))?;
        (session, ctx.compare_state.lock().nonce)
    };
    let cleanup = session.cleanup_handle();
    let late_cleanup = cleanup.clone();
    let late_app = app.clone();
    let late_handler =
        compare_late_result_handler(Arc::clone(&ctx), nonce, late_cleanup, Some(app.clone()));
    match session.stop_with_late_result(move |_| {
        late_handler();
        emit_compare(&late_app);
    }) {
        CaptureStopOutcome::TimedOut(err) => {
            cleanup.discard_late_result();
            emit_compare(app);
            Err(err)
        }
        CaptureStopOutcome::Completed(result) => {
            let result = if ctx
                .shutdown_requested
                .load(std::sync::atomic::Ordering::SeqCst)
            {
                cleanup.discard_late_result();
                Err(AppError::Cancelled)
            } else {
                stop_model_compare_inner(&ctx, nonce, result, cleanup)
            };
            release_compare_capture_reservation(&ctx, nonce);
            emit_compare(app);
            crate::app::session::schedule_deferred_shutdown_exit(app, &ctx);
            result.map(|mut snapshot| {
                snapshot.recording = false;
                snapshot
            })
        }
    }
}

fn stop_model_compare_inner(
    ctx: &AppContext,
    nonce: u64,
    result: Result<crate::audio::capture::CaptureResult, AppError>,
    cleanup: crate::audio::capture::CaptureCleanupHandle,
) -> Result<CompareState, AppError> {
    let processed = (|| {
        let result = result?;
        validate_capture_result(&result)?;
        let (pcm, rate) = if result.samples.is_empty() {
            read_pcm16_wav_with_rate(&result.path)?
        } else {
            (result.samples, result.sample_rate)
        };
        let preset = ctx.settings.lock().active_preset();
        process_compare_audio(ctx, nonce, pcm, rate, |pcm| {
            prepare_listen_audio(preset, pcm)
        })
    })();
    if processed.is_err() {
        cleanup.discard_late_result();
    }
    processed
}

fn process_compare_audio(
    ctx: &AppContext,
    nonce: u64,
    pcm: Vec<f32>,
    rate: u32,
    prepare: impl FnOnce(Vec<f32>) -> Result<Vec<f32>, AppError>,
) -> Result<CompareState, AppError> {
    let listen = prepare(pcm)?;
    let stt = samples_for_stt(&listen, rate);
    commit_compare_audio(ctx, nonce, &listen, &stt)
}

fn commit_compare_audio(
    ctx: &AppContext,
    nonce: u64,
    listen: &[f32],
    stt: &[f32],
) -> Result<CompareState, AppError> {
    let _admission = crate::app::operations::lock_admission(ctx);
    let mut state = ctx.compare_state.lock();
    if state.nonce != nonce
        || !state.recording
        || ctx
            .shutdown_requested
            .load(std::sync::atomic::Ordering::SeqCst)
    {
        return Err(AppError::Cancelled);
    }
    let audio = audio_dir(&ctx.data_dir);
    let listen_path = audio.join(compare_listen_name(nonce));
    let stt_path = audio.join(compare_stt_name(nonce));
    // Each take owns fresh paths. Refuse a collision before writing either output.
    for path in [&listen_path, &stt_path] {
        if path
            .try_exists()
            .map_err(|err| AppError::StorageFailed(err.to_string()))?
        {
            return Err(AppError::StorageFailed(
                "compare output already exists".into(),
            ));
        }
    }
    let written = (|| {
        write_pcm16_wav(&listen_path, SAMPLE_RATE, listen)?;
        write_pcm16_wav(&stt_path, STT_SAMPLE_RATE, stt)
    })();
    if let Err(err) = written {
        for path in [
            listen_path.clone(),
            listen_path.with_extension("wav.tmp"),
            stt_path.clone(),
            stt_path.with_extension("wav.tmp"),
        ] {
            if let Err(cleanup_error) = std::fs::remove_file(&path) {
                if cleanup_error.kind() != std::io::ErrorKind::NotFound {
                    tracing::warn!(error = %cleanup_error, file = %path.display(), "remove uncommitted compare output");
                }
            }
        }
        return Err(err);
    }
    state.recording = false;
    state.nonce = nonce;
    state.listen_path = Some(listen_path.to_string_lossy().into_owned());
    state.stt_path = Some(stt_path.to_string_lossy().into_owned());
    Ok(state.clone())
}

pub(crate) fn release_compare_capture_reservation(ctx: &AppContext, nonce: u64) {
    let mut state = ctx.compare_state.lock();
    if state.nonce == nonce {
        state.recording = false;
    }
}

fn compare_late_result_handler(
    ctx: Arc<AppContext>,
    nonce: u64,
    cleanup: crate::audio::capture::CaptureCleanupHandle,
    app: Option<AppHandle>,
) -> impl FnOnce() + Send + 'static {
    move || {
        cleanup.discard_late_result();
        release_compare_capture_reservation(&ctx, nonce);
        if let Some(app) = app.as_ref() {
            crate::app::session::schedule_deferred_shutdown_exit(app, &ctx);
        }
    }
}

pub fn get_model_compare(app: &AppHandle) -> CompareState {
    app.state::<Arc<AppContext>>().compare_state.lock().clone()
}

pub fn clear_model_compare(app: &AppHandle) -> Result<(), AppError> {
    let ctx = app.state::<Arc<AppContext>>();
    let _admission = crate::app::operations::lock_admission(&ctx);
    if ctx.compare_state.lock().recording
        || ctx
            .compare_running
            .load(std::sync::atomic::Ordering::SeqCst)
    {
        return Err(AppError::TranscriptionInProgress);
    }
    if let Some(tx) = ctx.compare_cancel.lock().as_ref() {
        tx.send_replace(true);
    }
    *ctx.compare_state.lock() = CompareState::default();
    emit_compare(app);
    let failed = remove_compare_artifacts(&audio_dir(&ctx.data_dir));
    if !failed.is_empty() {
        return Err(AppError::StorageFailed(format!(
            "could not remove compare audio: {}",
            failed.join(", ")
        )));
    }
    Ok(())
}

pub fn remove_compare_artifacts(audio_root: &Path) -> Vec<String> {
    let entries = match std::fs::read_dir(audio_root) {
        Ok(entries) => entries,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Vec::new(),
        Err(_) => return vec![audio_root.to_string_lossy().into_owned()],
    };
    entries
        .filter_map(|entry| match entry {
            Ok(entry)
                if entry
                    .file_name()
                    .to_string_lossy()
                    .starts_with("model-compare.") =>
            {
                std::fs::remove_file(entry.path())
                    .err()
                    .map(|_| entry.path().to_string_lossy().into_owned())
            }
            Ok(_) => None,
            Err(err) => Some(err.to_string()),
        })
        .collect()
}

pub fn cancel_model_compare(app: &AppHandle) -> Result<CompareState, AppError> {
    let ctx = app.state::<Arc<AppContext>>();
    if let Some(tx) = ctx.compare_cancel.lock().as_ref() {
        tx.send_replace(true);
    }
    {
        let mut state = ctx.compare_state.lock();
        mark_compare_cancelled(&mut state);
    }
    // Keep the lease until the cancelled tasks have actually joined.
    emit_compare(app);
    let snapshot = ctx.compare_state.lock().clone();
    Ok(snapshot)
}

fn mark_compare_cancelled(state: &mut CompareState) {
    for slot in &mut state.slots {
        if slot.status == "running" {
            slot.status = "cancelled".into();
        }
    }
}

fn reserve_compare_run(ctx: &AppContext) -> Result<tokio::sync::watch::Sender<bool>, AppError> {
    let _admission = crate::app::operations::lock_admission(ctx);
    if crate::app::operations::system_busy(ctx) {
        return Err(AppError::TranscriptionInProgress);
    }
    let (cancel, _) = tokio::sync::watch::channel(false);
    *ctx.compare_cancel.lock() = Some(cancel.clone());
    ctx.compare_running
        .store(true, std::sync::atomic::Ordering::SeqCst);
    Ok(cancel)
}

#[cfg(test)]
#[test]
fn reserve_compare_run_installs_a_persistent_cancel_signal() {
    let dir = tempfile::tempdir().unwrap();
    let ctx = AppContext::initialize(dir.path().to_path_buf()).unwrap();
    let cancel = reserve_compare_run(&ctx).unwrap();

    cancel_model_compare_internal(&ctx);
    let receiver = cancel.subscribe();

    assert!(*receiver.borrow());
    finish_compare_run(&ctx);
    assert!(ctx.compare_cancel.lock().is_none());
}

fn finish_compare_run(ctx: &AppContext) {
    let _admission = crate::app::operations::lock_admission(ctx);
    ctx.compare_state.lock().running = false;
    ctx.compare_cancel.lock().take();
    ctx.compare_running
        .store(false, std::sync::atomic::Ordering::SeqCst);
}

pub fn cancel_model_compare_internal(ctx: &AppContext) {
    if let Some(tx) = ctx.compare_cancel.lock().as_ref() {
        tx.send_replace(true);
    }
}

pub async fn run_model_compare(app: AppHandle) -> Result<CompareState, AppError> {
    let ctx = app.state::<Arc<AppContext>>();
    let cancel = reserve_compare_run(&ctx)?;
    let result = run_compare_inner(app.clone(), cancel).await;
    finish_compare_run(&ctx);
    emit_compare(&app);
    result
}

async fn run_compare_inner(
    app: AppHandle,
    tx: tokio::sync::watch::Sender<bool>,
) -> Result<CompareState, AppError> {
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
    {
        let mut state = ctx.compare_state.lock();
        state.running = true;
        state.run_id = Some(run_id.clone());
        state.stt_path = Some(frozen.to_string_lossy().into_owned());
        let clip_nonce = state.nonce;
        state.slots = models
            .iter()
            .map(|model| CompareSlot {
                slot_id: uuid::Uuid::new_v4().to_string(),
                model: model.clone(),
                status: "running".into(),
                text: None,
                error: None,
                attempt: 0,
                cost: None,
                latency_ms: None,
                clip_nonce,
                run_id: Some(run_id.clone()),
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
    let client = ctx.clone_transport_client()?;
    let audio_duration = {
        let (pcm, rate) =
            read_pcm16_wav_with_rate(&frozen).unwrap_or((Vec::new(), STT_SAMPLE_RATE));
        if rate == 0 {
            Duration::from_millis(1)
        } else {
            Duration::from_millis((pcm.len() as u64 * 1000) / u64::from(rate))
        }
    };
    let mut set = tokio::task::JoinSet::new();
    for slot in ctx.compare_state.lock().slots.clone() {
        let key = key.clone();
        let frozen = frozen.clone();
        let language = language.clone();
        let policy = policy.clone();
        let rx = tx.subscribe();
        let base = base.clone();
        let client = client.clone();
        let slot_id = slot.slot_id.clone();
        let model = slot.model.clone();
        set.spawn(async move {
            let outcome = transcribe_file(
                &client,
                &base,
                &key,
                &model,
                language.as_deref(),
                &frozen,
                policy,
                audio_duration,
                rx,
            )
            .await;
            (slot_id, model, outcome)
        });
    }
    while let Some(joined) = set.join_next().await {
        match joined {
            Ok((slot_id, model, Ok(success))) => {
                apply_slot_success(&app, &slot_id, &model, success);
            }
            Ok((slot_id, model, Err(err))) => {
                apply_slot_error(&app, &slot_id, &model, err);
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

fn apply_slot_success(app: &AppHandle, slot_id: &str, model: &str, success: TranscriptionSuccess) {
    let ctx = app.state::<Arc<AppContext>>();
    let mut state = ctx.compare_state.lock();
    if let Some(slot) = state.slots.iter_mut().find(|slot| slot.slot_id == slot_id) {
        slot.model = model.into();
        slot.status = "done".into();
        slot.text = Some(success.text);
        slot.error = None;
        slot.attempt = success.attempt;
        slot.cost = success.cost;
        slot.latency_ms = Some(success.latency_ms);
    }
}

fn apply_slot_error(app: &AppHandle, slot_id: &str, model: &str, err: AppError) {
    let ctx = app.state::<Arc<AppContext>>();
    let mut state = ctx.compare_state.lock();
    if let Some(slot) = state.slots.iter_mut().find(|slot| slot.slot_id == slot_id) {
        slot.model = model.into();
        slot.status = if matches!(err, AppError::Cancelled) {
            "cancelled".into()
        } else {
            "error".into()
        };
        slot.error = Some(err.code().to_string());
        slot.text = None;
        slot.attempt = slot.attempt.max(1);
    }
}

pub fn steal_compare_capture(app: &AppHandle) {
    let ctx = app.state::<Arc<AppContext>>();
    if let Some(session) = ctx.compare_capture.lock().take() {
        std::thread::spawn(move || {
            let _ = session.stop_and_discard();
        });
    }
    ctx.compare_state.lock().recording = false;
    emit_compare(app);
}

pub fn frozen_stt_copy(audio_root: &Path, run_id: &str, src: &Path) -> Result<PathBuf, AppError> {
    let dest = audio_root.join(format!("model-compare.{run_id}.stt.wav"));
    std::fs::copy(src, &dest).map_err(|e| AppError::StorageFailed(e.to_string()))?;
    Ok(dest)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audio::capture::{CaptureCleanupHandle, CaptureResult};
    use httpmock::prelude::*;
    use std::time::Duration;

    fn recording_context(dir: &Path, nonce: u64) -> AppContext {
        let ctx = AppContext::initialize(dir.to_path_buf()).unwrap();
        ctx.compare_state.lock().nonce = nonce;
        ctx.compare_state.lock().recording = true;
        ctx
    }

    fn finished_capture_cleanup(path: &Path) -> CaptureCleanupHandle {
        let cleanup = CaptureCleanupHandle::new(Some(path.to_path_buf()));
        cleanup.mark_finished();
        cleanup
    }

    fn capture_result(path: PathBuf, samples: Vec<f32>) -> CaptureResult {
        CaptureResult {
            path,
            duration_ms: 100,
            sample_rate: SAMPLE_RATE,
            samples,
            truncated: false,
            capture_error: None,
        }
    }

    #[test]
    fn compare_stop_discards_unreadable_capture_and_temporary_file() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = recording_context(dir.path(), 3);
        let raw = audio_dir(dir.path()).join(compare_raw_name(3));
        std::fs::write(&raw, b"invalid WAV").unwrap();
        std::fs::write(raw.with_extension("wav.tmp"), b"unfinished WAV").unwrap();
        let before = ctx.compare_state.lock().clone();
        let result = stop_model_compare_inner(
            &ctx,
            3,
            Ok(capture_result(raw.clone(), Vec::new())),
            finished_capture_cleanup(&raw),
        );
        assert!(matches!(result, Err(AppError::StorageFailed(_))));
        assert!(!raw.exists());
        assert!(!raw.with_extension("wav.tmp").exists());
        assert_eq!(*ctx.compare_state.lock(), before);
        release_compare_capture_reservation(&ctx, 3);
        assert!(!crate::app::operations::system_busy(&ctx));
    }

    #[test]
    fn compare_stop_discards_raw_audio_when_processed_output_cannot_be_saved() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = recording_context(dir.path(), 3);
        let raw = audio_dir(dir.path()).join(compare_raw_name(3));
        write_pcm16_wav(&raw, SAMPLE_RATE, &[0.1; 4800]).unwrap();
        // A directory at the atomic writer's temporary path deterministically rejects creation.
        let blocked = audio_dir(dir.path())
            .join(compare_listen_name(3))
            .with_extension("wav.tmp");
        std::fs::create_dir(&blocked).unwrap();
        let before = ctx.compare_state.lock().clone();
        let result = stop_model_compare_inner(
            &ctx,
            3,
            Ok(capture_result(raw.clone(), vec![0.1; 4800])),
            finished_capture_cleanup(&raw),
        );
        assert!(matches!(result, Err(AppError::StorageFailed(_))));
        assert!(!raw.exists());
        assert_eq!(*ctx.compare_state.lock(), before);
        assert!(!audio_dir(dir.path()).join(compare_stt_name(3)).exists());
    }

    #[test]
    fn compare_second_output_failure_rolls_back_only_new_files_and_keeps_previous_clip() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = recording_context(dir.path(), 3);
        let previous_listen = audio_dir(dir.path()).join(compare_listen_name(2));
        let previous_stt = audio_dir(dir.path()).join(compare_stt_name(2));
        write_pcm16_wav(&previous_listen, SAMPLE_RATE, &[0.25; 64]).unwrap();
        write_pcm16_wav(&previous_stt, STT_SAMPLE_RATE, &[0.25; 16]).unwrap();
        {
            let mut state = ctx.compare_state.lock();
            state.listen_path = Some(previous_listen.to_string_lossy().into_owned());
            state.stt_path = Some(previous_stt.to_string_lossy().into_owned());
        }
        let before = ctx.compare_state.lock().clone();
        let previous_bytes = [
            std::fs::read(&previous_listen).unwrap(),
            std::fs::read(&previous_stt).unwrap(),
        ];
        let new_listen = audio_dir(dir.path()).join(compare_listen_name(3));
        let new_stt = audio_dir(dir.path()).join(compare_stt_name(3));
        let blocked = new_stt.with_extension("wav.tmp");
        std::fs::create_dir(&blocked).unwrap();
        assert!(matches!(
            commit_compare_audio(&ctx, 3, &[0.1; 64], &[0.1; 16]),
            Err(AppError::StorageFailed(_))
        ));
        assert!(!new_listen.exists());
        assert!(!new_listen.with_extension("wav.tmp").exists());
        assert!(!new_stt.exists());
        assert_eq!(*ctx.compare_state.lock(), before);
        assert_eq!(std::fs::read(previous_listen).unwrap(), previous_bytes[0]);
        assert_eq!(std::fs::read(previous_stt).unwrap(), previous_bytes[1]);
    }

    #[test]
    fn compare_commit_collision_does_not_overwrite_either_committed_output() {
        for collision_is_listen in [false, true] {
            let dir = tempfile::tempdir().unwrap();
            let ctx = recording_context(dir.path(), 3);
            let listen = audio_dir(dir.path()).join(compare_listen_name(3));
            let stt = audio_dir(dir.path()).join(compare_stt_name(3));
            let existing = if collision_is_listen { &listen } else { &stt };
            std::fs::write(existing, b"previously committed audio").unwrap();
            let before = ctx.compare_state.lock().clone();
            assert!(matches!(
                commit_compare_audio(&ctx, 3, &[0.1; 64], &[0.1; 16]),
                Err(AppError::StorageFailed(_))
            ));
            assert_eq!(
                std::fs::read(existing).unwrap(),
                b"previously committed audio"
            );
            assert!(!listen.with_extension("wav.tmp").exists());
            assert!(!stt.with_extension("wav.tmp").exists());
            let other = if collision_is_listen { &stt } else { &listen };
            assert!(!other.exists());
            assert_eq!(*ctx.compare_state.lock(), before);
        }
    }

    #[test]
    fn compare_stop_rejects_capture_failure_and_truncation_without_publishing_audio() {
        for mode in 0..3 {
            let dir = tempfile::tempdir().unwrap();
            let ctx = recording_context(dir.path(), 3);
            let raw = audio_dir(dir.path()).join(compare_raw_name(3));
            std::fs::write(&raw, b"capture data").unwrap();
            let before = ctx.compare_state.lock().clone();
            let mut capture = capture_result(raw.clone(), vec![0.1; 4800]);
            let (result, expected) = match mode {
                0 => (
                    Err(AppError::AudioCaptureFailed("worker stopped".into())),
                    AppError::AudioCaptureFailed("worker stopped".into()),
                ),
                1 => {
                    capture.capture_error = Some("device disconnected".into());
                    (
                        Ok(capture),
                        AppError::AudioCaptureFailed("device disconnected".into()),
                    )
                }
                _ => {
                    capture.truncated = true;
                    (Ok(capture), AppError::RecordingTruncated)
                }
            };
            let actual = stop_model_compare_inner(&ctx, 3, result, finished_capture_cleanup(&raw));
            assert_eq!(actual.unwrap_err(), expected);
            assert!(!raw.exists());
            assert_eq!(*ctx.compare_state.lock(), before);
            assert!(!audio_dir(dir.path()).join(compare_listen_name(3)).exists());
        }
    }

    #[test]
    fn stale_compare_stop_discards_its_raw_audio_without_changing_new_take() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = recording_context(dir.path(), 4);
        let new_raw = audio_dir(dir.path()).join(compare_raw_name(4));
        write_pcm16_wav(&new_raw, SAMPLE_RATE, &[0.25; 4800]).unwrap();
        let new_bytes = std::fs::read(&new_raw).unwrap();
        let old_raw = audio_dir(dir.path()).join(compare_raw_name(3));
        write_pcm16_wav(&old_raw, SAMPLE_RATE, &[0.1; 4800]).unwrap();
        let before = ctx.compare_state.lock().clone();
        let result = stop_model_compare_inner(
            &ctx,
            3,
            Ok(capture_result(old_raw.clone(), vec![0.1; 4800])),
            finished_capture_cleanup(&old_raw),
        );
        assert_eq!(result.unwrap_err(), AppError::Cancelled);
        assert!(!old_raw.exists());
        assert_eq!(std::fs::read(new_raw).unwrap(), new_bytes);
        assert_eq!(*ctx.compare_state.lock(), before);
        release_compare_capture_reservation(&ctx, 3);
        assert_eq!(*ctx.compare_state.lock(), before);
        assert!(crate::app::operations::system_busy(&ctx));
    }

    #[test]
    fn compare_stop_recovers_samples_from_wav_and_publishes_both_audio_rates() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = recording_context(dir.path(), 3);
        let raw = audio_dir(dir.path()).join(compare_raw_name(3));
        let input: Vec<f32> = (0..4800)
            .map(|index| (index as f32 * 0.1).sin() * 0.2)
            .collect();
        write_pcm16_wav(&raw, SAMPLE_RATE, &input).unwrap();
        let raw_bytes = std::fs::read(&raw).unwrap();
        let snapshot = stop_model_compare_inner(
            &ctx,
            3,
            Ok(capture_result(raw.clone(), Vec::new())),
            finished_capture_cleanup(&raw),
        )
        .unwrap();
        assert!(!snapshot.recording);
        assert_eq!(snapshot.nonce, 3);
        assert_eq!(*ctx.compare_state.lock(), snapshot);
        let (listen, listen_rate) =
            read_pcm16_wav_with_rate(Path::new(snapshot.listen_path.as_ref().unwrap())).unwrap();
        let (stt, stt_rate) =
            read_pcm16_wav_with_rate(Path::new(snapshot.stt_path.as_ref().unwrap())).unwrap();
        assert_eq!(listen_rate, SAMPLE_RATE);
        assert_eq!(stt_rate, STT_SAMPLE_RATE);
        assert_eq!(listen.len(), input.len());
        assert_eq!(
            stt.len(),
            input.len() * STT_SAMPLE_RATE as usize / SAMPLE_RATE as usize
        );
        assert!(listen.iter().any(|sample| sample.abs() > 0.001));
        assert!(stt.iter().all(|sample| sample.is_finite()));
        assert_eq!(std::fs::read(raw).unwrap(), raw_bytes);
        assert!(!crate::app::operations::system_busy(&ctx));
    }

    #[test]
    fn compare_dsp_failure_preserves_previous_clip_and_reservation() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = recording_context(dir.path(), 3);
        let previous = audio_dir(dir.path()).join(compare_listen_name(2));
        write_pcm16_wav(&previous, SAMPLE_RATE, &[0.25; 64]).unwrap();
        ctx.compare_state.lock().listen_path = Some(previous.to_string_lossy().into_owned());
        let before = ctx.compare_state.lock().clone();
        let previous_bytes = std::fs::read(&previous).unwrap();
        let error = AppError::AudioProcessingFailed("DSP rejected input".into());
        assert_eq!(
            process_compare_audio(
                &ctx,
                3,
                vec![0.1; 4800],
                SAMPLE_RATE,
                |_| Err(error.clone())
            )
            .unwrap_err(),
            error
        );
        assert_eq!(*ctx.compare_state.lock(), before);
        assert_eq!(std::fs::read(previous).unwrap(), previous_bytes);
        assert!(crate::app::operations::system_busy(&ctx));
        assert!(!audio_dir(dir.path()).join(compare_listen_name(3)).exists());
    }

    #[test]
    fn compare_commit_rejects_shutdown_and_released_recording_before_any_write() {
        for shutdown in [false, true] {
            let dir = tempfile::tempdir().unwrap();
            let ctx = recording_context(dir.path(), 3);
            if shutdown {
                ctx.shutdown_requested
                    .store(true, std::sync::atomic::Ordering::SeqCst);
            } else {
                release_compare_capture_reservation(&ctx, 3);
            }
            let before = ctx.compare_state.lock().clone();
            assert_eq!(
                commit_compare_audio(&ctx, 3, &[0.1; 64], &[0.1; 16]).unwrap_err(),
                AppError::Cancelled
            );
            assert_eq!(*ctx.compare_state.lock(), before);
            assert!(!audio_dir(dir.path()).join(compare_listen_name(3)).exists());
            assert!(!audio_dir(dir.path()).join(compare_stt_name(3)).exists());
        }
    }

    #[test]
    fn compare_cancellation_changes_only_running_slots_and_retains_lease() {
        let slot = |status: &str| CompareSlot {
            slot_id: format!("slot-{status}"),
            model: "model-a".into(),
            status: status.into(),
            text: Some("saved result".into()),
            error: Some("saved error".into()),
            attempt: 2,
            cost: Some(0.002),
            latency_ms: Some(125),
            clip_nonce: 3,
            run_id: Some("run-3".into()),
        };
        let mut state = CompareState {
            running: true,
            nonce: 3,
            run_id: Some("run-3".into()),
            slots: ["running", "done", "error", "cancelled"].map(slot).to_vec(),
            ..Default::default()
        };
        let before = state.clone();
        mark_compare_cancelled(&mut state);
        assert!(state.running);
        assert_eq!(state.run_id, before.run_id);
        let mut expected_running = before.slots[0].clone();
        expected_running.status = "cancelled".into();
        assert_eq!(state.slots[0], expected_running);
        assert_eq!(state.slots[1..], before.slots[1..]);
        let cancelled = state.clone();
        mark_compare_cancelled(&mut state);
        assert_eq!(state, cancelled);
    }

    #[test]
    fn compare_admission_rejects_other_operations_without_replacing_cancel_signal() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = AppContext::initialize(dir.path().to_path_buf()).unwrap();
        let original = reserve_compare_run(&ctx).unwrap();
        assert!(compare_busy(&ctx));
        cancel_model_compare_internal(&ctx);
        assert_eq!(
            reserve_compare_run(&ctx).unwrap_err(),
            AppError::TranscriptionInProgress
        );
        assert!(*original.borrow());
        assert!(*ctx.compare_cancel.lock().as_ref().unwrap().borrow());
        finish_compare_run(&ctx);
        assert!(!compare_busy(&ctx));
        assert!(ctx.compare_cancel.lock().is_none());
        ctx.filter_recording
            .store(true, std::sync::atomic::Ordering::SeqCst);
        assert_eq!(
            reserve_compare_run(&ctx).unwrap_err(),
            AppError::TranscriptionInProgress
        );
        assert!(ctx.compare_cancel.lock().is_none());
        assert!(!compare_busy(&ctx));
        ctx.filter_recording
            .store(false, std::sync::atomic::Ordering::SeqCst);
        ctx.shutdown_requested
            .store(true, std::sync::atomic::Ordering::SeqCst);
        assert_eq!(
            reserve_compare_run(&ctx).unwrap_err(),
            AppError::TranscriptionInProgress
        );
        assert!(ctx.compare_cancel.lock().is_none());
    }

    #[test]
    fn compare_retry_policy_caps_cost_and_deadline_without_changing_request_settings() {
        for additional_retries in [0, 1, 10] {
            let policy = RetryPolicy {
                additional_retries,
                connect_timeout: Duration::from_secs(4),
                request_timeout: Duration::from_secs(17),
                initial_retry_delay: Duration::from_millis(123),
                max_retry_delay: Duration::from_secs(2),
                ..RetryPolicy::default()
            };
            let actual = compare_retry_policy(policy.clone());
            let expected = RetryPolicy {
                additional_retries: additional_retries.min(1),
                total_operation_timeout: Duration::from_secs(90),
                ..policy
            };
            assert_eq!(actual, expected);
            assert!(actual.max_attempts() <= 2);
        }
        let disabled = compare_retry_policy(RetryPolicy {
            automatic_retries: false,
            ..RetryPolicy::default()
        });
        assert_eq!(disabled.max_attempts(), 1);
    }

    #[test]
    fn compare_cleanup_handles_missing_and_unreadable_root_and_preserves_samples() {
        let dir = tempfile::tempdir().unwrap();
        assert!(remove_compare_artifacts(&dir.path().join("absent")).is_empty());
        let not_directory = dir.path().join("not-directory");
        std::fs::write(&not_directory, b"file").unwrap();
        assert_eq!(
            remove_compare_artifacts(&not_directory),
            vec![not_directory.to_string_lossy().into_owned()]
        );
        for name in [
            "filter-sample.wav",
            "filter-preview.wav",
            "dictation.raw.wav",
        ] {
            std::fs::write(dir.path().join(name), name.as_bytes()).unwrap();
        }
        std::fs::write(
            dir.path().join("model-compare.run.stt.wav.tmp"),
            b"temporary",
        )
        .unwrap();
        assert!(remove_compare_artifacts(dir.path()).is_empty());
        assert!(!dir.path().join("model-compare.run.stt.wav.tmp").exists());
        for name in [
            "filter-sample.wav",
            "filter-preview.wav",
            "dictation.raw.wav",
        ] {
            assert_eq!(
                std::fs::read(dir.path().join(name)).unwrap(),
                name.as_bytes()
            );
        }
    }

    #[test]
    fn compare_model_validation_counts_nonempty_entries_and_rejects_trimmed_duplicates() {
        let models = vec![
            "  ".into(),
            " provider/a ".into(),
            "\t".into(),
            "provider/b".into(),
        ];
        assert_eq!(
            validate_compare_models(&models).unwrap(),
            ["provider/a", "provider/b"]
        );
        assert!(matches!(
            validate_compare_models(&["provider/a".into(), " provider/a ".into()]),
            Err(AppError::RequestValidationFailed(_))
        ));
        assert!(validate_compare_models(&[" ".into(), "provider/a".into()]).is_err());
        let maximum: Vec<String> = (0..12).map(|index| format!("provider/{index}")).collect();
        assert_eq!(validate_compare_models(&maximum).unwrap(), maximum);
    }

    #[test]
    fn compare_processing_keeps_reservation_until_audio_is_committed() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = Arc::new(AppContext::initialize(dir.path().to_path_buf()).unwrap());
        {
            let mut state = ctx.compare_state.lock();
            state.nonce = 7;
            state.recording = true;
        }
        let processing = Arc::new(std::sync::Barrier::new(2));
        let finish = Arc::new(std::sync::Barrier::new(2));
        let worker_ctx = ctx.clone();
        let worker_processing = processing.clone();
        let worker_finish = finish.clone();
        let worker = std::thread::spawn(move || {
            process_compare_audio(&worker_ctx, 7, vec![0.1; 4800], SAMPLE_RATE, |pcm| {
                worker_processing.wait();
                worker_finish.wait();
                Ok(pcm)
            })
        });
        processing.wait();
        assert!(ctx.compare_state.lock().recording);
        assert!(crate::app::operations::system_busy(&ctx));
        assert!(matches!(
            reserve_compare_run(&ctx),
            Err(AppError::TranscriptionInProgress)
        ));
        finish.wait();
        let snapshot = worker.join().unwrap().unwrap();
        assert!(!snapshot.recording);
        assert_eq!(snapshot.nonce, 7);
        let (listen, _) =
            read_pcm16_wav_with_rate(Path::new(snapshot.listen_path.as_ref().unwrap())).unwrap();
        assert_eq!(listen.len(), 4800);
        assert!(!crate::app::operations::system_busy(&ctx));
    }

    #[test]
    fn stale_compare_processing_cannot_write_or_release_a_new_take() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = Arc::new(AppContext::initialize(dir.path().to_path_buf()).unwrap());
        {
            let mut state = ctx.compare_state.lock();
            state.nonce = 7;
            state.recording = true;
        }
        let processing = Arc::new(std::sync::Barrier::new(2));
        let finish = Arc::new(std::sync::Barrier::new(2));
        let worker_ctx = ctx.clone();
        let worker_processing = processing.clone();
        let worker_finish = finish.clone();
        let worker = std::thread::spawn(move || {
            let result =
                process_compare_audio(&worker_ctx, 7, vec![0.1; 4800], SAMPLE_RATE, |pcm| {
                    worker_processing.wait();
                    worker_finish.wait();
                    Ok(pcm)
                });
            release_compare_capture_reservation(&worker_ctx, 7);
            result
        });
        processing.wait();
        let new_audio = audio_dir(dir.path()).join(compare_listen_name(8));
        write_pcm16_wav(&new_audio, SAMPLE_RATE, &[0.25; 64]).unwrap();
        let expected_audio = std::fs::read(&new_audio).unwrap();
        {
            let _admission = crate::app::operations::lock_admission(&ctx);
            let mut state = ctx.compare_state.lock();
            state.nonce = 8;
            state.listen_path = Some(new_audio.to_string_lossy().into_owned());
        }
        let expected_state = ctx.compare_state.lock().clone();
        finish.wait();
        assert_eq!(worker.join().unwrap().unwrap_err(), AppError::Cancelled);
        assert_eq!(*ctx.compare_state.lock(), expected_state);
        assert_eq!(std::fs::read(new_audio).unwrap(), expected_audio);
        assert!(!audio_dir(dir.path()).join(compare_listen_name(7)).exists());
        assert!(!audio_dir(dir.path()).join(compare_stt_name(7)).exists());
    }

    #[test]
    fn timed_out_compare_keeps_busy_reservation_until_its_capture_finishes() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = AppContext::initialize(dir.path().to_path_buf()).unwrap();
        {
            let mut state = ctx.compare_state.lock();
            state.nonce = 7;
            state.recording = true;
        }

        release_compare_capture_reservation(&ctx, 6);
        assert!(ctx.compare_state.lock().recording);
        assert!(crate::app::operations::system_busy(&ctx));

        release_compare_capture_reservation(&ctx, 7);
        assert!(!ctx.compare_state.lock().recording);
        assert!(!crate::app::operations::system_busy(&ctx));
    }

    #[test]
    fn compare_timeout_keeps_busy_until_late_native_capture_and_discards_its_audio() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = Arc::new(AppContext::initialize(dir.path().to_path_buf()).unwrap());
        {
            let mut state = ctx.compare_state.lock();
            state.nonce = 7;
            state.recording = true;
        }
        let path = dir.path().join("late-compare.wav");
        let (release, wait) = std::sync::mpsc::channel();
        let worker_path = path.clone();
        let worker = std::thread::spawn(move || {
            wait.recv().unwrap();
            std::fs::write(&worker_path, b"late compare sample").unwrap();
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
        let late_handler =
            compare_late_result_handler(ctx.clone(), 7, session.cleanup_handle(), None);
        let outcome = session
            .stop_with_late_result_timeout(Duration::from_millis(20), move |_| late_handler());
        assert!(matches!(outcome, CaptureStopOutcome::TimedOut(_)));
        assert!(ctx.compare_state.lock().recording);
        assert!(crate::app::operations::system_busy(&ctx));

        release.send(()).unwrap();
        let deadline = std::time::Instant::now() + Duration::from_secs(2);
        while ctx.compare_state.lock().recording && std::time::Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(5));
        }
        assert!(!ctx.compare_state.lock().recording);
        assert!(!crate::app::operations::system_busy(&ctx));
        assert!(!path.exists());
    }

    #[test]
    fn cancelled_run_keeps_lease_until_old_worker_finishes() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = Arc::new(AppContext::initialize(dir.path().to_path_buf()).unwrap());
        reserve_compare_run(&ctx).unwrap();
        ctx.compare_state.lock().running = true;
        mark_compare_cancelled(&mut ctx.compare_state.lock());
        let barrier = Arc::new(std::sync::Barrier::new(2));
        let worker_barrier = barrier.clone();
        let worker_ctx = ctx.clone();
        let old_worker = std::thread::spawn(move || {
            worker_barrier.wait();
            finish_compare_run(&worker_ctx);
        });
        assert!(matches!(
            reserve_compare_run(&ctx),
            Err(AppError::TranscriptionInProgress)
        ));
        assert!(ctx.compare_state.lock().running);
        barrier.wait();
        old_worker.join().unwrap();
        reserve_compare_run(&ctx).unwrap();
        assert!(reserve_compare_run(&ctx).is_err());
        finish_compare_run(&ctx);
        assert!(!crate::app::operations::system_busy(&ctx));
    }

    #[test]
    fn compare_cleanup_removes_audio_and_reports_failures_without_touching_history() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join(compare_listen_name(1)), b"audio").unwrap();
        std::fs::write(dir.path().join("history.raw.wav"), b"history").unwrap();
        std::fs::create_dir(dir.path().join("model-compare.locked.wav")).unwrap();
        let failures = remove_compare_artifacts(dir.path());
        assert!(!dir.path().join(compare_listen_name(1)).exists());
        assert_eq!(
            std::fs::read(dir.path().join("history.raw.wav")).unwrap(),
            b"history"
        );
        assert_eq!(failures.len(), 1);
        assert!(failures[0].ends_with("model-compare.locked.wav"));
    }

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
        .is_ok());
        assert!(validate_compare_models(
            &(0..13).map(|index| format!("m{index}")).collect::<Vec<_>>()
        )
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
        assert!(!keep_compare_wav("model-compare.stt.wav"));
        assert!(!keep_compare_wav("model-compare.abc.stt.wav"));
        assert!(!keep_compare_wav("orphan.wav"));
        assert!(!keep_compare_wav(&compare_listen_name(9)));
        assert!(crate::history::retention::keep_compare_temp_wav(
            &compare_listen_name(9)
        ));
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
