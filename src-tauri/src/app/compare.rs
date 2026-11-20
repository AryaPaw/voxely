use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager};

use crate::app::machine::is_cancellable;
use crate::app::session::{stop_meter_capture, take_meter_monitor_for_release, AppContext};
use crate::audio::capture::{
    capture_wav_duration_ms, read_pcm16_wav_with_rate, validate_capture_result, write_pcm16_wav,
    CaptureSession, CaptureStopOutcome,
};
use crate::dsp::metrics::{SAMPLE_RATE, STT_SAMPLE_RATE};
use crate::dsp::pipeline::{prepare_listen_audio, prepare_stt_audio};
use crate::error::AppError;
use crate::history::repository::{audio_dir, ApiAttemptUsage};
use crate::transcription::openrouter::{
    transcribe_file_with_progress, SttProgress, TranscriptionSuccess,
};
use crate::transcription::retry::RetryPolicy;
use crate::windows_int::credentials::get_api_key;

const MAX_COMPARE_MODELS: usize = 12;
const MAX_COMPARE_ADDITIONAL_RETRIES: u32 = 1;
// The capture owner crosses IPC as a JavaScript number. Every admitted nonce
// must remain exact there, including when startup restores it from filenames.
const MAX_COMPARE_NONCE: u64 = 9_007_199_254_740_991;

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
    pub revision: u64,
    pub recording: bool,
    pub running: bool,
    pub nonce: u64,
    pub listen_path: Option<String>,
    pub stt_path: Option<String>,
    pub run_id: Option<String>,
    pub slots: Vec<CompareSlot>,
}

impl CompareState {
    fn next_nonce(&self) -> Result<u64, AppError> {
        self.nonce
            .checked_add(1)
            .filter(|nonce| *nonce <= MAX_COMPARE_NONCE)
            .ok_or_else(|| {
                AppError::StorageFailed("compare recording generation is exhausted".into())
            })
    }

    fn snapshot(&mut self) -> Self {
        self.revision = self.revision.saturating_add(1);
        self.clone()
    }

    pub(crate) fn reset(&mut self) {
        *self = Self {
            revision: self.revision,
            nonce: self.nonce,
            ..Self::default()
        };
    }
}

pub(crate) fn initial_compare_nonce(audio_root: &Path) -> Result<u64, AppError> {
    let mut maximum = 0;
    for entry in
        std::fs::read_dir(audio_root).map_err(|error| AppError::StorageFailed(error.to_string()))?
    {
        let entry = entry.map_err(|error| AppError::StorageFailed(error.to_string()))?;
        let name = entry.file_name();
        let Some((number, kind)) = name
            .to_str()
            .and_then(|name| name.strip_prefix("model-compare."))
            .and_then(|name| name.split_once('.'))
        else {
            continue;
        };
        if !matches!(
            kind,
            "raw.wav" | "listen.wav" | "stt.wav" | "raw.wav.tmp" | "listen.wav.tmp" | "stt.wav.tmp"
        ) {
            continue;
        }
        // UUID run copies and unknown/handwritten formats don't own this clock.
        if let Ok(nonce) = number.parse::<u64>() {
            if nonce > 0 && nonce.to_string() == number {
                maximum = maximum.max(nonce);
            }
        }
    }
    Ok(maximum)
}

pub(crate) fn compare_snapshot(ctx: &AppContext) -> CompareState {
    ctx.compare_state.lock().snapshot()
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
    if !(2..=MAX_COMPARE_MODELS).contains(&trimmed.len()) {
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

pub(crate) fn emit_compare(app: &AppHandle) {
    let ctx = app.state::<Arc<AppContext>>();
    let state = compare_snapshot(&ctx);
    let _ = app.emit_to("main", "compare://state", state);
}

pub fn start_model_compare(app: &AppHandle) -> Result<u64, AppError> {
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
    let nonce = ctx.compare_state.lock().next_nonce()?;
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
    let previous_meter = take_meter_monitor_for_release(&ctx);
    let late_ctx = ctx.inner().clone();
    let late_app = app.clone();
    // The pending native-start reservation protects the device while the
    // worker waits. Neither the driver nor monitor shutdown owns UI admission.
    let started = crate::app::operations::with_admission_released(_admission, || {
        let ready = previous_meter
            .map(|session| stop_meter_capture(&ctx, session))
            .unwrap_or(Ok(()))
            .and_then(|()| {
                if compare_start_is_current(&ctx, nonce) {
                    Ok(())
                } else {
                    Err(AppError::Cancelled)
                }
            })
            .map_err(|error| crate::audio::capture::CaptureStartFailure {
                error,
                native_worker_pending: false,
            });
        ready.and_then(|()| {
            CaptureSession::start_with_late_completion(device.as_deref(), dest, move || {
                let _admission = crate::app::operations::lock_admission(&late_ctx);
                release_compare_capture_reservation(&late_ctx, nonce);
                crate::app::session::release_pending_native_capture_start(&late_ctx);
                drop(_admission);
                emit_compare(&late_app);
                crate::app::session::schedule_deferred_shutdown_exit(&late_app, &late_ctx);
            })
        })
    });
    let native_worker_pending = started
        .as_ref()
        .err()
        .is_some_and(|failure| failure.native_worker_pending);
    let session = match started {
        Ok(session) => session,
        Err(failure) => {
            let _admission = crate::app::operations::lock_admission(&ctx);
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
    commit_compare_capture_start(&ctx, nonce, session, |session| {
        let done_ctx = ctx.inner().clone();
        let done_app = app.clone();
        crate::app::session::stop_capture_discard_during_shutdown(
            ctx.inner().clone(),
            app.clone(),
            session,
            move || {
                release_compare_capture_reservation(&done_ctx, nonce);
                crate::app::session::release_pending_native_capture_start(&done_ctx);
                emit_compare(&done_app);
            },
        );
    })?;
    crate::app::session::schedule_deferred_shutdown_exit(app, &ctx);
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
                    if let Err(err) = stop_model_compare(&watcher_app, Some(nonce)) {
                        let _ = watcher_app.emit_to("main", "compare://error", err.code());
                    }
                    break;
                }
                None => break,
                _ => {}
            }
        }
    });
    // A delayed reply still belongs to the exact take admitted above. It must
    // never borrow a later take's nonce from the current shared state.
    Ok(nonce)
}

fn compare_start_is_current(ctx: &AppContext, nonce: u64) -> bool {
    let state = ctx.compare_state.lock();
    state.nonce == nonce
        && state.recording
        && !ctx
            .shutdown_requested
            .load(std::sync::atomic::Ordering::SeqCst)
}

fn commit_compare_capture_start(
    ctx: &AppContext,
    nonce: u64,
    session: CaptureSession,
    discard: impl FnOnce(CaptureSession),
) -> Result<(), AppError> {
    let admission = crate::app::operations::lock_admission(ctx);
    if !compare_start_is_current(ctx, nonce) {
        // The discard callback retains the pending native-start reservation
        // until the real worker has stopped, including a late result.
        drop(admission);
        discard(session);
        return Err(AppError::Cancelled);
    }
    *ctx.compare_capture.lock() = Some(session);
    crate::app::session::release_pending_native_capture_start(ctx);
    Ok(())
}

fn take_compare_capture_for_stop(
    ctx: &AppContext,
    expected_nonce: Option<u64>,
) -> Option<(CaptureSession, u64)> {
    let _admission = crate::app::operations::lock_admission(ctx);
    let nonce = ctx.compare_state.lock().nonce;
    if expected_nonce.is_some_and(|expected| expected != nonce) {
        return None;
    }
    let session = ctx.compare_capture.lock().take();
    if session.is_none()
        && ctx
            .pending_native_capture_starts
            .load(std::sync::atomic::Ordering::SeqCst)
            > 0
    {
        // The device can still be opening. Withdraw this take's publication
        // intent, but retain its pending reservation until native cleanup ends.
        // A repeated stop during an already-owned stop worker must not release
        // that worker's reservation or admit a concurrent device capture.
        ctx.compare_state.lock().recording = false;
    }
    session.map(|session| (session, nonce))
}

pub fn stop_model_compare(
    app: &AppHandle,
    expected_nonce: Option<u64>,
) -> Result<CompareState, AppError> {
    let ctx = app.state::<Arc<AppContext>>();
    let Some((session, nonce)) = take_compare_capture_for_stop(&ctx, expected_nonce) else {
        emit_compare(app);
        crate::app::session::schedule_deferred_shutdown_exit(app, &ctx);
        return Ok(compare_snapshot(&ctx));
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
    let silence = ctx.settings.lock().silence_compression.clone();
    let stt = prepare_stt_audio(&listen, rate, &silence)?;
    commit_compare_audio(ctx, nonce, &listen, &stt)
}

fn commit_compare_audio(
    ctx: &AppContext,
    nonce: u64,
    listen: &[f32],
    stt: &[f32],
) -> Result<CompareState, AppError> {
    commit_compare_audio_with(ctx, nonce, |listen_path, stt_path| {
        write_pcm16_wav(listen_path, SAMPLE_RATE, listen)?;
        write_pcm16_wav(stt_path, STT_SAMPLE_RATE, stt)
    })
}

fn commit_compare_audio_with(
    ctx: &AppContext,
    nonce: u64,
    write: impl FnOnce(&Path, &Path) -> Result<(), AppError>,
) -> Result<CompareState, AppError> {
    let audio = audio_dir(&ctx.data_dir);
    let listen_path = audio.join(compare_listen_name(nonce));
    let stt_path = audio.join(compare_stt_name(nonce));
    let owned_paths = [
        listen_path.clone(),
        listen_path.with_extension("wav.tmp"),
        stt_path.clone(),
        stt_path.with_extension("wav.tmp"),
    ];
    let reservation = {
        let _admission = crate::app::operations::lock_admission(ctx);
        if !compare_start_is_current(ctx, nonce) {
            return Err(AppError::Cancelled);
        }
        crate::app::operations::AudioWriteReservation::reserve(
            ctx,
            owned_paths.iter().map(|path| {
                path.file_name()
                    .expect("generated compare name")
                    .to_string_lossy()
                    .into_owned()
            }),
        )?
    };
    // Each take owns fresh paths. Refuse a collision before writing either output.
    // Clear preserves the nonce and a pending writer exclusively reserves all
    // four names, so a stale completion can never own a newer take's files.
    for path in &owned_paths {
        if path
            .try_exists()
            .map_err(|err| AppError::StorageFailed(err.to_string()))?
        {
            return Err(AppError::StorageFailed(
                "compare output already exists".into(),
            ));
        }
    }
    let written = write(&listen_path, &stt_path);
    let published = match written {
        Ok(()) => {
            let _admission = crate::app::operations::lock_admission(ctx);
            let mut state = ctx.compare_state.lock();
            if state.nonce != nonce
                || !state.recording
                || ctx
                    .shutdown_requested
                    .load(std::sync::atomic::Ordering::SeqCst)
            {
                Err(AppError::Cancelled)
            } else {
                state.recording = false;
                state.listen_path = Some(listen_path.to_string_lossy().into_owned());
                state.stt_path = Some(stt_path.to_string_lossy().into_owned());
                Ok(state.snapshot())
            }
        }
        Err(error) => Err(error),
    };
    if published.is_err() {
        for path in owned_paths {
            if let Err(cleanup_error) = std::fs::remove_file(&path) {
                if cleanup_error.kind() != std::io::ErrorKind::NotFound {
                    tracing::warn!(error = %cleanup_error, file = %path.display(), "remove uncommitted compare output");
                }
            }
        }
    }
    drop(reservation);
    published
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
    compare_snapshot(&app.state::<Arc<AppContext>>())
}

pub fn clear_model_compare(app: &AppHandle) -> Result<(), AppError> {
    let ctx = app.state::<Arc<AppContext>>();
    let result = clear_model_compare_inner(&ctx, remove_compare_artifacts);
    emit_compare(app);
    crate::app::session::schedule_deferred_shutdown_exit(app, &ctx);
    result
}

fn clear_model_compare_inner(
    ctx: &AppContext,
    remove: impl FnOnce(&Path) -> Vec<String>,
) -> Result<(), AppError> {
    {
        let _admission = crate::app::operations::lock_admission(ctx);
        let mut state = ctx.compare_state.lock();
        if state.recording
            || ctx
                .compare_running
                .load(std::sync::atomic::Ordering::SeqCst)
            || ctx
                .pending_audio_writes
                .lock()
                .iter()
                .any(|name| name.starts_with("model-compare."))
        {
            return Err(AppError::TranscriptionInProgress);
        }
        if let Some(tx) = ctx.compare_cancel.lock().as_ref() {
            tx.send_replace(true);
        }
        // Keep an operation reservation while retiring files outside the UI
        // gate. No capture/run can publish a future generation into this set.
        ctx.compare_running
            .store(true, std::sync::atomic::Ordering::SeqCst);
        state.reset();
    }
    let failed = remove(&audio_dir(&ctx.data_dir));
    ctx.compare_running
        .store(false, std::sync::atomic::Ordering::SeqCst);
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
                std::fs::remove_file(entry.path()).err().and_then(|error| {
                    (error.kind() != std::io::ErrorKind::NotFound)
                        .then(|| entry.path().to_string_lossy().into_owned())
                })
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
    let snapshot = compare_snapshot(&ctx);
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
    crate::app::session::ensure_compare_usage_capacity(
        ctx,
        MAX_COMPARE_MODELS * (1 + MAX_COMPARE_ADDITIONAL_RETRIES) as usize,
    )?;
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
    ctx.emit_history(&app);
    result
}

async fn run_compare_inner(
    app: AppHandle,
    tx: tokio::sync::watch::Sender<bool>,
) -> Result<CompareState, AppError> {
    let ctx = app.state::<Arc<AppContext>>();
    let settings = ctx.settings.lock().clone();
    if !settings.first_run_complete {
        return Err(AppError::DisclosureRequired);
    }
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
    let usage_generation = ctx.history.lock().compare_usage_generation()?;
    let preparation_reservation = {
        let _admission = crate::app::operations::lock_admission(&ctx);
        if ctx
            .shutdown_requested
            .load(std::sync::atomic::Ordering::SeqCst)
        {
            return Err(AppError::Cancelled);
        }
        crate::app::operations::AudioWriteReservation::reserve(
            &ctx,
            [format!("model-compare.{run_id}.stt.wav")],
        )?
    };
    let preparation_path = frozen.clone();
    let preparation_run_id = run_id.clone();
    let audio_duration = tokio::task::spawn_blocking(move || {
        frozen_stt_copy(
            preparation_path.parent().expect("generated compare path"),
            &preparation_run_id,
            Path::new(&stt_src),
        )?;
        match capture_wav_duration_ms(&preparation_path) {
            Ok(duration) => Ok(Duration::from_millis(duration)),
            Err(error) => {
                let _ = std::fs::remove_file(&preparation_path);
                Err(error)
            }
        }
    })
    .await
    .map_err(|_| AppError::StorageFailed("compare audio preparation worker failed".into()))??;
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
    drop(preparation_reservation);
    emit_compare(&app);
    let policy = compare_retry_policy(settings.retry.to_policy());
    let language = if settings.language == "auto" {
        None
    } else {
        Some(settings.language.clone())
    };
    let base = crate::transcription::openrouter::default_base_url().to_string();
    let client = ctx.clone_transport_client()?;
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
        let run_id = run_id.clone();
        let generation = usage_generation.clone();
        let request_ctx = ctx.inner().clone();
        let cancel = tx.clone();
        set.spawn(async move {
            let mut persistence_error = None;
            let outcome = transcribe_file_with_progress(
                &client,
                &base,
                &key,
                &model,
                language.as_deref(),
                &frozen,
                policy,
                audio_duration,
                rx,
                |progress| {
                    if let Some(usage) = compare_attempt_usage(&run_id, &slot_id, &model, &progress)
                    {
                        if let Err(error) =
                            persist_compare_usage(&request_ctx, usage, &generation, &cancel)
                        {
                            persistence_error.get_or_insert(error);
                        }
                    }
                },
            )
            .await;
            (slot_id, model, outcome, persistence_error)
        });
    }
    let mut persistence_error = None;
    while let Some(joined) = set.join_next().await {
        match joined {
            Ok((slot_id, model, outcome, storage_error)) => {
                if let Some(error) = storage_error {
                    persistence_error.get_or_insert(error);
                }
                match outcome {
                    Ok(success) => apply_slot_success(&app, &slot_id, &model, success),
                    Err(error) => apply_slot_error(&app, &slot_id, &model, error),
                }
            }
            Err(err) => {
                tracing::error!(error = %err, "compare join failed");
            }
        }
        emit_compare(&app);
    }
    let mut state = ctx.compare_state.lock();
    state.running = false;
    let snapshot = state.snapshot();
    drop(state);
    emit_compare(&app);
    // Do not silently discard a confirmed price after a transient SQLite fault.
    // Stable IDs make this recovery idempotent; full clear invalidates its epoch.
    finish_compare_usage(&ctx, &format!("compare:{run_id}"), persistence_error)?;
    Ok(snapshot)
}

fn persist_compare_usage(
    ctx: &AppContext,
    usage: ApiAttemptUsage,
    generation: &str,
    cancel: &tokio::sync::watch::Sender<bool>,
) -> Result<(), AppError> {
    let persisted = crate::app::session::persist_compare_attempt(ctx, usage, generation);
    if let Err(error) = &persisted {
        tracing::error!(%error, "persist compare API usage failed; cancelling remaining requests");
        cancel.send_replace(true);
    }
    persisted
}

fn finish_compare_usage(
    ctx: &AppContext,
    operation_id: &str,
    persistence_error: Option<AppError>,
) -> Result<(), AppError> {
    crate::app::session::flush_pending_attempts(ctx);
    if crate::app::session::compare_usage_is_pending(ctx, operation_id) {
        return Err(persistence_error.unwrap_or_else(|| {
            AppError::StorageFailed("compare usage recovery is pending".into())
        }));
    }
    Ok(())
}

fn compare_attempt_usage(
    run_id: &str,
    slot_id: &str,
    model: &str,
    progress: &SttProgress,
) -> Option<ApiAttemptUsage> {
    let (attempt, cost_usd) = match progress {
        SttProgress::Attempt(attempt) => (*attempt, None),
        SttProgress::Finished {
            attempt, cost_usd, ..
        } => (*attempt, *cost_usd),
        SttProgress::Waiting { .. } => return None,
    };
    Some(ApiAttemptUsage {
        operation_id: format!("compare:{run_id}"),
        attempt_id: format!("compare:{run_id}:{slot_id}:{attempt}"),
        occurred_at: chrono::Utc::now(),
        model: model.to_owned(),
        cost_usd,
    })
}

fn compare_retry_policy(mut policy: RetryPolicy) -> RetryPolicy {
    policy.additional_retries = policy
        .additional_retries
        .min(MAX_COMPARE_ADDITIONAL_RETRIES);
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
    // A failed create owns nothing. Never overwrite or remove an earlier run.
    let mut output = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&dest)
        .map_err(|error| AppError::StorageFailed(error.to_string()))?;
    let copied = (|| {
        let mut input =
            std::fs::File::open(src).map_err(|error| AppError::StorageFailed(error.to_string()))?;
        std::io::copy(&mut input, &mut output)
            .map_err(|error| AppError::StorageFailed(error.to_string()))?;
        Ok(())
    })();
    drop(output);
    if let Err(error) = copied {
        let _ = std::fs::remove_file(&dest);
        return Err(error);
    }
    Ok(dest)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audio::capture::{CaptureCleanupHandle, CaptureResult};
    use crate::transcription::openrouter::transcribe_file;
    use httpmock::prelude::*;
    use std::time::Duration;

    #[test]
    fn delayed_compare_stop_cannot_take_a_newer_capture_after_the_observed_one_finishes() {
        let directory = tempfile::tempdir().unwrap();
        let ctx = Arc::new(recording_context(directory.path(), 1));
        let first_path = audio_dir(directory.path()).join(compare_raw_name(1));
        write_pcm16_wav(&first_path, SAMPLE_RATE, &[0.1; 4800]).unwrap();
        let result_path = first_path.clone();
        let first = CaptureSession::from_test_worker(
            std::thread::spawn(move || Ok(capture_result(result_path, vec![0.1; 4800]))),
            first_path,
        );
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        while !first.is_finished() && std::time::Instant::now() < deadline {
            std::thread::yield_now();
        }
        assert!(first.is_finished());
        *ctx.compare_capture.lock() = Some(first);
        let (observed, ready) = std::sync::mpsc::channel();
        let (release, waiting) = std::sync::mpsc::channel();
        let watcher_ctx = Arc::clone(&ctx);
        let watcher = std::thread::spawn(move || {
            let nonce = watcher_ctx.compare_state.lock().nonce;
            assert!(watcher_ctx
                .compare_capture
                .lock()
                .as_ref()
                .unwrap()
                .is_finished());
            observed.send(()).unwrap();
            waiting.recv_timeout(Duration::from_secs(5)).unwrap();
            take_compare_capture_for_stop(&watcher_ctx, Some(nonce))
        });
        ready.recv_timeout(Duration::from_secs(5)).unwrap();
        let (first, nonce) = take_compare_capture_for_stop(&ctx, Some(1)).unwrap();
        first.stop().unwrap();
        release_compare_capture_reservation(&ctx, nonce);

        let second_path = audio_dir(directory.path()).join(compare_raw_name(2));
        write_pcm16_wav(&second_path, SAMPLE_RATE, &[0.25; 4800]).unwrap();
        let original = std::fs::read(&second_path).unwrap();
        let (finish_second, native_wait) = std::sync::mpsc::channel();
        let result_path = second_path.clone();
        let second = CaptureSession::from_test_worker(
            std::thread::spawn(move || {
                native_wait.recv_timeout(Duration::from_secs(5)).unwrap();
                Ok(capture_result(result_path, vec![0.25; 4800]))
            }),
            second_path.clone(),
        );
        {
            let _admission = crate::app::operations::lock_admission(&ctx);
            ctx.compare_state.lock().nonce = 2;
            ctx.compare_state.lock().recording = true;
            *ctx.compare_capture.lock() = Some(second);
        }
        release.send(()).unwrap();
        let late_stop = watcher.join().unwrap();
        let stale_stop_was_inert = late_stop.is_none();
        let newer_still_owned = ctx.compare_capture.lock().is_some();
        let newer_still_recording = ctx.compare_state.lock().recording;
        // Release the synthetic native worker even for a broken implementation.
        let second = late_stop
            .map(|(session, _)| session)
            .or_else(|| ctx.compare_capture.lock().take())
            .unwrap();
        finish_second.send(()).unwrap();
        second.stop().unwrap();
        assert!(stale_stop_was_inert, "an old watcher took the next capture");
        assert!(newer_still_owned && newer_still_recording);
        assert_eq!(std::fs::read(second_path).unwrap(), original);
    }

    #[test]
    fn stopping_a_pending_compare_withdraws_only_its_publication_until_native_cleanup() {
        let directory = tempfile::tempdir().unwrap();
        let ctx = Arc::new(recording_context(directory.path(), 7));
        ctx.pending_native_capture_starts
            .store(1, std::sync::atomic::Ordering::SeqCst);
        let path = audio_dir(directory.path()).join(compare_raw_name(7));
        let (finish_native, native_wait) = std::sync::mpsc::channel();
        let native_path = path.clone();
        let session = CaptureSession::from_test_worker(
            std::thread::spawn(move || {
                native_wait.recv_timeout(Duration::from_secs(5)).unwrap();
                write_pcm16_wav(&native_path, SAMPLE_RATE, &[0.1; 4800]).unwrap();
                Ok(capture_result(native_path, vec![0.1; 4800]))
            }),
            path.clone(),
        );
        assert!(take_compare_capture_for_stop(&ctx, Some(6)).is_none());
        assert!(ctx.compare_state.lock().recording);
        assert!(take_compare_capture_for_stop(&ctx, Some(7)).is_none());
        assert!(!ctx.compare_state.lock().recording);
        assert!(crate::app::operations::system_busy(&ctx));
        let (discarding, entered) = std::sync::mpsc::channel();
        let worker_ctx = Arc::clone(&ctx);
        let worker = std::thread::spawn(move || {
            commit_compare_capture_start(&worker_ctx, 7, session, |session| {
                discarding.send(()).unwrap();
                session.stop_and_discard().unwrap();
                release_compare_capture_reservation(&worker_ctx, 7);
                crate::app::session::release_pending_native_capture_start(&worker_ctx);
            })
        });
        entered.recv_timeout(Duration::from_secs(5)).unwrap();
        {
            let _admission = ctx.admission.try_lock().expect("discard held UI admission");
            assert!(crate::app::operations::system_busy(&ctx));
            assert!(ctx.compare_capture.lock().is_none());
            assert_eq!(
                ctx.pending_native_capture_starts
                    .load(std::sync::atomic::Ordering::SeqCst),
                1
            );
        }
        finish_native.send(()).unwrap();
        assert_eq!(worker.join().unwrap().unwrap_err(), AppError::Cancelled);
        assert!(!path.exists());
        assert!(!crate::app::operations::system_busy(&ctx));
    }

    #[test]
    fn duplicate_compare_stop_keeps_the_stop_reservation_and_ready_audio() {
        let directory = tempfile::tempdir().unwrap();
        let ctx = recording_context(directory.path(), 7);
        let path = audio_dir(directory.path()).join(compare_raw_name(7));
        write_pcm16_wav(&path, SAMPLE_RATE, &[0.1; 4800]).unwrap();
        let result_path = path.clone();
        let session = CaptureSession::from_test_worker(
            std::thread::spawn(move || Ok(capture_result(result_path, vec![0.1; 4800]))),
            path.clone(),
        );
        *ctx.compare_capture.lock() = Some(session);
        let (session, nonce) = take_compare_capture_for_stop(&ctx, Some(7)).unwrap();
        assert!(take_compare_capture_for_stop(&ctx, Some(7)).is_none());
        assert!(ctx.compare_state.lock().recording);
        assert!(crate::app::operations::system_busy(&ctx));
        let cleanup = session.cleanup_handle();
        let captured = session.stop().unwrap();
        let saved = stop_model_compare_inner(&ctx, nonce, Ok(captured), cleanup).unwrap();
        release_compare_capture_reservation(&ctx, nonce);
        assert_eq!(saved.nonce, 7);
        assert!(Path::new(saved.listen_path.as_ref().unwrap()).is_file());
        assert!(Path::new(saved.stt_path.as_ref().unwrap()).is_file());
        assert!(path.is_file());
        assert!(!crate::app::operations::system_busy(&ctx));
    }

    #[test]
    fn startup_nonce_skips_undeletable_previous_outputs_and_rejects_exhaustion_before_capture() {
        let directory = tempfile::tempdir().unwrap();
        let audio = audio_dir(directory.path());
        std::fs::create_dir_all(&audio).unwrap();
        // A directory is a deterministic failed unlink on Windows and Unix.
        let preserved = audio.join(compare_listen_name(7));
        std::fs::create_dir(&preserved).unwrap();
        std::fs::write(preserved.join("keep"), b"old payload").unwrap();
        for unknown in [
            "model-compare.9999999.unknown",
            "model-compare.000099.listen.wav",
            "model-compare.4b0bc73d-5fae-4bc7-a82e-b5f3b5a8c18d.stt.wav",
        ] {
            std::fs::create_dir(audio.join(unknown)).unwrap();
        }
        let ctx = AppContext::initialize(directory.path().to_path_buf()).unwrap();
        let nonce = ctx.compare_state.lock().next_nonce().unwrap();
        assert_eq!(nonce, 8);
        {
            let mut state = ctx.compare_state.lock();
            state.nonce = nonce;
            state.recording = true;
        }
        let committed = commit_compare_audio(&ctx, nonce, &[0.1; 64], &[0.1; 16]).unwrap();
        assert_eq!(committed.nonce, 8);
        assert_eq!(
            std::fs::read(preserved.join("keep")).unwrap(),
            b"old payload"
        );
        assert!(audio.join(compare_stt_name(8)).is_file());

        assert_eq!(
            CompareState {
                nonce: MAX_COMPARE_NONCE - 1,
                ..Default::default()
            }
            .next_nonce()
            .unwrap(),
            MAX_COMPARE_NONCE
        );
        for stored_nonce in [MAX_COMPARE_NONCE, MAX_COMPARE_NONCE + 1, u64::MAX] {
            let exhausted = tempfile::tempdir().unwrap();
            let audio = audio_dir(exhausted.path());
            std::fs::create_dir_all(&audio).unwrap();
            std::fs::create_dir(audio.join(compare_listen_name(stored_nonce))).unwrap();
            let ctx = AppContext::initialize(exhausted.path().to_path_buf()).unwrap();
            assert_eq!(ctx.compare_state.lock().nonce, stored_nonce);
            assert!(matches!(
                ctx.compare_state.lock().next_nonce(),
                Err(AppError::StorageFailed(_))
            ));
            assert!(!ctx.compare_state.lock().recording);
            assert_eq!(
                ctx.pending_native_capture_starts
                    .load(std::sync::atomic::Ordering::SeqCst),
                0
            );
        }
    }

    #[test]
    fn delayed_compare_snapshots_keep_ordered_revisions_across_clear_and_getters() {
        let directory = tempfile::tempdir().unwrap();
        let ctx = Arc::new(AppContext::initialize(directory.path().to_path_buf()).unwrap());
        {
            let mut state = ctx.compare_state.lock();
            state.nonce = 7;
            state.running = true;
        }
        let (captured, wait_captured) = std::sync::mpsc::channel();
        let (release, waiting) = std::sync::mpsc::channel();
        let (emitted, received) = std::sync::mpsc::channel();
        let worker_ctx = ctx.clone();
        let old_emitter = emitted.clone();
        let worker = std::thread::spawn(move || {
            let snapshot = compare_snapshot(&worker_ctx);
            captured.send(()).unwrap();
            waiting.recv_timeout(Duration::from_secs(2)).unwrap();
            old_emitter.send(snapshot).unwrap();
        });
        wait_captured.recv_timeout(Duration::from_secs(2)).unwrap();
        ctx.compare_state.lock().running = false;
        let terminal = compare_snapshot(&ctx);
        emitted.send(terminal.clone()).unwrap();
        release.send(()).unwrap();
        worker.join().unwrap();
        let first = received.recv_timeout(Duration::from_secs(2)).unwrap();
        let delayed = received.recv_timeout(Duration::from_secs(2)).unwrap();
        assert!(!first.running);
        assert!(delayed.running);
        assert!(first.revision > delayed.revision);
        // Capture tokens, rather than event delivery order, identify staleness.
        let getter = compare_snapshot(&ctx);
        assert!(getter.revision > terminal.revision);
        ctx.compare_state.lock().reset();
        let cleared = compare_snapshot(&ctx);
        assert!(cleared.revision > getter.revision);
        assert_eq!(cleared.nonce, 7);
        assert!(!cleared.running);
    }

    #[test]
    fn compare_admission_reserves_recovery_capacity_and_full_queue_preserves_confirmed_prices() {
        let directory = tempfile::tempdir().unwrap();
        let ctx = AppContext::initialize(directory.path().to_path_buf()).unwrap();
        let injection =
            rusqlite::Connection::open(directory.path().join("history.sqlite")).unwrap();
        injection.execute_batch("CREATE TRIGGER reject_compare_capacity BEFORE INSERT ON usage_attempts BEGIN SELECT RAISE(ABORT,'fixture capacity persistence failure'); END;").unwrap();
        let generation = ctx.history.lock().compare_usage_generation().unwrap();
        let capacity = crate::app::session::MAX_PENDING_ATTEMPTS;
        let reserved = MAX_COMPARE_MODELS * (1 + MAX_COMPARE_ADDITIONAL_RETRIES) as usize;
        let usage = |index: usize, cost_usd| ApiAttemptUsage {
            operation_id: "compare:capacity".into(),
            attempt_id: format!("compare:capacity:{index}"),
            occurred_at: chrono::Utc::now(),
            model: "model".into(),
            cost_usd,
        };
        for index in 0..capacity - reserved {
            assert!(crate::app::session::persist_compare_attempt(
                &ctx,
                usage(index, None),
                &generation
            )
            .is_err());
        }
        reserve_compare_run(&ctx).unwrap();
        assert!(crate::app::operations::system_busy(&ctx));
        for index in capacity - reserved..capacity {
            assert!(crate::app::session::persist_compare_attempt(
                &ctx,
                usage(index, None),
                &generation
            )
            .is_err());
        }
        // At capacity, Finished updates its already reserved Started entry.
        for index in capacity - reserved..capacity {
            assert!(crate::app::session::persist_compare_attempt(
                &ctx,
                usage(index, Some(0.02)),
                &generation
            )
            .is_err());
        }
        finish_compare_run(&ctx);
        assert!(!crate::app::operations::system_busy(&ctx));
        assert!(matches!(
            reserve_compare_run(&ctx),
            Err(AppError::StorageFailed(_))
        ));
        injection
            .execute_batch("DROP TRIGGER reject_compare_capacity;")
            .unwrap();
        while crate::app::session::flush_pending_attempts(&ctx) {}
        let now = chrono::Utc::now();
        let statistics = ctx
            .history
            .lock()
            .get_usage_statistics(
                now - chrono::Duration::days(1),
                now + chrono::Duration::days(1),
            )
            .unwrap();
        assert_eq!(statistics.api_requests, capacity as i64);
        assert_eq!(statistics.unpriced_attempts, (capacity - reserved) as i64);
        assert!((statistics.reported_cost_usd - reserved as f64 * 0.02).abs() < 1e-9);
        reserve_compare_run(&ctx).unwrap();
        finish_compare_run(&ctx);
    }

    #[tokio::test]
    async fn compare_usage_storage_failure_cancels_requests_recovers_price_and_respects_clear() {
        let server = MockServer::start();
        let request = server.mock(|when, then| {
            when.method(POST).path("/audio/transcriptions");
            then.status(200)
                .json_body(serde_json::json!({"text":"retained", "usage":{"cost":0.02}}));
        });
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("clip.wav");
        write_pcm16_wav(&path, STT_SAMPLE_RATE, &[0.1; 1600]).unwrap();
        let ctx = AppContext::initialize(directory.path().to_path_buf()).unwrap();
        let database = directory.path().join("history.sqlite");
        let injection = rusqlite::Connection::open(&database).unwrap();
        injection.execute_batch("CREATE TRIGGER reject_compare_price BEFORE UPDATE OF cost_usd ON usage_attempts WHEN NEW.operation_kind='compare' AND NEW.cost_usd IS NOT NULL BEGIN SELECT RAISE(ABORT,'fixture price persistence failure'); END;").unwrap();
        let generation = ctx.history.lock().compare_usage_generation().unwrap();
        let client = reqwest::Client::new();
        let cancel = reserve_compare_run(&ctx).unwrap();
        let receiver = cancel.subscribe();
        let mut persistence_error = None;
        let response = transcribe_file_with_progress(
            &client,
            &server.base_url(),
            "fixture-key",
            "model",
            None,
            &path,
            compare_retry_policy(RetryPolicy::default()),
            Duration::from_millis(100),
            receiver.clone(),
            |progress| {
                if let Some(usage) = compare_attempt_usage("run", "slot", "model", &progress) {
                    if let Err(error) = persist_compare_usage(&ctx, usage, &generation, &cancel) {
                        persistence_error.get_or_insert(error);
                    }
                }
            },
        )
        .await
        .unwrap();
        assert_eq!(response.text, "retained");
        assert!(*cancel.borrow());
        assert!(finish_compare_usage(&ctx, "compare:run", persistence_error).is_err());
        finish_compare_run(&ctx);
        // The API operation has returned and all callback-local values are gone.
        // Only the same shared owner used by periodic housekeeping retains cost.
        assert!(crate::app::session::compare_usage_is_pending(
            &ctx,
            "compare:run"
        ));
        assert!(!crate::app::operations::system_busy(&ctx));
        let rejected = transcribe_file(
            &client,
            &server.base_url(),
            "fixture-key",
            "another-model",
            None,
            &path,
            RetryPolicy::default(),
            Duration::from_millis(100),
            receiver,
        )
        .await;
        assert_eq!(rejected.unwrap_err(), AppError::Cancelled);
        assert_eq!(request.calls(), 1);
        injection
            .execute_batch("DROP TRIGGER reject_compare_price;")
            .unwrap();
        assert!(crate::app::session::flush_pending_attempts(&ctx));
        assert!(!crate::app::session::flush_pending_attempts(&ctx));
        assert!(!crate::app::session::compare_usage_is_pending(
            &ctx,
            "compare:run"
        ));
        let start = chrono::Utc::now() - chrono::Duration::days(1);
        let end = chrono::Utc::now() + chrono::Duration::days(1);
        let recovered = ctx.history.lock().get_usage_statistics(start, end).unwrap();
        assert_eq!(recovered.api_requests, 1);
        assert_eq!(recovered.reported_cost_usd, 0.02);
        assert_eq!(recovered.unpriced_attempts, 0);
        assert_eq!(ctx.history.lock().count(None).unwrap(), 0);
        injection.execute_batch("CREATE TRIGGER reject_clear_price BEFORE INSERT ON usage_attempts BEGIN SELECT RAISE(ABORT,'fixture clear persistence failure'); END;").unwrap();
        assert!(persist_compare_usage(
            &ctx,
            ApiAttemptUsage {
                operation_id: "compare:clear".into(),
                attempt_id: "compare:clear:slot:1".into(),
                occurred_at: chrono::Utc::now(),
                model: "model".into(),
                cost_usd: Some(0.03),
            },
            &generation,
            &cancel
        )
        .is_err());
        ctx.history.lock().clear_usage_statistics().unwrap();
        injection
            .execute_batch("DROP TRIGGER reject_clear_price;")
            .unwrap();
        assert!(crate::app::session::flush_pending_attempts(&ctx));
        assert!(!crate::app::session::compare_usage_is_pending(
            &ctx,
            "compare:clear"
        ));
        let cleared = ctx.history.lock().get_usage_statistics(start, end).unwrap();
        assert_eq!(cleared.api_requests, 0);
        assert_eq!(cleared.reported_cost_usd, 0.0);
    }

    #[tokio::test]
    async fn compare_api_retries_and_partial_prices_reach_shared_usage_without_history() {
        let server = MockServer::start();
        let mut busy = server.mock(|when, then| {
            when.method(POST).path("/audio/transcriptions");
            then.status(503).body("busy");
        });
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("clip.wav");
        write_pcm16_wav(&path, STT_SAMPLE_RATE, &[0.1; 1600]).unwrap();
        let repo = Arc::new(parking_lot::Mutex::new(
            crate::history::repository::HistoryRepo::open(&dir.path().join("usage.db")).unwrap(),
        ));
        let generation = repo.lock().compare_usage_generation().unwrap();
        let client = reqwest::Client::new();
        let base = server.base_url();
        let (_cancel, receiver) = tokio::sync::watch::channel(false);
        let policy = compare_retry_policy(RetryPolicy {
            additional_retries: 1,
            initial_retry_delay: Duration::from_millis(50),
            max_retry_delay: Duration::from_millis(50),
            ..RetryPolicy::default()
        });
        let request = transcribe_file_with_progress(
            &client,
            &base,
            "fixture-key",
            "model-a",
            None,
            &path,
            policy.clone(),
            Duration::from_millis(100),
            receiver.clone(),
            |progress| {
                if let Some(usage) = compare_attempt_usage("run", "a", "model-a", &progress) {
                    repo.lock()
                        .record_compare_attempt(&usage, &generation)
                        .unwrap();
                }
            },
        );
        let switch = async {
            while busy.calls() == 0 {
                tokio::time::sleep(Duration::from_millis(2)).await;
            }
            busy.delete();
            server.mock(|when, then| {
                when.method(POST).path("/audio/transcriptions");
                then.status(200)
                    .json_body(serde_json::json!({"text":"ok", "usage":{"cost":0.01}}));
            });
        };
        let (result, _) = tokio::time::timeout(Duration::from_secs(5), async {
            tokio::join!(request, switch)
        })
        .await
        .unwrap();
        assert_eq!(result.unwrap().attempt, 2);

        let missing_price = MockServer::start();
        missing_price.mock(|when, then| {
            when.method(POST).path("/audio/transcriptions");
            then.status(200)
                .json_body(serde_json::json!({"text":"second"}));
        });
        transcribe_file_with_progress(
            &client,
            &missing_price.base_url(),
            "fixture-key",
            "model-b",
            None,
            &path,
            policy,
            Duration::from_millis(100),
            receiver,
            |progress| {
                if let Some(usage) = compare_attempt_usage("run", "b", "model-b", &progress) {
                    repo.lock()
                        .record_compare_attempt(&usage, &generation)
                        .unwrap();
                }
            },
        )
        .await
        .unwrap();
        let repo = repo.lock();
        let statistics = repo
            .get_usage_statistics(
                chrono::Utc::now() - chrono::Duration::days(1),
                chrono::Utc::now() + chrono::Duration::days(1),
            )
            .unwrap();
        assert_eq!(statistics.api_requests, 3);
        assert_eq!(statistics.reported_cost_usd, 0.01);
        assert_eq!(statistics.unpriced_attempts, 2);
        assert_eq!(statistics.dictations, 0);
        assert_eq!(repo.count(None).unwrap(), 0);
        assert_eq!(
            statistics
                .models
                .iter()
                .find(|row| row.model == "model-a")
                .unwrap()
                .api_requests,
            2
        );
        assert_eq!(
            statistics
                .models
                .iter()
                .find(|row| row.model == "model-b")
                .unwrap()
                .unpriced_attempts,
            1
        );
    }

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
            limit_reached: false,
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
        let failure = AppError::StorageFailed("injected second compare output failure".into());
        let result = commit_compare_audio_with(&ctx, 3, |listen, stt| {
            write_pcm16_wav(listen, SAMPLE_RATE, &[0.1; 64])?;
            std::fs::write(
                stt.with_extension("wav.tmp"),
                b"partially written second WAV",
            )
            .map_err(|error| AppError::StorageFailed(error.to_string()))?;
            assert!(
                listen.is_file(),
                "failure must follow the first complete write"
            );
            Err(failure.clone())
        });
        assert_eq!(result.unwrap_err(), failure);
        assert!(!new_listen.exists());
        assert!(!new_listen.with_extension("wav.tmp").exists());
        assert!(!new_stt.exists());
        assert!(!new_stt.with_extension("wav.tmp").exists());
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
    fn compare_disk_write_releases_admission_protects_candidates_and_rejects_stale_publish() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = Arc::new(recording_context(dir.path(), 3));
        let root = audio_dir(dir.path());
        let (writing, waiting) = std::sync::mpsc::channel();
        let (release, resume) = std::sync::mpsc::channel();
        let worker_ctx = Arc::clone(&ctx);
        let worker = std::thread::spawn(move || {
            commit_compare_audio_with(&worker_ctx, 3, |listen, stt| {
                write_pcm16_wav(listen, SAMPLE_RATE, &[0.1; 64])?;
                writing.send(()).unwrap();
                resume.recv_timeout(Duration::from_secs(3)).unwrap();
                write_pcm16_wav(stt, STT_SAMPLE_RATE, &[0.1; 16])
            })
        });
        waiting.recv_timeout(Duration::from_secs(2)).unwrap();
        let admission = ctx
            .admission
            .try_lock()
            .expect("disk write retained UI admission");
        assert!(ctx.compare_state.try_lock().is_some());
        let protected = crate::app::operations::protected_audio_names(&ctx);
        assert!(protected.contains(&compare_stt_name(3)));
        assert!(protected.contains(&format!("{}.tmp", compare_stt_name(3))));
        crate::history::retention::cleanup_orphans_except(&root, &ctx.history.lock(), &protected)
            .unwrap();
        assert!(root.join(compare_listen_name(3)).is_file());
        drop(admission);
        assert_eq!(
            clear_model_compare_inner(&ctx, |_| panic!("active writer must not be cleared"))
                .unwrap_err(),
            AppError::TranscriptionInProgress
        );
        // The generation can be invalidated by shutdown while IO is pending.
        // A new clip's files must survive the old writer's rollback.
        let admission = ctx.admission.lock();
        let newer = root.join(compare_listen_name(4));
        std::fs::write(&newer, b"newer clip").unwrap();
        {
            let mut state = ctx.compare_state.lock();
            state.nonce = 4;
            state.recording = false;
            state.listen_path = Some(newer.to_string_lossy().into_owned());
        }
        let expected = ctx.compare_state.lock().clone();
        drop(admission);
        release.send(()).unwrap();
        assert_eq!(worker.join().unwrap().unwrap_err(), AppError::Cancelled);
        assert_eq!(*ctx.compare_state.lock(), expected);
        assert_eq!(std::fs::read(newer).unwrap(), b"newer clip");
        assert!(!root.join(compare_listen_name(3)).exists());
        assert!(!root.join(compare_stt_name(3)).exists());
        assert!(ctx.pending_audio_writes.lock().is_empty());
    }

    #[test]
    fn compare_clear_releases_admission_and_reserves_monotonic_generation_until_cleanup() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = Arc::new(recording_context(dir.path(), 8));
        ctx.compare_state.lock().recording = false;
        let root = audio_dir(dir.path());
        std::fs::write(root.join(compare_listen_name(8)), b"retired clip").unwrap();
        std::fs::write(root.join("unrelated.wav"), b"other").unwrap();
        let (clearing, waiting) = std::sync::mpsc::channel();
        let (release, resume) = std::sync::mpsc::channel();
        let worker_ctx = Arc::clone(&ctx);
        let worker = std::thread::spawn(move || {
            clear_model_compare_inner(&worker_ctx, |root| {
                clearing.send(()).unwrap();
                resume.recv_timeout(Duration::from_secs(3)).unwrap();
                remove_compare_artifacts(root)
            })
        });
        waiting.recv_timeout(Duration::from_secs(2)).unwrap();
        let admission = ctx
            .admission
            .try_lock()
            .expect("clear retained UI admission");
        assert!(crate::app::operations::system_busy(&ctx));
        assert_eq!(ctx.compare_state.lock().nonce, 8);
        assert!(!ctx.compare_state.lock().recording);
        drop(admission);
        assert_eq!(
            clear_model_compare_inner(&ctx, |_| panic!("second clear must not own the file set"))
                .unwrap_err(),
            AppError::TranscriptionInProgress
        );
        release.send(()).unwrap();
        worker.join().unwrap().unwrap();
        assert!(!crate::app::operations::system_busy(&ctx));
        assert_eq!(ctx.compare_state.lock().nonce, 8);
        assert!(!root.join(compare_listen_name(8)).exists());
        assert_eq!(std::fs::read(root.join("unrelated.wav")).unwrap(), b"other");
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
    fn compare_compresses_upload_only_and_respects_disabled_setting() {
        for enabled in [true, false] {
            let dir = tempfile::tempdir().unwrap();
            let ctx = recording_context(dir.path(), 3);
            ctx.settings.lock().silence_compression.enabled = enabled;
            let mut samples = vec![0.01; SAMPLE_RATE as usize];
            samples.extend(vec![0.0; SAMPLE_RATE as usize * 10]);
            samples.extend(vec![-0.02; SAMPLE_RATE as usize]);
            let input_len = samples.len();
            let snapshot = process_compare_audio(&ctx, 3, samples, SAMPLE_RATE, Ok).unwrap();
            let (listen, rate) =
                read_pcm16_wav_with_rate(Path::new(snapshot.listen_path.as_ref().unwrap()))
                    .unwrap();
            assert_eq!(rate, SAMPLE_RATE);
            assert_eq!(listen.len(), input_len);
            let (stt, rate) =
                read_pcm16_wav_with_rate(Path::new(snapshot.stt_path.as_ref().unwrap())).unwrap();
            assert_eq!(rate, STT_SAMPLE_RATE);
            let seconds = if enabled { 4 } else { 12 };
            assert!((stt.len() as i64 - i64::from(STT_SAMPLE_RATE) * seconds).abs() <= 320);
        }
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
    fn pending_compare_start_and_stale_cleanup_never_hold_ui_admission() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = Arc::new(recording_context(dir.path(), 7));
        ctx.pending_native_capture_starts
            .store(1, std::sync::atomic::Ordering::SeqCst);
        let path = audio_dir(dir.path()).join(compare_raw_name(7));
        let native_path = path.clone();
        let (finish_native, native_wait) = std::sync::mpsc::channel();
        let native_worker = std::thread::spawn(move || {
            native_wait.recv().unwrap();
            write_pcm16_wav(&native_path, SAMPLE_RATE, &[0.25; 48]).unwrap();
            Ok(capture_result(native_path, Vec::new()))
        });
        let session = CaptureSession::from_test_worker(native_worker, path.clone());
        let (starting, started) = std::sync::mpsc::channel();
        let (finish_start, start_wait) = std::sync::mpsc::channel();
        let (discarding, discarded) = std::sync::mpsc::channel();
        let worker_ctx = ctx.clone();
        let worker = std::thread::spawn(move || {
            let admission = crate::app::operations::lock_admission(&worker_ctx);
            let session = crate::app::operations::with_admission_released(admission, || {
                starting.send(()).unwrap();
                start_wait.recv().unwrap();
                session
            });
            commit_compare_capture_start(&worker_ctx, 7, session, |session| {
                discarding.send(()).unwrap();
                session.stop_and_discard().unwrap();
                release_compare_capture_reservation(&worker_ctx, 7);
                crate::app::session::release_pending_native_capture_start(&worker_ctx);
            })
        });
        started.recv_timeout(Duration::from_secs(2)).unwrap();
        {
            let _admission = ctx
                .admission
                .try_lock()
                .expect("UI admission blocked during native start");
            assert!(crate::app::operations::system_busy(&ctx));
            ctx.shutdown_requested
                .store(true, std::sync::atomic::Ordering::SeqCst);
        }
        finish_start.send(()).unwrap();
        discarded.recv_timeout(Duration::from_secs(2)).unwrap();
        {
            let _admission = ctx
                .admission
                .try_lock()
                .expect("UI admission blocked during stale stop");
            assert!(ctx.compare_capture.lock().is_none());
            assert_eq!(
                ctx.pending_native_capture_starts
                    .load(std::sync::atomic::Ordering::SeqCst),
                1
            );
        }
        finish_native.send(()).unwrap();
        assert_eq!(worker.join().unwrap().unwrap_err(), AppError::Cancelled);
        assert_eq!(
            ctx.pending_native_capture_starts
                .load(std::sync::atomic::Ordering::SeqCst),
            0
        );
        assert!(!ctx.compare_state.lock().recording);
        assert!(!path.exists());
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
                limit_reached: false,
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
        assert!(frozen_stt_copy(dir.path(), "run1", &src).is_err());
        assert_eq!(std::fs::read(&frozen).unwrap(), b"one");
        assert!(frozen_stt_copy(dir.path(), "missing", &dir.path().join("absent.wav")).is_err());
        assert!(!dir.path().join("model-compare.missing.stt.wav").exists());
    }

    #[tokio::test]
    async fn parallel_slots_do_not_share_failure() {
        let server = MockServer::start();
        server.mock(|when, then| {
            when.method(POST)
                .path("/audio/transcriptions")
                .body_includes("model-a");
            then.status(401)
                .json_body(serde_json::json!({"error":"no"}));
        });
        server.mock(|when, then| {
            when.method(POST)
                .path("/audio/transcriptions")
                .body_includes("model-b");
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
