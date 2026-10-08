use crate::app::lifecycle::configure_tray;
use crate::app::machine::SessionState;
use crate::app::overlay::OverlayTimingReport;
use crate::app::overlay_controller::OverlaySnapshot;
use crate::app::session::{
    current_meter, devices, release_meter_monitor, reserve_meter_monitor, start_meter_monitor,
    stop_meter_capture, take_owned_meter_monitor, AppContext,
};
use crate::audio::capture::{read_pcm16_wav, write_pcm16_wav, CaptureSession, CaptureStopOutcome};
use crate::audio::devices::InputDeviceInfo;
use crate::dsp::metrics::SAMPLE_RATE;
use crate::dsp::pipeline::prepare_listen_preview;
use crate::error::AppError;
use crate::history::repository::Recording;
use crate::settings::{validate_settings_graph, AppSettings};
use crate::transcription::openrouter::{list_transcription_models, SttModel};
use crate::windows_int::credentials::{delete_api_key, has_api_key, set_api_key};
use crate::windows_int::text_injector::native;
use std::path::Path;
use std::sync::Arc;
use tauri::{AppHandle, Emitter, Manager, State};

#[tauri::command]
pub fn get_session_state(ctx: State<'_, Arc<AppContext>>) -> SessionState {
    ctx.state.lock().clone()
}

#[tauri::command]
pub fn get_overlay_snapshot(ctx: State<'_, Arc<AppContext>>) -> OverlaySnapshot {
    ctx.overlay_snapshot()
}

#[tauri::command]
pub fn overlay_pointer_matches(window: tauri::WebviewWindow, client_x: f64, client_y: f64) -> bool {
    let (Ok(hwnd), Ok(scale_factor)) = (window.hwnd(), window.scale_factor()) else {
        return false;
    };
    crate::windows_int::overlay::pointer_event_matches_overlay(
        hwnd.0 as isize,
        client_x,
        client_y,
        scale_factor,
    )
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
    persist_settings_inner(app, ctx, settings, true, false)
}

fn persist_settings_inner(
    app: &AppHandle,
    ctx: &AppContext,
    mut settings: AppSettings,
    run_retention: bool,
    allow_retention_change: bool,
) -> Result<AppSettings, AppError> {
    let _writer = ctx.settings_write_gate.lock();
    settings.retry.validate()?;
    settings.mic_tune.validate()?;
    validate_settings_graph(&settings)?;
    let previous = ctx.settings.lock().clone();
    settings.retention_policy_confirmed = previous.retention_policy_confirmed;
    if !allow_retention_change
        && (settings.retention != previous.retention
            || settings.storage_limit != previous.storage_limit)
    {
        return Err(AppError::RequestValidationFailed(
            "retention changes require preview and confirmation".into(),
        ));
    }
    settings.first_run_complete = previous.first_run_complete;
    if settings.write_seq != 0 && settings.write_seq <= previous.write_seq {
        return Ok(previous);
    }
    if settings.hotkey != previous.hotkey {
        crate::app::shortcuts::parse_hotkey(&settings.hotkey)?;
    }
    let hotkey_changed = settings.hotkey != previous.hotkey;
    write_ordered_settings_snapshot(ctx, &mut settings, &previous)?;
    if hotkey_changed {
        if let Err(err) = crate::app::shortcuts::sync_shortcuts(app) {
            write_settings_snapshot(ctx, &previous)?;
            let _ = crate::app::shortcuts::sync_shortcuts(app);
            return Err(err);
        }
    }
    if settings.start_with_windows != previous.start_with_windows {
        if let Err(err) = crate::app::lifecycle::sync_autostart(app, settings.start_with_windows) {
            settings.start_with_windows = previous.start_with_windows;
            write_settings_snapshot(ctx, &settings)?;
            let _ = app.emit_to("main", "settings://changed", settings.clone());
            return Err(err);
        }
    }
    let _ = app.emit_to("main", "settings://changed", settings.clone());
    let _ = app.emit_to("overlay", "settings://changed", settings.clone());
    if run_retention
        && (settings.retention != previous.retention
            || settings.storage_limit != previous.storage_limit)
    {
        let _ = crate::app::session::apply_configured_retention(ctx);
    }
    if previous.ui_language != settings.ui_language {
        let _ = configure_tray(app);
    }
    ctx.transport
        .lock()
        .sync(settings.retry.to_policy().connect_timeout)?;
    if settings.debug_logging != previous.debug_logging {
        crate::logging::apply_debug_logging(settings.debug_logging);
    }
    Ok(settings)
}

fn write_settings_snapshot(ctx: &AppContext, settings: &AppSettings) -> Result<(), AppError> {
    settings.save(&ctx.settings_path)?;
    let policy_changed = {
        let mut current = ctx.settings.lock();
        let changed = current.close_to_tray != settings.close_to_tray;
        *current = settings.clone();
        changed
    };
    if policy_changed {
        crate::runtime_diagnostics::record(crate::runtime_diagnostics::Event::ClosePolicyChanged {
            close_to_tray: settings.close_to_tray,
            write_seq: settings.write_seq,
        });
    }
    Ok(())
}

fn write_ordered_settings_snapshot(
    ctx: &AppContext,
    settings: &mut AppSettings,
    previous: &AppSettings,
) -> Result<bool, AppError> {
    if settings.write_seq != 0 && settings.write_seq <= previous.write_seq {
        return Ok(false);
    }
    settings.write_seq = settings.write_seq.max(previous.write_seq.saturating_add(1));
    write_settings_snapshot(ctx, settings)?;
    Ok(true)
}

#[tauri::command]
pub fn acknowledge_first_run_disclosure(
    ctx: State<'_, Arc<AppContext>>,
) -> Result<AppSettings, AppError> {
    let _writer = ctx.settings_write_gate.lock();
    let mut settings = ctx.settings.lock().clone();
    settings.first_run_complete = true;
    settings.write_seq = settings.write_seq.saturating_add(1);
    settings.save(&ctx.settings_path)?;
    *ctx.settings.lock() = settings.clone();
    Ok(settings)
}

#[tauri::command]
pub fn list_history(ctx: State<'_, Arc<AppContext>>) -> Result<Vec<Recording>, AppError> {
    ctx.history.lock().list(500)
}

#[tauri::command]
pub fn preview_retention_settings(
    ctx: State<'_, Arc<AppContext>>,
    settings: AppSettings,
) -> Result<crate::history::retention::RetentionPreview, AppError> {
    validate_settings_graph(&settings)?;
    let protected = crate::app::operations::protected_recording_ids(ctx.inner());
    let protected_names = crate::app::operations::protected_audio_names(ctx.inner());
    crate::history::retention::preview_retention(
        &ctx.history.lock(),
        &crate::history::repository::audio_dir(&ctx.data_dir),
        crate::history::retention::Retention::from_setting(&settings.retention),
        settings.storage_limit_bytes(),
        &protected,
        &protected_names,
    )
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RetentionApplyResult {
    pub settings: AppSettings,
    pub deleted: Vec<String>,
    pub failed: Vec<String>,
}

#[tauri::command]
pub fn apply_retention_settings(
    app: AppHandle,
    ctx: State<'_, Arc<AppContext>>,
    settings: AppSettings,
    expected_preview: crate::history::retention::RetentionPreview,
    confirmed: bool,
) -> Result<RetentionApplyResult, AppError> {
    if !confirmed {
        return Err(AppError::RequestValidationFailed(
            "retention confirmation required".into(),
        ));
    }
    let requested_retention = settings.retention.clone();
    let requested_storage_limit = settings.storage_limit.clone();
    settings.retry.validate()?;
    settings.mic_tune.validate()?;
    validate_settings_graph(&settings)?;
    let _admission = crate::app::operations::lock_admission(ctx.inner());
    let protected = crate::app::operations::protected_recording_ids(ctx.inner());
    let names = crate::app::operations::protected_audio_names(ctx.inner());
    let audio_root = crate::history::repository::audio_dir(&ctx.data_dir);
    let current_preview = crate::history::retention::preview_retention(
        &ctx.history.lock(),
        &audio_root,
        crate::history::retention::Retention::from_setting(&requested_retention),
        settings.storage_limit_bytes(),
        &protected,
        &names,
    )?;
    if current_preview != expected_preview {
        return Err(AppError::RequestValidationFailed(
            "retention preview changed; review the updated preview before applying".into(),
        ));
    }
    let persisted = persist_settings_inner(&app, ctx.inner().as_ref(), settings, false, true)?;
    if persisted.retention != requested_retention
        || persisted.storage_limit != requested_storage_limit
    {
        return Err(AppError::RequestValidationFailed(
            "retention settings changed before they could be applied".into(),
        ));
    }
    let settings = {
        let _writer = ctx.settings_write_gate.lock();
        let mut settings = ctx.settings.lock().clone();
        settings.retention_policy_confirmed = true;
        write_settings_snapshot(ctx.inner(), &settings)?;
        *ctx.settings_recovery.lock() = false;
        settings
    };
    let result = crate::history::retention::apply_retention_recording_ids(
        &ctx.history.lock(),
        &audio_root,
        &expected_preview.recording_ids_to_delete,
        &protected,
        &names,
    )?;
    ctx.emit_history(&app);
    Ok(RetentionApplyResult {
        settings,
        deleted: result.deleted,
        failed: result.failed,
    })
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
pub fn get_usage_statistics(
    ctx: State<'_, Arc<AppContext>>,
    start: String,
    end: String,
) -> Result<crate::history::repository::UsageStatistics, AppError> {
    let start = chrono::DateTime::parse_from_rfc3339(&start)
        .map_err(|e| AppError::StorageFailed(e.to_string()))?
        .with_timezone(&chrono::Utc);
    let end = chrono::DateTime::parse_from_rfc3339(&end)
        .map_err(|e| AppError::StorageFailed(e.to_string()))?
        .with_timezone(&chrono::Utc);
    let first_day = start.with_timezone(&chrono::Local).date_naive();
    let last_day = (end - chrono::Duration::nanoseconds(1))
        .with_timezone(&chrono::Local)
        .date_naive();
    if end <= start || last_day < first_day || (last_day - first_day).num_days() >= 90 {
        return Err(AppError::StorageFailed(
            "statistics range must be 1-90 days".into(),
        ));
    }
    ctx.history.lock().get_usage_statistics(start, end)
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
    let _admission = crate::app::operations::lock_admission(ctx.inner());
    if crate::app::operations::history_recording_is_protected(ctx.inner(), &id) {
        return Err(AppError::TranscriptionInProgress);
    }
    let rec = ctx
        .history
        .lock()
        .get(&id)?
        .ok_or_else(|| AppError::StorageFailed("not found".into()))?;
    let root = crate::history::repository::audio_dir(&ctx.data_dir);
    let history = ctx.history.lock();
    let deletion = crate::history::retention::delete_recording_and_usage(&history, &root, &rec);
    drop(history);
    ctx.emit_history(&app);
    if !deletion? {
        return Err(AppError::StorageFailed("recording missing".into()));
    }
    Ok(())
}

#[tauri::command]
pub fn delete_all_history(
    app: AppHandle,
    ctx: State<'_, Arc<AppContext>>,
) -> Result<crate::history::retention::DeleteAllResult, AppError> {
    let _admission = crate::app::operations::lock_admission(ctx.inner());
    let root = crate::history::repository::audio_dir(&ctx.data_dir);
    let protected = crate::app::operations::protected_recording_ids(ctx.inner());
    let mut result =
        crate::history::retention::delete_all_recordings(&ctx.history.lock(), &root, &protected)?;
    if !crate::app::operations::system_busy(ctx.inner()) {
        result
            .failed
            .extend(crate::history::retention::remove_filter_artifacts(&root));
        result
            .failed
            .extend(crate::app::compare::remove_compare_artifacts(&root));
        *ctx.compare_state.lock() = crate::app::compare::CompareState::default();
        let _ = app.emit_to("main", "compare://state", ctx.compare_state.lock().clone());
    } else {
        result.failed.push("active audio samples".into());
    }
    clear_usage_after_delete_all(&ctx.history.lock(), &result, &protected)?;
    ctx.emit_history(&app);
    Ok(result)
}

fn clear_usage_after_delete_all(
    history: &crate::history::repository::HistoryRepo,
    result: &crate::history::retention::DeleteAllResult,
    protected_ids: &std::collections::HashSet<String>,
) -> Result<(), AppError> {
    if protected_ids.is_empty() && history.count(None)? == 0 {
        history.clear_usage_statistics()?;
    } else {
        for id in &result.deleted {
            history.delete_usage_for_recording(id)?;
        }
    }
    Ok(())
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
    let revision = uuid::Uuid::new_v4();
    let original_preview = audio_root.join(format!("filter-preview-{revision}-original.wav"));
    let processed_path = audio_root.join(format!("filter-preview-{revision}.wav"));
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

fn finish_filter_start<T>(
    ctx: &AppContext,
    generation: u64,
    started: Result<T, AppError>,
    publish: impl FnOnce(T),
    discard: impl FnOnce(T),
) -> Result<(), AppError> {
    let _admission = crate::app::operations::lock_admission(ctx);
    if ctx
        .filter_generation
        .load(std::sync::atomic::Ordering::SeqCst)
        != generation
    {
        match started {
            Ok(session) => discard(session),
            Err(_) => {
                ctx.filter_recording
                    .store(false, std::sync::atomic::Ordering::SeqCst);
            }
        }
        return Err(AppError::Cancelled);
    }
    match started {
        Ok(session) => {
            publish(session);
            Ok(())
        }
        Err(err) => {
            ctx.filter_recording
                .store(false, std::sync::atomic::Ordering::SeqCst);
            Err(err)
        }
    }
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
    ctx.settings.lock().mic_tune.validate()?;
    let generation = {
        let _admission = crate::app::operations::lock_admission(ctx.inner());
        if crate::app::operations::system_busy(ctx.inner()) {
            return Err(AppError::TranscriptionInProgress);
        }
        if !release_meter_monitor(ctx.inner()) {
            return Err(AppError::TranscriptionInProgress);
        }
        ctx.filter_recording
            .store(true, std::sync::atomic::Ordering::SeqCst);
        ctx.pending_native_capture_starts
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        ctx.filter_generation
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst)
            + 1
    };
    let dest = crate::history::repository::audio_dir(&ctx.data_dir).join("filter-sample.wav");
    let settings = ctx.settings.lock().clone();
    let device = if settings.input_device == "default" {
        None
    } else {
        Some(settings.input_device.clone())
    };
    let ctx = ctx.inner().clone();
    let worker_ctx = ctx.clone();
    let worker_app = app.clone();
    let started = tokio::task::spawn_blocking(move || {
        let late_ctx = worker_ctx.clone();
        let late_app = worker_app.clone();
        let started =
            CaptureSession::start_with_late_completion(device.as_deref(), dest, move || {
                let _admission = crate::app::operations::lock_admission(&late_ctx);
                if late_ctx
                    .shutdown_requested
                    .load(std::sync::atomic::Ordering::SeqCst)
                {
                    late_ctx
                        .filter_recording
                        .store(false, std::sync::atomic::Ordering::SeqCst);
                } else {
                    release_filter_capture_reservation(&late_ctx, generation);
                }
                crate::app::session::release_pending_native_capture_start(&late_ctx);
                drop(_admission);
                crate::app::session::schedule_deferred_shutdown_exit(&late_app, &late_ctx);
            });
        let native_worker_pending = started
            .as_ref()
            .err()
            .is_some_and(|failure| failure.native_worker_pending);
        let started = if native_worker_pending {
            match started {
                Err(failure) => Err(failure.error),
                Ok(_) => unreachable!("native worker pending requires a timed-out start"),
            }
        } else {
            finish_filter_start(
                &worker_ctx,
                generation,
                started.map_err(|failure| failure.error),
                |session| {
                    *worker_ctx.preview_capture.lock() = Some(session);
                },
                |session| {
                    let done_ctx = worker_ctx.clone();
                    let done_app = worker_app.clone();
                    crate::app::session::stop_capture_discard_during_shutdown(
                        Arc::clone(&worker_ctx),
                        worker_app.clone(),
                        session,
                        move || {
                            done_ctx
                                .filter_recording
                                .store(false, std::sync::atomic::Ordering::SeqCst);
                            let _ = done_app.emit_to("main", "filter://sample", false);
                        },
                    );
                },
            )
        };
        if !native_worker_pending {
            crate::app::session::release_pending_native_capture_start(&worker_ctx);
            crate::app::session::schedule_deferred_shutdown_exit(&worker_app, &worker_ctx);
        }
        (started, native_worker_pending)
    })
    .await
    .unwrap_or_else(|e| (Err(AppError::AudioCaptureFailed(e.to_string())), false));
    let (started, native_worker_pending) = started;
    if let Err(err) = started {
        if !native_worker_pending
            && ctx
                .filter_generation
                .load(std::sync::atomic::Ordering::SeqCst)
                == generation
        {
            ctx.filter_recording
                .store(false, std::sync::atomic::Ordering::SeqCst);
        }
        return Err(err);
    }
    let _ = app.emit_to("main", "filter://sample", true);
    let watcher_app = app.clone();
    tauri::async_runtime::spawn(async move {
        loop {
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
            if ctx
                .filter_generation
                .load(std::sync::atomic::Ordering::SeqCst)
                != generation
            {
                break;
            }
            let finished = ctx
                .preview_capture
                .lock()
                .as_ref()
                .is_some_and(CaptureSession::is_finished);
            if finished {
                if let Err(err) =
                    stop_filter_sample(watcher_app.clone(), watcher_app.state::<Arc<AppContext>>())
                        .await
                {
                    let _ = watcher_app.emit_to("main", "filter://error", err.code());
                }
                break;
            }
        }
    });
    Ok(())
}

#[tauri::command]
pub async fn stop_filter_sample(
    app: AppHandle,
    ctx: State<'_, Arc<AppContext>>,
) -> Result<DspPreview, AppError> {
    let (session, stop_generation) = {
        let _admission = crate::app::operations::lock_admission(ctx.inner());
        let session = ctx.preview_capture.lock().take();
        let generation = if session.is_some() {
            ctx.filter_generation
                .fetch_add(1, std::sync::atomic::Ordering::SeqCst)
                + 1
        } else {
            ctx.filter_generation
                .load(std::sync::atomic::Ordering::SeqCst)
        };
        (session, generation)
    };
    let had_session = session.is_some();
    let ctx = ctx.inner().clone();
    let result_ctx = ctx.clone();
    let app_for_late_result = app.clone();
    let timed_out = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let timed_out_worker = Arc::clone(&timed_out);
    let preview = tokio::task::spawn_blocking(move || {
        let session =
            session.ok_or_else(|| AppError::AudioCaptureFailed("sample not started".into()))?;
        let cleanup = session.cleanup_handle();
        let late_cleanup = cleanup.clone();
        let late_handler = filter_late_result_handler(
            ctx.clone(),
            stop_generation,
            Some(app_for_late_result.clone()),
        );
        let late_app = app_for_late_result;
        let outcome = session.stop_with_late_result(move |_| {
            late_cleanup.discard_late_result();
            late_handler();
            let _ = late_app.emit_to("main", "filter://sample", false);
        });
        let result = match outcome {
            CaptureStopOutcome::Completed(Ok(result)) => result,
            CaptureStopOutcome::Completed(Err(err)) => {
                cleanup.discard_late_result();
                return Err(err);
            }
            CaptureStopOutcome::TimedOut(err) => {
                timed_out_worker.store(true, std::sync::atomic::Ordering::SeqCst);
                cleanup.discard_late_result();
                return Err(err);
            }
        };
        if let Err(err) = crate::audio::capture::validate_capture_result(&result) {
            cleanup.discard_late_result();
            return Err(err);
        }
        preview_from_original(&ctx, &result.path)
    })
    .await;
    if had_session && !timed_out.load(std::sync::atomic::Ordering::SeqCst) {
        result_ctx
            .filter_recording
            .store(false, std::sync::atomic::Ordering::SeqCst);
        crate::app::session::schedule_deferred_shutdown_exit(&app, &result_ctx);
    }
    let _ = app.emit_to("main", "filter://sample", false);
    preview.map_err(|e| AppError::AudioProcessingFailed(e.to_string()))?
}

fn release_filter_capture_reservation(ctx: &AppContext, generation: u64) {
    if ctx
        .shutdown_requested
        .load(std::sync::atomic::Ordering::SeqCst)
        || ctx
            .filter_generation
            .load(std::sync::atomic::Ordering::SeqCst)
            == generation
    {
        ctx.filter_recording
            .store(false, std::sync::atomic::Ordering::SeqCst);
    }
}

fn filter_late_result_handler(
    ctx: Arc<AppContext>,
    generation: u64,
    app: Option<AppHandle>,
) -> impl FnOnce() + Send + 'static {
    move || {
        release_filter_capture_reservation(&ctx, generation);
        if let Some(app) = app.as_ref() {
            crate::app::session::schedule_deferred_shutdown_exit(app, &ctx);
        }
    }
}

#[tauri::command]
pub async fn start_input_meter(
    app: AppHandle,
    ctx: State<'_, Arc<AppContext>>,
    owner: String,
) -> Result<bool, AppError> {
    let ctx = ctx.inner().clone();
    let meter_app = app.clone();
    let Some(generation) = reserve_meter_monitor(&ctx, &owner) else {
        return Ok(false);
    };
    tokio::task::spawn_blocking(move || start_meter_monitor(&ctx, generation, &meter_app))
        .await
        .map_err(|e| AppError::AudioCaptureFailed(e.to_string()))?
}

#[tauri::command]
pub async fn stop_input_meter(
    ctx: State<'_, Arc<AppContext>>,
    owner: String,
) -> Result<(), AppError> {
    let ctx = ctx.inner().clone();
    tokio::task::spawn_blocking(move || {
        let session = take_owned_meter_monitor(&ctx, &owner);
        if let Some(session) = session {
            stop_meter_capture(&ctx, session)?;
        }
        Ok::<(), AppError>(())
    })
    .await
    .map_err(|e| AppError::AudioCaptureFailed(e.to_string()))??;
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
pub async fn test_openrouter(ctx: State<'_, Arc<AppContext>>) -> Result<String, AppError> {
    let key = crate::windows_int::credentials::get_api_key()?.ok_or(AppError::InvalidApiKey)?;
    let models = list_transcription_models(&ctx.clone_transport_client()?, &key).await?;
    let selected_model = ctx.settings.lock().model.clone();
    crate::transcription::openrouter::ensure_transcription_model_available(
        &selected_model,
        &models,
    )?;
    Ok(selected_model)
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
pub async fn manual_reprocess(app: AppHandle, recording_id: String) -> Result<Recording, AppError> {
    crate::app::session::manual_reprocess(app, recording_id).await
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
    persist_settings_inner(&app, ctx.inner().as_ref(), reset, false, false)
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
pub fn reset_main_window(app: AppHandle) -> Result<(), AppError> {
    crate::app::lifecycle::reset_main_window(&app)
}

#[tauri::command]
pub fn open_openrouter_models() -> Result<(), AppError> {
    tauri_plugin_opener::open_url(
        crate::app::lifecycle::OPENROUTER_TRANSCRIPTION_MODELS_URL,
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
    let root = crate::history::repository::audio_dir(&ctx.data_dir);
    let Some(name) = existing_recording_audio(&rec, &root) else {
        return Ok(None);
    };
    if name.contains("..") || Path::new(name).is_absolute() {
        return Err(AppError::StorageFailed("invalid audio path".into()));
    }
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

fn existing_recording_audio<'a>(rec: &'a Recording, root: &Path) -> Option<&'a str> {
    [
        rec.processed_audio_path.as_deref(),
        rec.raw_audio_path.as_deref(),
    ]
    .into_iter()
    .flatten()
    .find(|name| {
        !name.contains("..") && !Path::new(name).is_absolute() && root.join(name).is_file()
    })
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

#[cfg(test)]
mod tests {
    use super::*;

    struct CommandFixture {
        app: tauri::App<tauri::test::MockRuntime>,
        _directory: tempfile::TempDir,
    }

    impl CommandFixture {
        fn new() -> Self {
            let directory = tempfile::tempdir().unwrap();
            let ctx = Arc::new(AppContext::initialize(directory.path().to_path_buf()).unwrap());
            let app = tauri::test::mock_builder()
                .invoke_handler(tauri::generate_handler![
                    get_session_state,
                    get_overlay_snapshot,
                    get_settings,
                    acknowledge_first_run_disclosure,
                    list_history,
                    list_history_summaries,
                    search_history,
                    get_usage_statistics,
                    get_recording,
                    recording_audio_url,
                    get_runtime_info,
                    preview_retention_settings,
                    run_retention,
                    preview_dsp,
                    stop_input_meter,
                    overlay_timing,
                    overlay_timeline,
                    overlay_mark_frame,
                ])
                .build(tauri::test::mock_context(tauri::test::noop_assets()))
                .unwrap();
            assert!(app.manage(ctx));
            Self {
                app,
                _directory: directory,
            }
        }

        fn state(&self) -> State<'_, Arc<AppContext>> {
            self.app.state()
        }
    }

    fn invoke_command(
        webview: &tauri::WebviewWindow<tauri::test::MockRuntime>,
        command: &str,
        body: serde_json::Value,
    ) -> Result<tauri::ipc::InvokeResponseBody, serde_json::Value> {
        tauri::test::get_ipc_response(
            webview,
            tauri::webview::InvokeRequest {
                cmd: command.into(),
                callback: tauri::ipc::CallbackFn(0),
                error: tauri::ipc::CallbackFn(1),
                url: "http://tauri.localhost".parse().unwrap(),
                body: tauri::ipc::InvokeBody::Json(body),
                headers: Default::default(),
                invoke_key: tauri::test::INVOKE_KEY.to_string(),
            },
        )
    }

    #[test]
    fn retention_and_preview_ipc_preserve_error_codes_and_camel_case_contracts() {
        let fixture = CommandFixture::new();
        let webview = tauri::WebviewWindowBuilder::new(&fixture.app, "main", Default::default())
            .build()
            .unwrap();
        let preview = invoke_command(
            &webview,
            "preview_retention_settings",
            serde_json::json!({ "settings": get_settings(fixture.state()) }),
        )
        .unwrap()
        .deserialize::<serde_json::Value>()
        .unwrap();
        assert_eq!(
            preview,
            serde_json::json!({
                "entriesToDelete": 0,
                "recordingIdsToDelete": [],
                "filesToDelete": 0,
                "bytesToFree": 0,
                "protectedEntries": 0,
                "protectedBytes": 0,
                "totalAudioBytes": 0,
            })
        );
        let invalid = invoke_command(
            &webview,
            "preview_retention_settings",
            serde_json::json!({ "settings": { "retention": "1d" } }),
        )
        .unwrap_err();
        assert!(invalid.to_string().contains("settings"));
        let missing_sample =
            invoke_command(&webview, "preview_dsp", serde_json::json!({})).unwrap_err();
        assert_eq!(missing_sample["code"], "StorageFailed");
        assert_eq!(missing_sample["detail"], "no filter sample");
        let stopped = invoke_command(
            &webview,
            "stop_input_meter",
            serde_json::json!({ "owner": "no-capture" }),
        )
        .unwrap()
        .deserialize::<serde_json::Value>()
        .unwrap();
        assert!(stopped.is_null());
        assert!(invoke_command(&webview, "stop_input_meter", serde_json::json!({})).is_err());
        let deleted = invoke_command(&webview, "run_retention", serde_json::json!({}))
            .unwrap()
            .deserialize::<Vec<String>>()
            .unwrap();
        assert!(deleted.is_empty());
        assert!(list_history(fixture.state()).unwrap().is_empty());
    }

    #[test]
    fn retention_preview_is_read_only_and_excludes_live_and_retry_owned_audio() {
        let fixture = CommandFixture::new();
        let root = crate::history::repository::audio_dir(&fixture.state().data_dir);
        let mut records = Vec::new();
        for (index, size) in [10usize, 20, 30].into_iter().enumerate() {
            let mut rec = crate::history::repository::new_recording("fixture/model".into());
            rec.created_at -= chrono::Duration::days(5);
            let name = format!("{}.raw.wav", rec.id);
            rec.raw_audio_path = Some(name.clone());
            std::fs::write(root.join(name), vec![0u8; size]).unwrap();
            fixture.state().history.lock().insert(&rec).unwrap();
            if index == 1 {
                *fixture.state().session_recording_id.lock() = Some(rec.id.clone());
            } else if index == 2 {
                fixture.state().in_flight.lock().insert(rec.id.clone());
            }
            records.push(rec);
        }
        let previous = get_settings(fixture.state());
        let mut proposed = previous.clone();
        proposed.retention = "1d".into();
        proposed.storage_limit = "unlimited".into();
        let preview = preview_retention_settings(fixture.state(), proposed).unwrap();
        assert_eq!(preview.recording_ids_to_delete, [records[0].id.clone()]);
        assert_eq!((preview.entries_to_delete, preview.files_to_delete), (1, 1));
        assert_eq!((preview.bytes_to_free, preview.protected_bytes), (10, 50));
        assert_eq!(
            (preview.protected_entries, preview.total_audio_bytes),
            (2, 60)
        );
        assert_eq!(get_settings(fixture.state()), previous);
        assert_eq!(list_history(fixture.state()).unwrap().len(), 3);
        for rec in records {
            assert!(root.join(rec.raw_audio_path.unwrap()).is_file());
        }
        let mut invalid = previous;
        invalid.retention = "invalid".into();
        assert!(matches!(
            preview_retention_settings(fixture.state(), invalid),
            Err(AppError::RequestValidationFailed(_))
        ));
    }

    #[test]
    fn retention_command_waits_for_recovery_and_live_ownership_then_keeps_usage() {
        let fixture = CommandFixture::new();
        let root = crate::history::repository::audio_dir(&fixture.state().data_dir);
        let mut rec = crate::history::repository::new_recording("fixture/model".into());
        rec.created_at -= chrono::Duration::days(5);
        rec.raw_audio_path = Some(format!("{}.raw.wav", rec.id));
        let raw = root.join(rec.raw_audio_path.as_ref().unwrap());
        std::fs::write(&raw, b"retained until owner releases").unwrap();
        std::fs::write(root.join("filter-sample.wav"), b"filter fixture").unwrap();
        fixture.state().history.lock().insert(&rec).unwrap();
        {
            let state = fixture.state();
            let mut settings = state.settings.lock();
            settings.retention = "1d".into();
            settings.storage_limit = "unlimited".into();
        }
        *fixture.state().settings_recovery.lock() = true;
        assert!(run_retention(fixture.state()).unwrap().is_empty());
        assert!(raw.is_file());
        *fixture.state().settings_recovery.lock() = false;
        *fixture.state().session_recording_id.lock() = Some(rec.id.clone());
        assert!(run_retention(fixture.state()).unwrap().is_empty());
        assert!(raw.is_file());
        *fixture.state().session_recording_id.lock() = None;
        assert_eq!(run_retention(fixture.state()).unwrap(), [rec.id.clone()]);
        assert!(!raw.exists());
        assert!(get_recording(fixture.state(), rec.id).unwrap().is_none());
        assert!(root.join("filter-sample.wav").is_file());
        let stats = get_usage_statistics(
            fixture.state(),
            (rec.created_at - chrono::Duration::seconds(1)).to_rfc3339(),
            (rec.created_at + chrono::Duration::seconds(1)).to_rfc3339(),
        )
        .unwrap();
        assert_eq!(stats.dictations, 1);
        assert_eq!(stats.models[0].model, "fixture/model");
        assert!(run_retention(fixture.state()).unwrap().is_empty());
    }

    #[test]
    fn history_commands_propagate_database_failure_and_recover_without_deleting_audio() {
        let fixture = CommandFixture::new();
        let mut rec = crate::history::repository::new_recording("fixture/model".into());
        rec.raw_audio_path = Some(format!("{}.raw.wav", rec.id));
        fixture.state().history.lock().insert(&rec).unwrap();
        let raw = crate::history::repository::audio_dir(&fixture.state().data_dir)
            .join(rec.raw_audio_path.as_ref().unwrap());
        std::fs::write(&raw, b"preserve on lookup failure").unwrap();
        let fault =
            rusqlite::Connection::open(fixture.state().data_dir.join("history.sqlite")).unwrap();
        fault
            .execute_batch("ALTER TABLE recordings RENAME TO unavailable_recordings")
            .unwrap();
        assert!(matches!(
            list_history(fixture.state()),
            Err(AppError::StorageFailed(_))
        ));
        assert!(matches!(
            list_history_summaries(fixture.state(), None, None, None),
            Err(AppError::StorageFailed(_))
        ));
        assert!(matches!(
            search_history(fixture.state(), "fixture".into(), None, None),
            Err(AppError::StorageFailed(_))
        ));
        assert!(matches!(
            get_recording(fixture.state(), rec.id.clone()),
            Err(AppError::StorageFailed(_))
        ));
        assert!(matches!(
            recording_audio_url(fixture.state(), rec.id.clone()),
            Err(AppError::StorageFailed(_))
        ));
        assert!(matches!(
            preview_retention_settings(fixture.state(), get_settings(fixture.state())),
            Err(AppError::StorageFailed(_))
        ));
        assert!(matches!(
            run_retention(fixture.state()),
            Err(AppError::StorageFailed(_))
        ));
        assert!(raw.is_file());
        fault
            .execute_batch("ALTER TABLE unavailable_recordings RENAME TO recordings")
            .unwrap();
        assert_eq!(
            get_recording(fixture.state(), rec.id)
                .unwrap()
                .unwrap()
                .raw_audio_path,
            rec.raw_audio_path
        );
        assert_eq!(list_history(fixture.state()).unwrap().len(), 1);
    }

    #[tokio::test]
    async fn dsp_preview_command_validates_sample_and_settings_before_processing() {
        let fixture = CommandFixture::new();
        assert!(
            matches!(preview_dsp(fixture.state()).await, Err(AppError::StorageFailed(message)) if message == "no filter sample")
        );
        let source = crate::history::repository::audio_dir(&fixture.state().data_dir)
            .join("filter-sample.wav");
        std::fs::write(&source, b"invalid wave").unwrap();
        assert!(preview_dsp(fixture.state()).await.is_err());
        write_pcm16_wav(&source, SAMPLE_RATE, &[0.1; 4800]).unwrap();
        fixture.state().settings.lock().mic_tune.gain_db = f32::NAN;
        assert!(preview_dsp(fixture.state()).await.is_err());
        fixture.state().settings.lock().mic_tune = Default::default();
        let source_bytes = std::fs::read(&source).unwrap();
        let preview = preview_dsp(fixture.state()).await.unwrap();
        assert!(Path::new(&preview.original_path).is_file());
        assert!(Path::new(&preview.processed_path).is_file());
        assert_ne!(preview.original_path, preview.processed_path);
        let processed = read_pcm16_wav(Path::new(&preview.processed_path)).unwrap();
        assert!(!processed.is_empty());
        assert!(processed
            .iter()
            .all(|sample| sample.is_finite() && sample.abs() <= 1.0));
        assert!(preview.peak.is_finite() && preview.rms.is_finite());
        assert_eq!(std::fs::read(source).unwrap(), source_bytes);
    }

    #[tokio::test]
    async fn meter_stop_command_checks_owner_and_cleans_real_worker_result_on_success_or_error() {
        for fail in [false, true] {
            let fixture = CommandFixture::new();
            let path = fixture.state().data_dir.join("meter-fixture.wav");
            let worker_path = path.clone();
            let (release, wait) = std::sync::mpsc::channel();
            let worker = std::thread::spawn(move || {
                wait.recv().unwrap();
                std::fs::write(&worker_path, b"meter fixture").unwrap();
                if fail {
                    Err(AppError::AudioCaptureFailed(
                        "fixture capture failure".into(),
                    ))
                } else {
                    Ok(crate::audio::capture::CaptureResult {
                        path: worker_path,
                        duration_ms: 100,
                        sample_rate: SAMPLE_RATE,
                        samples: vec![0.1; 8],
                        truncated: false,
                        capture_error: None,
                    })
                }
            });
            *fixture.state().meter_owner.lock() = Some("owner".into());
            *fixture.state().meter_monitor.lock() =
                Some(CaptureSession::from_test_worker(worker, path.clone()));
            stop_input_meter(fixture.state(), "stale-owner".into())
                .await
                .unwrap();
            assert_eq!(fixture.state().meter_owner.lock().as_deref(), Some("owner"));
            assert!(fixture.state().meter_monitor.lock().is_some());
            release.send(()).unwrap();
            let result = stop_input_meter(fixture.state(), "owner".into()).await;
            if fail {
                assert!(
                    matches!(result, Err(AppError::AudioCaptureFailed(message)) if message == "fixture capture failure")
                );
            } else {
                result.unwrap();
            }
            assert!(fixture.state().meter_owner.lock().is_none());
            assert!(fixture.state().meter_monitor.lock().is_none());
            assert!(!fixture
                .state()
                .meter_starting
                .load(std::sync::atomic::Ordering::SeqCst));
            assert!(!path.exists());
            stop_input_meter(fixture.state(), "owner".into())
                .await
                .unwrap();
        }
    }

    #[test]
    fn overlay_timing_commands_track_first_marks_and_clear_after_hide() {
        let fixture = CommandFixture::new();
        assert!(overlay_timing(fixture.state()).is_none());
        fixture.state().overlay_timeline.lock().begin_show();
        overlay_mark_frame(fixture.state(), "unknown".into());
        assert!(overlay_timeline(fixture.state()).react_mounted_ms.is_none());
        overlay_mark_frame(fixture.state(), "react".into());
        overlay_mark_frame(fixture.state(), "frame".into());
        let first = overlay_timeline(fixture.state());
        assert_eq!(first.hotkey_ms, Some(0));
        assert!(first.react_mounted_ms.is_some());
        assert!(first.first_frame_ms.is_some());
        assert!(overlay_timing(fixture.state()).is_some());
        overlay_mark_frame(fixture.state(), "react".into());
        overlay_mark_frame(fixture.state(), "frame".into());
        assert_eq!(overlay_timeline(fixture.state()), first);
        fixture.state().overlay_timeline.lock().hide();
        assert!(overlay_timing(fixture.state()).is_none());
        let hidden = overlay_timeline(fixture.state());
        assert!(hidden.hotkey_ms.is_none());
        assert!(hidden.first_frame_ms.is_none());
    }

    #[test]
    fn history_and_statistics_ipc_commands_serialize_aggregates_and_validate_arguments() {
        let fixture = CommandFixture::new();
        let webview = tauri::WebviewWindowBuilder::new(&fixture.app, "main", Default::default())
            .build()
            .unwrap();
        let invoke =
            |command: &str, body: serde_json::Value| invoke_command(&webview, command, body);
        let idle = invoke("get_session_state", serde_json::json!({}))
            .unwrap()
            .deserialize::<serde_json::Value>()
            .unwrap();
        assert_eq!(idle, serde_json::json!({"kind": "idle"}));
        let history = invoke("list_history", serde_json::json!({}))
            .unwrap()
            .deserialize::<Vec<Recording>>()
            .unwrap();
        assert!(history.is_empty());
        let now = chrono::Utc::now();
        let stats = invoke(
            "get_usage_statistics",
            serde_json::json!({
                "start": now.to_rfc3339(),
                "end": (now + chrono::Duration::seconds(1)).to_rfc3339(),
            }),
        )
        .unwrap()
        .deserialize::<serde_json::Value>()
        .unwrap();
        assert_eq!(stats["dictations"], 0);
        assert_eq!(stats["apiRequests"], 0);
        assert!(stats["daily"].is_array());
        assert!(stats["models"].is_array());
        assert!(stats.get("transcript").is_none());
        let missing_argument = invoke("get_usage_statistics", serde_json::json!({})).unwrap_err();
        assert!(missing_argument.to_string().contains("start"));
        assert!(invoke("get_recording", serde_json::json!({"id": 42})).is_err());
    }

    #[test]
    fn state_commands_read_the_managed_context_without_exposing_transcripts() {
        let fixture = CommandFixture::new();
        assert_eq!(
            get_settings(fixture.state()),
            *fixture.state().settings.lock()
        );
        assert_eq!(get_session_state(fixture.state()), SessionState::Idle);
        let snapshot = get_overlay_snapshot(fixture.state());
        let json = serde_json::to_value(snapshot).unwrap();
        assert!(json.get("transcript").is_none());
        *fixture.state().settings_recovery.lock() = true;
        assert!(get_runtime_info(fixture.state()).settings_recovered);
    }

    #[test]
    fn disclosure_command_persists_completion_and_preserves_other_settings() {
        let fixture = CommandFixture::new();
        let before = fixture.state().settings.lock().clone();
        let saved = acknowledge_first_run_disclosure(fixture.state()).unwrap();
        assert!(saved.first_run_complete);
        assert_eq!(saved.write_seq, before.write_seq + 1);
        let mut expected = before;
        expected.first_run_complete = true;
        expected.write_seq = saved.write_seq;
        assert_eq!(saved, expected);
        assert_eq!(
            AppSettings::load(&fixture.state().settings_path).unwrap(),
            saved
        );
        assert_eq!(get_settings(fixture.state()), saved);
    }

    #[test]
    fn disclosure_command_does_not_publish_completion_when_disk_write_fails() {
        let fixture = CommandFixture::new();
        let before = get_settings(fixture.state());
        before.save(&fixture.state().settings_path).unwrap();
        std::fs::create_dir(fixture.state().settings_path.with_extension("json.tmp")).unwrap();
        assert!(acknowledge_first_run_disclosure(fixture.state()).is_err());
        assert_eq!(get_settings(fixture.state()), before);
        assert_eq!(
            AppSettings::load(&fixture.state().settings_path).unwrap(),
            before
        );
    }

    #[test]
    fn history_commands_page_and_search_real_sqlite_records() {
        let fixture = CommandFixture::new();
        let mut older = crate::history::repository::new_recording("provider/old".into());
        older.created_at -= chrono::Duration::minutes(1);
        older.transcript = Some("Older fixture transcript".into());
        let mut newer = crate::history::repository::new_recording("provider/new".into());
        newer.transcript = Some("Newer fixture transcript".into());
        for rec in [&older, &newer] {
            fixture.state().history.lock().insert(rec).unwrap();
        }
        let all = list_history(fixture.state()).unwrap();
        assert_eq!(
            all.iter().map(|rec| &rec.id).collect::<Vec<_>>(),
            [&newer.id, &older.id]
        );
        assert_eq!(
            get_recording(fixture.state(), older.id.clone()).unwrap(),
            Some(older.clone())
        );
        assert!(get_recording(fixture.state(), "missing".into())
            .unwrap()
            .is_none());
        let first = list_history_summaries(fixture.state(), None, Some(1), None).unwrap();
        assert_eq!(first.total, 2);
        assert!(first.has_more);
        assert_eq!(first.items[0].id, newer.id);
        let second =
            list_history_summaries(fixture.state(), first.next_cursor, Some(1), None).unwrap();
        assert!(!second.has_more);
        assert_eq!(second.items[0].id, older.id);
        let found = search_history(fixture.state(), "Older fixture".into(), None, None).unwrap();
        assert_eq!(found.total, 1);
        assert_eq!(found.items[0].id, older.id);
        assert_eq!(
            list_history_summaries(fixture.state(), None, None, Some("missing".into()))
                .unwrap()
                .total,
            0
        );
    }

    #[test]
    fn statistics_command_counts_attempts_and_costs_without_history_payloads() {
        let fixture = CommandFixture::new();
        let mut rec = crate::history::repository::new_recording("provider/model".into());
        rec.status = crate::history::repository::RecordingStatus::Completed;
        rec.duration_ms = 25_000;
        rec.transcript = Some("Private fixture transcript".into());
        fixture.state().history.lock().insert(&rec).unwrap();
        for (number, cost) in [(1, None), (2, Some(0.12))] {
            fixture
                .state()
                .history
                .lock()
                .record_attempt(
                    &crate::history::repository::TranscriptionAttempt {
                        id: format!("attempt-{number}"),
                        recording_id: rec.id.clone(),
                        attempt_number: number,
                        started_at: rec.created_at,
                        ended_at: Some(rec.created_at),
                        outcome: if cost.is_some() { "success" } else { "failed" }.into(),
                        error_category: None,
                        http_status: Some(if cost.is_some() { 200 } else { 500 }),
                        latency_ms: Some(20),
                    },
                    cost,
                )
                .unwrap();
        }
        let stats = get_usage_statistics(
            fixture.state(),
            (rec.created_at - chrono::Duration::seconds(1)).to_rfc3339(),
            (rec.created_at + chrono::Duration::seconds(1)).to_rfc3339(),
        )
        .unwrap();
        assert_eq!(
            (stats.dictations, stats.completed, stats.api_requests),
            (1, 1, 2)
        );
        assert_eq!(stats.audio_duration_ms, 25_000);
        assert_eq!(stats.unpriced_attempts, 1);
        assert!((stats.reported_cost_usd - 0.12).abs() < f64::EPSILON);
        assert_eq!(stats.models[0].api_requests, 2);
        assert_eq!(
            stats.daily.iter().map(|day| day.api_requests).sum::<i64>(),
            2
        );
        let serialized = serde_json::to_string(&stats).unwrap();
        assert!(!serialized.contains("Private fixture transcript"));
        assert!(!serialized.contains("rawAudioPath"));
        assert!(!serialized.contains("processedAudioPath"));
    }

    #[test]
    fn statistics_command_validates_local_calendar_range_and_exclusive_end() {
        use chrono::TimeZone;
        let fixture = CommandFixture::new();
        let start = chrono::Local
            .with_ymd_and_hms(2026, 1, 1, 0, 0, 0)
            .earliest()
            .unwrap();
        let end = start.checked_add_days(chrono::Days::new(90)).unwrap();
        assert!(
            get_usage_statistics(fixture.state(), start.to_rfc3339(), end.to_rfc3339()).is_ok()
        );
        let too_long = end + chrono::Duration::nanoseconds(1);
        assert!(
            get_usage_statistics(fixture.state(), start.to_rfc3339(), too_long.to_rfc3339())
                .is_err()
        );
        assert!(
            get_usage_statistics(fixture.state(), end.to_rfc3339(), start.to_rfc3339()).is_err()
        );
        assert!(
            get_usage_statistics(fixture.state(), start.to_rfc3339(), start.to_rfc3339()).is_err()
        );
        assert!(get_usage_statistics(fixture.state(), "invalid".into(), end.to_rfc3339()).is_err());
        assert!(
            get_usage_statistics(fixture.state(), start.to_rfc3339(), "invalid".into()).is_err()
        );
    }

    #[test]
    fn audio_url_command_uses_existing_processed_then_raw_and_rejects_unsafe_names() {
        let fixture = CommandFixture::new();
        let root = crate::history::repository::audio_dir(&fixture.state().data_dir);
        let mut rec = crate::history::repository::new_recording("provider/model".into());
        rec.processed_audio_path = Some("processed.wav".into());
        rec.raw_audio_path = Some("raw.wav".into());
        fixture.state().history.lock().insert(&rec).unwrap();
        assert!(recording_audio_url(fixture.state(), rec.id.clone())
            .unwrap()
            .is_none());
        std::fs::write(root.join("raw.wav"), b"raw fixture").unwrap();
        assert_eq!(
            recording_audio_url(fixture.state(), rec.id.clone()).unwrap(),
            Some(
                root.join("raw.wav")
                    .canonicalize()
                    .unwrap()
                    .to_string_lossy()
                    .into_owned()
            )
        );
        std::fs::write(root.join("processed.wav"), b"processed fixture").unwrap();
        assert_eq!(
            recording_audio_url(fixture.state(), rec.id.clone()).unwrap(),
            Some(
                root.join("processed.wav")
                    .canonicalize()
                    .unwrap()
                    .to_string_lossy()
                    .into_owned()
            )
        );
        let outside = fixture.state().data_dir.join("outside.wav");
        std::fs::write(&outside, b"private fixture").unwrap();
        for unsafe_name in [
            "../outside.wav".to_string(),
            outside.to_string_lossy().into_owned(),
        ] {
            rec.processed_audio_path = Some(unsafe_name);
            rec.raw_audio_path = None;
            fixture.state().history.lock().update(&rec).unwrap();
            assert!(recording_audio_url(fixture.state(), rec.id.clone())
                .unwrap()
                .is_none());
        }
        assert!(recording_audio_url(fixture.state(), "missing".into()).is_err());
    }

    #[test]
    fn filter_start_publishes_only_current_success_and_keeps_capture_reserved() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = AppContext::initialize(dir.path().to_path_buf()).unwrap();
        ctx.filter_generation
            .store(7, std::sync::atomic::Ordering::SeqCst);
        ctx.filter_recording
            .store(true, std::sync::atomic::Ordering::SeqCst);
        let published = std::cell::Cell::new(None);
        finish_filter_start(
            &ctx,
            7,
            Ok(42),
            |result| published.set(Some(result)),
            |_| panic!("a current capture must not be discarded"),
        )
        .unwrap();
        assert_eq!(published.get(), Some(42));
        assert!(crate::app::operations::system_busy(&ctx));
    }

    #[test]
    fn failed_filter_start_releases_reservation_and_preserves_current_error() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = AppContext::initialize(dir.path().to_path_buf()).unwrap();
        ctx.filter_generation
            .store(7, std::sync::atomic::Ordering::SeqCst);
        for generation in [7, 6] {
            ctx.filter_recording
                .store(true, std::sync::atomic::Ordering::SeqCst);
            let result = finish_filter_start::<()>(
                &ctx,
                generation,
                Err(AppError::AudioCaptureFailed("device unavailable".into())),
                |_| panic!("a failed capture must not be published"),
                |_| panic!("a failed capture has no result to discard"),
            );
            if generation == 7 {
                assert!(
                    matches!(result, Err(AppError::AudioCaptureFailed(message)) if message == "device unavailable")
                );
            } else {
                assert!(matches!(result, Err(AppError::Cancelled)));
            }
            assert!(!crate::app::operations::system_busy(&ctx));
        }
    }

    #[test]
    fn partial_delete_all_preserves_protected_and_retained_unrelated_usage() {
        let dir = tempfile::tempdir().unwrap();
        let history =
            crate::history::repository::HistoryRepo::open(&dir.path().join("history.sqlite"))
                .unwrap();
        let deleted = crate::history::repository::new_recording("deleted/model".into());
        let protected = crate::history::repository::new_recording("protected/model".into());
        let retained = crate::history::repository::new_recording("retained/model".into());
        for rec in [&deleted, &protected, &retained] {
            history.insert(rec).unwrap();
        }
        assert!(history.delete(&deleted.id).unwrap());
        assert!(history.delete(&retained.id).unwrap());
        let result = crate::history::retention::DeleteAllResult {
            deleted: vec![deleted.id.clone()],
            failed: vec![protected.id.clone()],
        };
        let protected_ids = std::collections::HashSet::from([protected.id.clone()]);
        clear_usage_after_delete_all(&history, &result, &protected_ids).unwrap();

        let start = deleted.created_at - chrono::Duration::seconds(1);
        let end = retained.created_at + chrono::Duration::seconds(1);
        let stats = history.get_usage_statistics(start, end).unwrap();
        assert_eq!(stats.dictations, 2);
        let models: std::collections::HashSet<_> =
            stats.models.iter().map(|row| row.model.as_str()).collect();
        assert_eq!(
            models,
            std::collections::HashSet::from(["protected/model", "retained/model"])
        );
        assert!(history.get(&protected.id).unwrap().is_some());
    }

    #[test]
    fn filter_capture_reservation_is_only_released_by_its_own_stop_generation() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = AppContext::initialize(dir.path().to_path_buf()).unwrap();
        ctx.filter_recording
            .store(true, std::sync::atomic::Ordering::SeqCst);
        let generation = ctx
            .filter_generation
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst)
            + 1;

        release_filter_capture_reservation(&ctx, generation.saturating_sub(1));
        assert!(ctx
            .filter_recording
            .load(std::sync::atomic::Ordering::SeqCst));
        assert!(crate::app::operations::system_busy(&ctx));

        release_filter_capture_reservation(&ctx, generation);
        assert!(!ctx
            .filter_recording
            .load(std::sync::atomic::Ordering::SeqCst));
        assert!(!crate::app::operations::system_busy(&ctx));
    }

    #[test]
    fn filter_timeout_keeps_busy_until_the_late_native_capture_result() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = Arc::new(AppContext::initialize(dir.path().to_path_buf()).unwrap());
        ctx.filter_recording
            .store(true, std::sync::atomic::Ordering::SeqCst);
        let generation = ctx
            .filter_generation
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst)
            + 1;
        let path = dir.path().join("late-filter.wav");
        let (release, wait) = std::sync::mpsc::channel();
        let worker_path = path.clone();
        let worker = std::thread::spawn(move || {
            wait.recv().unwrap();
            std::fs::write(&worker_path, b"late filter sample").unwrap();
            Ok(crate::audio::capture::CaptureResult {
                path: worker_path,
                duration_ms: 100,
                sample_rate: SAMPLE_RATE,
                samples: vec![0.1; 8],
                truncated: false,
                capture_error: None,
            })
        });
        let session = CaptureSession::from_test_worker(worker, path);
        let late_handler = filter_late_result_handler(ctx.clone(), generation, None);

        let outcome = session
            .stop_with_late_result_timeout(std::time::Duration::from_millis(20), move |_| {
                late_handler()
            });
        assert!(matches!(outcome, CaptureStopOutcome::TimedOut(_)));
        assert!(crate::app::operations::system_busy(&ctx));

        release.send(()).unwrap();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
        while ctx
            .filter_recording
            .load(std::sync::atomic::Ordering::SeqCst)
            && std::time::Instant::now() < deadline
        {
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        assert!(!ctx
            .filter_recording
            .load(std::sync::atomic::Ordering::SeqCst));
        assert!(!crate::app::operations::system_busy(&ctx));
    }

    #[test]
    fn late_filter_stop_releases_reservation_after_shutdown_invalidates_generation() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = Arc::new(AppContext::initialize(dir.path().to_path_buf()).unwrap());
        ctx.filter_recording
            .store(true, std::sync::atomic::Ordering::SeqCst);
        let stop_generation = ctx
            .filter_generation
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst)
            + 1;
        let handler = filter_late_result_handler(ctx.clone(), stop_generation, None);

        ctx.shutdown_requested
            .store(true, std::sync::atomic::Ordering::SeqCst);
        ctx.filter_generation
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        handler();

        assert!(!ctx
            .filter_recording
            .load(std::sync::atomic::Ordering::SeqCst));
    }

    #[test]
    fn later_preview_preserves_the_audio_of_an_earlier_result() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = AppContext::initialize(dir.path().to_path_buf()).unwrap();
        let source = crate::history::repository::audio_dir(dir.path()).join("filter-sample.wav");
        write_pcm16_wav(&source, SAMPLE_RATE, &[0.1; 4800]).unwrap();
        ctx.settings.lock().active_preset_id = "stt-fast".into();
        let first = preview_from_original(&ctx, &source).unwrap();
        let first_bytes = std::fs::read(&first.processed_path).unwrap();
        {
            let mut settings = ctx.settings.lock();
            settings
                .presets
                .iter_mut()
                .find(|preset| preset.id == "stt-fast")
                .unwrap()
                .gain
                .db = -12.0;
        }
        let second = preview_from_original(&ctx, &source).unwrap();
        assert_ne!(first.original_path, second.original_path);
        assert_ne!(first.processed_path, second.processed_path);
        assert_eq!(std::fs::read(&first.processed_path).unwrap(), first_bytes);
        assert_ne!(std::fs::read(&second.processed_path).unwrap(), first_bytes);
    }

    #[test]
    fn failed_settings_write_preserves_published_memory_and_saved_snapshot() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = AppContext::initialize(dir.path().to_path_buf()).unwrap();
        let previous = ctx.settings.lock().clone();
        previous.save(&ctx.settings_path).unwrap();
        let mut next = previous.clone();
        next.model = "new/model".into();
        let lockfile = ctx.settings_path.with_extension("json.tmp");
        // Block the actual Settings::save temporary file path.
        std::fs::create_dir(&lockfile).unwrap();
        assert!(write_settings_snapshot(&ctx, &next).is_err());
        assert_eq!(*ctx.settings.lock(), previous);
        assert_eq!(AppSettings::load(&ctx.settings_path).unwrap(), previous);
    }

    #[test]
    fn reordered_settings_requests_do_not_overwrite_the_newer_persisted_snapshot() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = AppContext::initialize(dir.path().to_path_buf()).unwrap();
        let mut older = ctx.settings.lock().clone();
        older.write_seq = 1;
        older.model = "older/model".into();
        let mut newer = older.clone();
        newer.write_seq = 2;
        newer.model = "newer/model".into();
        {
            let _writer = ctx.settings_write_gate.lock();
            let previous = ctx.settings.lock().clone();
            assert!(write_ordered_settings_snapshot(&ctx, &mut newer, &previous).unwrap());
        }
        {
            let _writer = ctx.settings_write_gate.lock();
            let previous = ctx.settings.lock().clone();
            assert!(!write_ordered_settings_snapshot(&ctx, &mut older, &previous).unwrap());
        }
        let saved = AppSettings::load(&ctx.settings_path).unwrap();
        assert_eq!(saved.write_seq, 2);
        assert_eq!(saved.model, "newer/model");
        assert_eq!(*ctx.settings.lock(), saved);
    }

    #[test]
    fn delete_all_clears_retained_usage_even_when_audio_cleanup_failed_after_commit() {
        let dir = tempfile::tempdir().unwrap();
        let history =
            crate::history::repository::HistoryRepo::open(&dir.path().join("history.sqlite"))
                .unwrap();
        let rec = crate::history::repository::new_recording("provider/model".into());
        history.insert(&rec).unwrap();
        history
            .record_attempt(
                &crate::history::repository::TranscriptionAttempt {
                    id: format!("attempt-{}", rec.id),
                    recording_id: rec.id.clone(),
                    attempt_number: 1,
                    started_at: rec.created_at,
                    ended_at: Some(rec.created_at),
                    outcome: "success".into(),
                    error_category: None,
                    http_status: Some(200),
                    latency_ms: Some(20),
                },
                Some(0.01),
            )
            .unwrap();
        assert!(history.delete(&rec.id).unwrap());
        let range_end = rec.created_at + chrono::Duration::seconds(1);
        assert_eq!(
            history
                .get_usage_statistics(rec.created_at, range_end)
                .unwrap()
                .dictations,
            1
        );
        let result = crate::history::retention::DeleteAllResult {
            deleted: vec![rec.id],
            failed: vec!["post-commit audio cleanup".into()],
        };

        clear_usage_after_delete_all(&history, &result, &std::collections::HashSet::new()).unwrap();

        let stats = history
            .get_usage_statistics(rec.created_at, range_end)
            .unwrap();
        assert_eq!((stats.dictations, stats.api_requests), (0, 0));
    }

    #[test]
    fn command_validation_rejects_invalid_retention_and_duplicate_presets() {
        let mut settings = AppSettings::default();
        assert!(validate_settings_graph(&settings).is_ok());
        settings.retention = "typo".into();
        assert!(validate_settings_graph(&settings).is_err());
        settings.retention = "forever".into();
        settings.presets.push(settings.presets[0].clone());
        assert!(validate_settings_graph(&settings).is_err());
    }

    #[test]
    fn command_validation_rejects_nonfinite_and_invalid_dsp() {
        let mut settings = AppSettings::default();
        settings.presets[0].gain.db = f32::NAN;
        assert!(validate_settings_graph(&settings).is_err());
        settings.presets[0].gain.db = 0.0;
        settings.presets[0].compressor.ratio = 0.5;
        assert!(validate_settings_graph(&settings).is_err());
        settings.presets[0].compressor.ratio = 2.0;
        settings.presets[0].rnnoise_mix = 1.1;
        assert!(validate_settings_graph(&settings).is_err());
    }

    #[test]
    fn playback_uses_existing_processed_then_raw_even_with_missing_processed() {
        let dir = tempfile::tempdir().unwrap();
        let mut rec = crate::history::repository::new_recording("model".into());
        rec.raw_audio_path = Some("raw.wav".into());
        rec.processed_audio_path = Some("processed.wav".into());
        assert_eq!(existing_recording_audio(&rec, dir.path()), None);
        std::fs::write(dir.path().join("raw.wav"), b"raw").unwrap();
        assert_eq!(existing_recording_audio(&rec, dir.path()), Some("raw.wav"));
        std::fs::write(dir.path().join("processed.wav"), b"processed").unwrap();
        assert_eq!(
            existing_recording_audio(&rec, dir.path()),
            Some("processed.wav")
        );
    }

    #[test]
    fn stopped_pending_filter_worker_discards_late_result_without_publishing() {
        use std::sync::{
            atomic::{AtomicBool, Ordering},
            Barrier,
        };
        let dir = tempfile::tempdir().unwrap();
        let ctx = Arc::new(AppContext::initialize(dir.path().to_path_buf()).unwrap());
        ctx.filter_recording.store(true, Ordering::SeqCst);
        ctx.filter_generation.store(1, Ordering::SeqCst);
        let barrier = Arc::new(Barrier::new(2));
        let published = Arc::new(AtomicBool::new(false));
        let path = dir.path().join("late-filter.wav");
        let worker_ctx = ctx.clone();
        let worker_barrier = barrier.clone();
        let worker_published = published.clone();
        let worker_path = path.clone();
        let worker = std::thread::spawn(move || {
            worker_barrier.wait();
            std::fs::write(&worker_path, b"late audio").unwrap();
            finish_filter_start(
                &worker_ctx,
                1,
                Ok(worker_path),
                |_| worker_published.store(true, Ordering::SeqCst),
                |path| {
                    std::fs::remove_file(path).unwrap();
                    worker_ctx.filter_recording.store(false, Ordering::SeqCst);
                },
            )
        });
        {
            let _admission = crate::app::operations::lock_admission(&ctx);
            ctx.filter_generation.fetch_add(1, Ordering::SeqCst);
            assert!(crate::app::operations::system_busy(&ctx));
        }
        barrier.wait();
        assert!(matches!(worker.join().unwrap(), Err(AppError::Cancelled)));
        assert!(!published.load(Ordering::SeqCst));
        assert!(!path.exists());
        assert!(!crate::app::operations::system_busy(&ctx));
    }
}
