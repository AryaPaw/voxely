use serde::{Deserialize, Serialize};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::Duration;

use cpal::traits::{DeviceTrait, StreamTrait};
use cpal::{SampleFormat, StreamConfig};
use hound::{WavSpec, WavWriter};
use parking_lot::Mutex;
use rtrb::RingBuffer;

use super::devices::resolve_device;
use super::resample::{f32_to_i16, i16_to_f32, to_mono, LinearStreamResampler};
use crate::dsp::metrics::{SAMPLE_RATE, STT_SAMPLE_RATE};
use crate::error::AppError;
use crate::transcription::limits::MAX_MULTIPART_AUDIO_FILE_BYTES;

const QUEUE_CAP: usize = 48_000 * 2;
const WAV_HEADER_BYTES: u64 = 44;
const WAV_HEADER_LEN: usize = 44;
const PCM16_BYTES_PER_SAMPLE: u64 = 2;
const MAX_STT_PCM16_FRAMES: u64 =
    (MAX_MULTIPART_AUDIO_FILE_BYTES - WAV_HEADER_BYTES) / PCM16_BYTES_PER_SAMPLE;
pub(crate) const MAX_PCM16_FRAMES: u64 =
    MAX_STT_PCM16_FRAMES * (SAMPLE_RATE as u64 / STT_SAMPLE_RATE as u64);
pub(crate) const MAX_WAV_BYTES: u64 = MAX_PCM16_FRAMES * PCM16_BYTES_PER_SAMPLE + WAV_HEADER_BYTES;
const CAPTURE_POLL: Duration = Duration::from_millis(16);
const START_TIMEOUT: Duration = Duration::from_secs(8);
const HUNG_JOIN_GRACE: Duration = Duration::from_millis(50);
pub(crate) const JOIN_BOUND: Duration = Duration::from_secs(8);

pub const METER_BINS: usize = 48;
const METER_HOPS_PER_SEC: u32 = 16;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MeterSample {
    pub rms: f32,
    pub peak: f32,
    pub levels: Vec<f32>,
    #[serde(skip)]
    pending_peak: f32,
    #[serde(skip)]
    pending_n: u32,
}

impl MeterSample {
    pub fn silent() -> Self {
        Self {
            rms: 0.0,
            peak: 0.0,
            levels: vec![0.0; METER_BINS],
            pending_peak: 0.0,
            pending_n: 0,
        }
    }
}

pub fn meter_hop(sample_rate: u32) -> u32 {
    (sample_rate / METER_HOPS_PER_SEC).max(1)
}

pub struct CaptureSession {
    stop: Arc<AtomicBool>,
    meter: Arc<Mutex<MeterSample>>,
    thread: Option<JoinHandle<Result<CaptureResult, AppError>>>,
    fallback: Arc<AtomicBool>,
    cleanup: CaptureCleanupHandle,
}

#[derive(Clone)]
pub struct CaptureCleanupHandle(Arc<CaptureCleanupState>);

struct CaptureCleanupState {
    output_path: Option<PathBuf>,
    discard: AtomicBool,
    finished: AtomicBool,
    cleaning: AtomicBool,
    cleaned: AtomicBool,
}

impl CaptureCleanupHandle {
    pub(crate) fn new(output_path: Option<PathBuf>) -> Self {
        Self(Arc::new(CaptureCleanupState {
            output_path,
            discard: AtomicBool::new(false),
            finished: AtomicBool::new(false),
            cleaning: AtomicBool::new(false),
            cleaned: AtomicBool::new(false),
        }))
    }

    /// Discard output after the capture thread actually exits. Native CPAL work cannot be aborted.
    pub fn discard_late_result(&self) {
        self.0.discard.store(true, Ordering::SeqCst);
        self.cleanup_if_ready();
    }

    pub(crate) fn mark_finished(&self) {
        self.0.finished.store(true, Ordering::SeqCst);
        self.cleanup_if_ready();
    }

    fn cleanup_if_ready(&self) {
        if !self.0.discard.load(Ordering::SeqCst)
            || !self.0.finished.load(Ordering::SeqCst)
            || self.0.cleaned.load(Ordering::SeqCst)
            || self.0.cleaning.swap(true, Ordering::SeqCst)
        {
            return;
        }
        let Some(path) = self.0.output_path.as_deref() else {
            self.0.cleaned.store(true, Ordering::SeqCst);
            self.0.cleaning.store(false, Ordering::SeqCst);
            return;
        };
        let mut cleaned = true;
        for artifact in [path.to_path_buf(), path.with_extension("wav.tmp")] {
            if let Err(err) = std::fs::remove_file(&artifact) {
                if err.kind() != std::io::ErrorKind::NotFound {
                    cleaned = false;
                    tracing::warn!(error = %err, file = %artifact.display(), "remove discarded late capture");
                }
            }
        }
        self.0.cleaned.store(cleaned, Ordering::SeqCst);
        self.0.cleaning.store(false, Ordering::SeqCst);
    }
}

fn defer_start_timeout_cleanup(
    worker: JoinHandle<Result<CaptureResult, AppError>>,
    cleanup: CaptureCleanupHandle,
    on_late_completion: Option<Box<dyn FnOnce() + Send + 'static>>,
) {
    thread::spawn(move || {
        let _ = worker.join();
        cleanup.mark_finished();
        for attempt in 1..=5 {
            if cleanup.0.cleaned.load(Ordering::SeqCst) {
                break;
            }
            thread::sleep(Duration::from_millis(25 * attempt));
            cleanup.discard_late_result();
        }
        if let Some(on_late_completion) = on_late_completion {
            on_late_completion();
        }
    });
}

pub struct CaptureResult {
    pub path: PathBuf,
    pub duration_ms: u64,
    pub sample_rate: u32,
    pub samples: Vec<f32>,
    pub truncated: bool,
    pub capture_error: Option<String>,
}

pub enum CaptureStopOutcome {
    Completed(Result<CaptureResult, AppError>),
    TimedOut(AppError),
}

pub struct CaptureStartFailure {
    pub error: AppError,
    /// True while a timed-out native capture worker still owns the device.
    pub native_worker_pending: bool,
}

pub fn validate_capture_result(result: &CaptureResult) -> Result<(), AppError> {
    if let Some(message) = result.capture_error.as_ref() {
        return Err(AppError::AudioCaptureFailed(message.clone()));
    }
    if result.truncated {
        return Err(AppError::RecordingTruncated);
    }
    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RecoveredCaptureWav {
    pub duration_ms: u64,
    pub truncated: bool,
}

pub fn max_duration_ms() -> u64 {
    (MAX_PCM16_FRAMES * 1000) / u64::from(SAMPLE_RATE)
}

pub fn output_frames_for_input(input_frames: u64, input_rate: u32) -> u64 {
    if input_rate == 0 || input_rate == SAMPLE_RATE {
        return input_frames;
    }
    input_frames.saturating_mul(u64::from(SAMPLE_RATE)) / u64::from(input_rate)
}

impl CaptureSession {
    pub fn start(device_name: Option<&str>, dest: PathBuf) -> Result<Self, AppError> {
        Self::spawn(device_name, Some(dest), None).map_err(|failure| failure.error)
    }

    pub fn start_with_late_completion<F>(
        device_name: Option<&str>,
        dest: PathBuf,
        on_late_completion: F,
    ) -> Result<Self, CaptureStartFailure>
    where
        F: FnOnce() + Send + 'static,
    {
        Self::spawn(device_name, Some(dest), Some(Box::new(on_late_completion)))
    }

    pub fn start_monitor(device_name: Option<&str>) -> Result<Self, AppError> {
        Self::spawn(device_name, None, None).map_err(|failure| failure.error)
    }

    pub fn start_monitor_with_late_completion<F>(
        device_name: Option<&str>,
        on_late_completion: F,
    ) -> Result<Self, CaptureStartFailure>
    where
        F: FnOnce() + Send + 'static,
    {
        Self::spawn(device_name, None, Some(Box::new(on_late_completion)))
    }

    fn spawn(
        device_name: Option<&str>,
        dest: Option<PathBuf>,
        on_late_completion: Option<Box<dyn FnOnce() + Send + 'static>>,
    ) -> Result<Self, CaptureStartFailure> {
        let device_name = device_name.map(str::to_string);
        let cleanup = CaptureCleanupHandle::new(dest.clone());
        let stop = Arc::new(AtomicBool::new(false));
        let meter = Arc::new(Mutex::new(MeterSample::silent()));
        let fallback = Arc::new(AtomicBool::new(false));
        let stop_t = stop.clone();
        let meter_t = meter.clone();
        let fallback_t = fallback.clone();
        let (ready_tx, ready_rx) = std::sync::mpsc::channel();
        let thread = thread::spawn(move || {
            start_stream_and_write(
                device_name.as_deref(),
                dest,
                stop_t,
                meter_t,
                fallback_t,
                ready_tx,
            )
        });
        match ready_rx.recv_timeout(START_TIMEOUT) {
            Ok(Ok(())) => Ok(Self {
                stop,
                meter,
                thread: Some(thread),
                fallback,
                cleanup,
            }),
            Ok(Err(err)) => {
                cleanup.discard_late_result();
                let _ = thread.join();
                cleanup.mark_finished();
                Err(CaptureStartFailure {
                    error: err,
                    native_worker_pending: false,
                })
            }
            Err(_) => {
                stop.store(true, Ordering::SeqCst);
                cleanup.discard_late_result();
                defer_start_timeout_cleanup(thread, cleanup, on_late_completion);
                thread::sleep(HUNG_JOIN_GRACE);
                Err(CaptureStartFailure {
                    error: AppError::AudioCaptureFailed("microphone did not start in time".into()),
                    native_worker_pending: true,
                })
            }
        }
    }

    pub fn meter(&self) -> MeterSample {
        self.meter.lock().clone()
    }

    pub fn is_finished(&self) -> bool {
        self.thread.as_ref().is_none_or(JoinHandle::is_finished)
    }

    pub fn used_fallback_device(&self) -> bool {
        self.fallback.load(Ordering::SeqCst)
    }

    pub fn cleanup_handle(&self) -> CaptureCleanupHandle {
        self.cleanup.clone()
    }

    #[cfg(test)]
    pub(crate) fn from_test_worker(
        worker: JoinHandle<Result<CaptureResult, AppError>>,
        path: PathBuf,
    ) -> Self {
        Self {
            stop: Arc::new(AtomicBool::new(false)),
            meter: Arc::new(Mutex::new(MeterSample::silent())),
            thread: Some(worker),
            fallback: Arc::new(AtomicBool::new(false)),
            cleanup: CaptureCleanupHandle::new(Some(path)),
        }
    }

    pub fn stop(self) -> Result<CaptureResult, AppError> {
        self.stop_with_timeout(JOIN_BOUND)
    }

    /// Stop a capture and deliver its eventual result if the bounded wait expires.
    pub fn stop_with_late_result<F>(self, on_late_result: F) -> CaptureStopOutcome
    where
        F: FnOnce(Result<CaptureResult, AppError>) + Send + 'static,
    {
        self.stop_with_late_result_timeout(JOIN_BOUND, on_late_result)
    }

    pub(crate) fn stop_with_late_result_timeout<F>(
        self,
        join_bound: Duration,
        on_late_result: F,
    ) -> CaptureStopOutcome
    where
        F: FnOnce(Result<CaptureResult, AppError>) + Send + 'static,
    {
        self.stop_with_timeout_and_late_result(join_bound, on_late_result)
    }

    /// Stop a capture whose output will not be used, including when the native worker exits late.
    pub fn stop_and_discard(self) -> Result<CaptureResult, AppError> {
        self.stop_and_discard_with_timeout(JOIN_BOUND)
    }

    fn stop_and_discard_with_timeout(
        self,
        join_bound: Duration,
    ) -> Result<CaptureResult, AppError> {
        let cleanup = self.cleanup_handle();
        let result = self.stop_with_timeout(join_bound);
        cleanup.discard_late_result();
        result
    }

    fn stop_with_timeout(self, join_bound: Duration) -> Result<CaptureResult, AppError> {
        match self.stop_with_timeout_and_late_result(join_bound, |_| {}) {
            CaptureStopOutcome::Completed(result) => result,
            CaptureStopOutcome::TimedOut(err) => Err(err),
        }
    }

    fn stop_with_timeout_and_late_result<F>(
        mut self,
        join_bound: Duration,
        on_late_result: F,
    ) -> CaptureStopOutcome
    where
        F: FnOnce(Result<CaptureResult, AppError>) + Send + 'static,
    {
        self.stop.store(true, Ordering::SeqCst);
        let Some(handle) = self.thread.take() else {
            return CaptureStopOutcome::Completed(Err(AppError::AudioCaptureFailed(
                "capture thread missing".into(),
            )));
        };
        if handle.is_finished() {
            let joined = handle.join();
            self.cleanup.mark_finished();
            return CaptureStopOutcome::Completed(joined.unwrap_or_else(|_| {
                Err(AppError::AudioCaptureFailed(
                    "capture thread panicked".into(),
                ))
            }));
        }
        let (tx, rx) = std::sync::mpsc::channel();
        let late_cleanup = self.cleanup.clone();
        thread::spawn(move || {
            let joined = handle.join();
            late_cleanup.mark_finished();
            let _ = tx.send(joined);
        });
        match rx.recv_timeout(join_bound) {
            Ok(joined) => {
                self.cleanup.mark_finished();
                CaptureStopOutcome::Completed(joined.unwrap_or_else(|_| {
                    Err(AppError::AudioCaptureFailed(
                        "capture thread panicked".into(),
                    ))
                }))
            }
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                thread::spawn(move || {
                    if let Ok(joined) = rx.recv() {
                        on_late_result(joined.unwrap_or_else(|_| {
                            Err(AppError::AudioCaptureFailed(
                                "capture thread panicked".into(),
                            ))
                        }));
                    }
                });
                CaptureStopOutcome::TimedOut(AppError::AudioCaptureFailed(format!(
                    "capture thread did not stop in time ({:.3}s); it remains detached until native capture exits",
                    join_bound.as_secs_f64()
                )))
            }
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                CaptureStopOutcome::Completed(Err(AppError::AudioCaptureFailed(
                    "capture result worker disconnected".into(),
                )))
            }
        }
    }

    pub fn discard(self) {
        let _ = self.stop();
    }
}

impl Drop for CaptureSession {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(thread) = self.thread.take() {
            let cleanup = self.cleanup.clone();
            thread::spawn(move || {
                let _ = thread.join();
                cleanup.mark_finished();
            });
        }
    }
}

fn start_stream_and_write(
    device_name: Option<&str>,
    dest: Option<PathBuf>,
    stop: Arc<AtomicBool>,
    meter: Arc<Mutex<MeterSample>>,
    fallback: Arc<AtomicBool>,
    ready_tx: std::sync::mpsc::Sender<Result<(), AppError>>,
) -> Result<CaptureResult, AppError> {
    match start_stream_inner(device_name, meter, fallback) {
        Ok(built) => {
            let _ = ready_tx.send(Ok(()));
            match dest {
                Some(path) => run_capture(built, path, stop),
                None => run_monitor(built, stop),
            }
        }
        Err(err) => {
            let _ = ready_tx.send(Err(err.clone()));
            Err(err)
        }
    }
}

struct BuiltCapture {
    stream: cpal::Stream,
    consumer: rtrb::Consumer<f32>,
    input_rate: u32,
    overflow: Arc<AtomicU32>,
    stream_error: Arc<Mutex<Option<String>>>,
}

fn start_stream_inner(
    device_name: Option<&str>,
    meter: Arc<Mutex<MeterSample>>,
    fallback: Arc<AtomicBool>,
) -> Result<BuiltCapture, AppError> {
    let (device, used_fallback) =
        resolve_device(device_name).map_err(|_| AppError::MicrophoneUnavailable)?;
    fallback.store(used_fallback, Ordering::SeqCst);
    let supported = device
        .default_input_config()
        .map_err(|e| AppError::AudioCaptureFailed(e.to_string()))?;
    let sample_format = supported.sample_format();
    let config: StreamConfig = supported.clone().into();
    let channels = config.channels as usize;
    let input_rate = config.sample_rate.0;
    let (mut producer, consumer) = RingBuffer::<f32>::new(QUEUE_CAP);
    let overflow = Arc::new(AtomicU32::new(0));
    let overflow_cb = overflow.clone();
    let meter_cb = meter.clone();
    let hop = meter_hop(input_rate);

    let stream_error = Arc::new(Mutex::new(None));

    let stream = match sample_format {
        SampleFormat::F32 => {
            let stream_error = Arc::clone(&stream_error);
            device.build_input_stream(
                &config,
                move |data: &[f32], _| {
                    push_frames(&mut producer, data, channels, &meter_cb, &overflow_cb, hop)
                },
                move |err: cpal::StreamError| record_stream_error(&stream_error, err),
                None,
            )
        }
        SampleFormat::I16 => {
            let stream_error = Arc::clone(&stream_error);
            device.build_input_stream(
                &config,
                move |data: &[i16], _| {
                    let converted = i16_to_f32(data);
                    push_frames(
                        &mut producer,
                        &converted,
                        channels,
                        &meter_cb,
                        &overflow_cb,
                        hop,
                    )
                },
                move |err: cpal::StreamError| record_stream_error(&stream_error, err),
                None,
            )
        }
        SampleFormat::U16 => {
            let stream_error = Arc::clone(&stream_error);
            device.build_input_stream(
                &config,
                move |data: &[u16], _| {
                    let converted: Vec<f32> =
                        data.iter().map(|s| (*s as f32 / 32768.0) - 1.0).collect();
                    push_frames(
                        &mut producer,
                        &converted,
                        channels,
                        &meter_cb,
                        &overflow_cb,
                        hop,
                    )
                },
                move |err: cpal::StreamError| record_stream_error(&stream_error, err),
                None,
            )
        }
        other => {
            return Err(AppError::AudioCaptureFailed(format!(
                "unsupported sample format {other:?}"
            )));
        }
    }
    .map_err(|e| AppError::AudioCaptureFailed(e.to_string()))?;
    stream
        .play()
        .map_err(|e| AppError::AudioCaptureFailed(e.to_string()))?;
    Ok(BuiltCapture {
        stream,
        consumer,
        input_rate,
        overflow,
        stream_error,
    })
}

fn record_stream_error(target: &Arc<Mutex<Option<String>>>, err: cpal::StreamError) {
    let message = err.to_string();
    let mut stored = target.lock();
    if stored.is_none() {
        *stored = Some(message.clone());
    }
    tracing::error!(error = %message, "cpal stream error");
}

fn run_monitor(mut built: BuiltCapture, stop: Arc<AtomicBool>) -> Result<CaptureResult, AppError> {
    while !stop.load(Ordering::SeqCst) {
        drain_discard(&mut built.consumer)?;
        if built.stream_error.lock().is_some() {
            break;
        }
        thread::sleep(CAPTURE_POLL);
    }
    drop(built.stream);
    Ok(CaptureResult {
        path: PathBuf::new(),
        duration_ms: 0,
        sample_rate: SAMPLE_RATE,
        samples: Vec::new(),
        truncated: false,
        capture_error: built.stream_error.lock().clone(),
    })
}

fn run_capture(
    mut built: BuiltCapture,
    dest: PathBuf,
    stop: Arc<AtomicBool>,
) -> Result<CaptureResult, AppError> {
    let spec = WavSpec {
        channels: 1,
        sample_rate: SAMPLE_RATE,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent).map_err(|e| AppError::StorageFailed(e.to_string()))?;
    }
    let tmp = dest.with_extension("wav.tmp");
    let mut writer =
        WavWriter::create(&tmp, spec).map_err(|e| AppError::StorageFailed(e.to_string()))?;
    let mut leftover = Vec::new();
    let mut resampler = LinearStreamResampler::new(built.input_rate, SAMPLE_RATE);
    let mut written = 0u64;
    let mut truncated = false;
    while !stop.load(Ordering::SeqCst) {
        drain_consumer(&mut built.consumer, &mut leftover)?;
        if built.stream_error.lock().is_some() {
            break;
        }
        if built.overflow.load(Ordering::Relaxed) > 0 {
            truncated = true;
            stop.store(true, Ordering::SeqCst);
            break;
        }
        if flush_pcm_chunk(&mut leftover, &mut resampler, &mut writer, &mut written)? {
            truncated = true;
            stop.store(true, Ordering::SeqCst);
            break;
        }
        thread::sleep(CAPTURE_POLL);
    }
    drop(built.stream);
    thread::sleep(Duration::from_millis(20));
    drain_consumer(&mut built.consumer, &mut leftover)?;
    if built.overflow.load(Ordering::Relaxed) > 0 {
        truncated = true;
    }
    if flush_pcm_chunk(&mut leftover, &mut resampler, &mut writer, &mut written)? {
        truncated = true;
    }
    if !leftover.is_empty() && written < MAX_PCM16_FRAMES {
        let ready = resampler.push(&leftover);
        leftover.clear();
        write_pcm_samples(&mut writer, &ready, &mut written)?;
    }
    if written < MAX_PCM16_FRAMES {
        let tail = resampler.finish();
        write_pcm_samples(&mut writer, &tail, &mut written)?;
        if written >= MAX_PCM16_FRAMES {
            truncated = true;
        }
    }
    writer
        .finalize()
        .map_err(|e| AppError::StorageFailed(e.to_string()))?;
    std::fs::rename(&tmp, &dest).map_err(|e| AppError::StorageFailed(e.to_string()))?;
    let duration_ms = (written * 1000) / u64::from(SAMPLE_RATE);
    Ok(CaptureResult {
        path: dest,
        duration_ms,
        sample_rate: SAMPLE_RATE,
        samples: Vec::new(),
        truncated,
        capture_error: built.stream_error.lock().clone(),
    })
}

pub fn recover_incomplete_capture_wav(
    path: &Path,
) -> Result<Option<RecoveredCaptureWav>, AppError> {
    let mut file = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(path)
        .map_err(|err| AppError::StorageFailed(err.to_string()))?;
    let file_len = file
        .metadata()
        .map_err(|err| AppError::StorageFailed(err.to_string()))?
        .len();
    if file_len < WAV_HEADER_BYTES {
        return Ok(None);
    }
    let mut header = [0u8; WAV_HEADER_LEN];
    file.read_exact(&mut header)
        .map_err(|err| AppError::StorageFailed(err.to_string()))?;
    let is_supported_pcm = &header[0..4] == b"RIFF"
        && &header[8..12] == b"WAVE"
        && &header[12..16] == b"fmt "
        && read_u32_header_field(&header, 16) == Some(16)
        && read_u16_header_field(&header, 20) == Some(1)
        && read_u16_header_field(&header, 22) == Some(1)
        && read_u32_header_field(&header, 24) == Some(SAMPLE_RATE)
        && read_u16_header_field(&header, 34) == Some(16)
        && &header[36..40] == b"data";
    if !is_supported_pcm {
        return Ok(None);
    }

    let actual_data_bytes = file_len.saturating_sub(WAV_HEADER_BYTES);
    let max_data_bytes = MAX_WAV_BYTES.saturating_sub(WAV_HEADER_BYTES) & !1;
    let data_bytes = actual_data_bytes.min(max_data_bytes) & !1;
    file.set_len(WAV_HEADER_BYTES + data_bytes)
        .map_err(|err| AppError::StorageFailed(err.to_string()))?;
    file.seek(SeekFrom::Start(4))
        .and_then(|_| file.write_all(&(36u32 + data_bytes as u32).to_le_bytes()))
        .and_then(|_| file.seek(SeekFrom::Start(40)).map(|_| ()))
        .and_then(|_| file.write_all(&(data_bytes as u32).to_le_bytes()))
        .and_then(|_| file.sync_all())
        .map_err(|err| AppError::StorageFailed(err.to_string()))?;
    let frames = data_bytes / 2;
    Ok(Some(RecoveredCaptureWav {
        duration_ms: frames.saturating_mul(1000) / u64::from(SAMPLE_RATE),
        truncated: actual_data_bytes >= max_data_bytes,
    }))
}

fn read_u16_header_field(header: &[u8; WAV_HEADER_LEN], offset: usize) -> Option<u16> {
    let bytes: [u8; 2] = header
        .get(offset..offset.checked_add(2)?)?
        .try_into()
        .ok()?;
    Some(u16::from_le_bytes(bytes))
}

fn read_u32_header_field(header: &[u8; WAV_HEADER_LEN], offset: usize) -> Option<u32> {
    let bytes: [u8; 4] = header
        .get(offset..offset.checked_add(4)?)?
        .try_into()
        .ok()?;
    Some(u32::from_le_bytes(bytes))
}

pub fn capture_wav_duration_ms(path: &Path) -> Result<u64, AppError> {
    let reader =
        hound::WavReader::open(path).map_err(|err| AppError::StorageFailed(err.to_string()))?;
    let sample_rate = u64::from(reader.spec().sample_rate);
    if sample_rate == 0 {
        return Err(AppError::StorageFailed(
            "capture WAV has zero sample rate".into(),
        ));
    }
    Ok(u64::from(reader.duration()).saturating_mul(1000) / sample_rate)
}

fn flush_pcm_chunk(
    leftover: &mut Vec<f32>,
    resampler: &mut LinearStreamResampler,
    writer: &mut hound::WavWriter<std::io::BufWriter<std::fs::File>>,
    written: &mut u64,
) -> Result<bool, AppError> {
    const MIN_CHUNK: usize = 512;
    if leftover.len() < MIN_CHUNK {
        return Ok(false);
    }
    let take = leftover.len();
    let chunk: Vec<f32> = leftover.drain(..take).collect();
    let resampled = resampler.push(&chunk);
    write_pcm_samples(writer, &resampled, written)?;
    Ok(*written >= MAX_PCM16_FRAMES)
}

fn write_pcm_samples(
    writer: &mut hound::WavWriter<std::io::BufWriter<std::fs::File>>,
    samples: &[f32],
    written: &mut u64,
) -> Result<(), AppError> {
    for sample in f32_to_i16(samples) {
        if *written >= MAX_PCM16_FRAMES {
            return Ok(());
        }
        writer
            .write_sample(sample)
            .map_err(|e| AppError::StorageFailed(e.to_string()))?;
        *written += 1;
    }
    Ok(())
}

fn drain_discard(consumer: &mut rtrb::Consumer<f32>) -> Result<(), AppError> {
    while consumer.pop().is_ok() {}
    Ok(())
}

fn drain_consumer(
    consumer: &mut rtrb::Consumer<f32>,
    pending: &mut Vec<f32>,
) -> Result<(), AppError> {
    while let Ok(sample) = consumer.pop() {
        pending.push(sample);
    }
    Ok(())
}

fn push_frames(
    producer: &mut rtrb::Producer<f32>,
    data: &[f32],
    channels: usize,
    meter: &Arc<Mutex<MeterSample>>,
    overflow: &Arc<AtomicU32>,
    hop: u32,
) {
    let mono = to_mono(data, channels);
    let mut peak = 0.0f32;
    let mut sum = 0.0f32;
    for &s in &mono {
        peak = peak.max(s.abs());
        sum += s * s;
        if producer.push(s).is_err() {
            overflow.fetch_add(1, Ordering::Relaxed);
        }
    }
    let rms = if mono.is_empty() {
        0.0
    } else {
        (sum / mono.len() as f32).sqrt()
    };
    let mut sample = meter.lock();
    sample.rms = rms;
    sample.peak = peak;
    push_meter(&mut sample, peak.max(rms * 1.6), mono.len() as u32, hop);
}

fn push_meter(sample: &mut MeterSample, amplitude: f32, frames: u32, hop: u32) {
    if sample.levels.len() != METER_BINS {
        sample.levels = vec![0.0; METER_BINS];
    }
    let hop = hop.max(1);
    let amplitude = amplitude.clamp(0.0, 1.0);
    sample.pending_peak = sample.pending_peak.max(amplitude);
    sample.pending_n = sample.pending_n.saturating_add(frames);
    while sample.pending_n >= hop {
        sample.pending_n -= hop;
        sample.levels.remove(0);
        sample.levels.push(sample.pending_peak);
        sample.pending_peak = 0.0;
    }
}

pub fn write_pcm16_wav(path: &Path, sample_rate: u32, samples: &[f32]) -> Result<(), AppError> {
    let spec = WavSpec {
        channels: 1,
        sample_rate,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let tmp = path.with_extension("wav.tmp");
    {
        let mut writer =
            WavWriter::create(&tmp, spec).map_err(|e| AppError::StorageFailed(e.to_string()))?;
        for sample in f32_to_i16(samples) {
            writer
                .write_sample(sample)
                .map_err(|e| AppError::StorageFailed(e.to_string()))?;
        }
        writer
            .finalize()
            .map_err(|e| AppError::StorageFailed(e.to_string()))?;
    }
    std::fs::rename(&tmp, path).map_err(|e| AppError::StorageFailed(e.to_string()))?;
    Ok(())
}

pub fn read_pcm16_wav(path: &Path) -> Result<Vec<f32>, AppError> {
    Ok(read_pcm16_wav_with_rate(path)?.0)
}

pub fn read_pcm16_wav_with_rate(path: &Path) -> Result<(Vec<f32>, u32), AppError> {
    let mut reader =
        hound::WavReader::open(path).map_err(|e| AppError::StorageFailed(e.to_string()))?;
    let rate = reader.spec().sample_rate;
    let samples: Result<Vec<i16>, _> = reader.samples::<i16>().collect();
    let samples = samples.map_err(|e| AppError::StorageFailed(e.to_string()))?;
    Ok((i16_to_f32(&samples), rate))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn wav_roundtrip() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("t.wav");
        let data = vec![0.0, 0.5, -0.5, 0.25];
        write_pcm16_wav(&path, 48_000, &data).unwrap();
        let decoded = read_pcm16_wav(&path).unwrap();
        assert_eq!(decoded.len(), data.len());
        let (_, rate) = read_pcm16_wav_with_rate(&path).unwrap();
        assert_eq!(rate, 48_000);
        for (a, b) in decoded.iter().zip(data.iter()) {
            assert!((a - b).abs() < 0.01);
        }
    }

    #[test]
    fn repairs_unfinalized_capture_header_and_reports_duration() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("partial.wav.tmp");
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
        std::fs::write(&path, bytes).unwrap();

        let recovered = recover_incomplete_capture_wav(&path).unwrap().unwrap();

        assert_eq!(recovered.duration_ms, 100);
        assert!(!recovered.truncated);
        assert_eq!(capture_wav_duration_ms(&path).unwrap(), 100);
    }

    #[test]
    fn recovery_marks_capture_truncated_when_file_ends_exactly_at_limit() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("at-limit.wav.tmp");
        let mut file = std::fs::File::create(&path).unwrap();
        file.write_all(b"RIFF\0\0\0\0WAVEfmt ").unwrap();
        file.write_all(&16u32.to_le_bytes()).unwrap();
        file.write_all(&1u16.to_le_bytes()).unwrap();
        file.write_all(&1u16.to_le_bytes()).unwrap();
        file.write_all(&SAMPLE_RATE.to_le_bytes()).unwrap();
        file.write_all(&(SAMPLE_RATE * 2).to_le_bytes()).unwrap();
        file.write_all(&2u16.to_le_bytes()).unwrap();
        file.write_all(&16u16.to_le_bytes()).unwrap();
        file.write_all(b"data\0\0\0\0").unwrap();
        file.set_len(MAX_WAV_BYTES).unwrap();
        drop(file);

        let recovered = recover_incomplete_capture_wav(&path).unwrap().unwrap();

        assert_eq!(std::fs::metadata(&path).unwrap().len(), MAX_WAV_BYTES);
        assert_eq!(recovered.duration_ms, max_duration_ms());
        assert!(recovered.truncated);
    }

    #[test]
    fn drain_empty_consumer_is_instant() {
        let (_producer, mut consumer) = RingBuffer::<f32>::new(16);
        let mut pending = Vec::new();
        drain_consumer(&mut consumer, &mut pending).unwrap();
        assert!(pending.is_empty());
    }

    #[test]
    fn monitor_drain_does_not_keep_pcm() {
        let (mut producer, mut consumer) = RingBuffer::<f32>::new(16);
        assert!(producer.push(0.25).is_ok());
        drain_discard(&mut consumer).unwrap();
        assert!(consumer.pop().is_err());
        assert_eq!(CAPTURE_POLL, Duration::from_millis(16));
    }

    #[test]
    fn meter_scrolls_latest_amplitude_to_the_right() {
        let mut sample = MeterSample::silent();
        push_meter(&mut sample, 0.2, 1, 1);
        push_meter(&mut sample, 0.9, 1, 1);
        assert!((sample.levels[METER_BINS - 1] - 0.9).abs() < f32::EPSILON);
        assert!((sample.levels[METER_BINS - 2] - 0.2).abs() < f32::EPSILON);
    }

    #[test]
    fn meter_does_not_advance_a_bin_per_callback() {
        let mut sample = MeterSample::silent();
        push_meter(&mut sample, 0.4, 200, 960);
        assert_eq!(sample.levels[METER_BINS - 1], 0.0);
        push_meter(&mut sample, 0.8, 800, 960);
        assert!((sample.levels[METER_BINS - 1] - 0.8).abs() < f32::EPSILON);
        assert_eq!(sample.levels[METER_BINS - 2], 0.0);
    }

    #[test]
    fn meter_hop_is_about_sixty_milliseconds() {
        assert_eq!(meter_hop(48_000), 3_000);
    }

    #[test]
    fn duration_cap_matches_across_input_rates() {
        let cap = max_duration_ms();
        let frames_44 = output_frames_for_input(44_100 * cap / 1000, 44_100);
        let frames_48 = output_frames_for_input(48_000 * cap / 1000, 48_000);
        let frames_96 = output_frames_for_input(96_000 * cap / 1000, 96_000);
        assert!((frames_44 as i64 - frames_48 as i64).abs() <= 1);
        assert!((frames_96 as i64 - frames_48 as i64).abs() <= 1);
        assert_eq!(MAX_PCM16_FRAMES * 2 + 44, MAX_WAV_BYTES);
    }

    #[test]
    fn hung_open_timeout_is_bounded() {
        assert_eq!(START_TIMEOUT, Duration::from_secs(8));
        assert!(HUNG_JOIN_GRACE < Duration::from_millis(100));
        assert_eq!(JOIN_BOUND, Duration::from_secs(8));
    }

    #[test]
    fn startup_timeout_releases_owner_only_after_native_worker_cleanup() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("late-start.wav");
        std::fs::write(&path, b"partial").unwrap();
        let cleanup = CaptureCleanupHandle::new(Some(path.clone()));
        cleanup.discard_late_result();

        let (release_tx, release_rx) = std::sync::mpsc::channel();
        let (completed_tx, completed_rx) = std::sync::mpsc::channel();
        let worker = thread::spawn(move || {
            release_rx.recv().unwrap();
            Ok(CaptureResult {
                path: PathBuf::new(),
                duration_ms: 0,
                sample_rate: SAMPLE_RATE,
                samples: Vec::new(),
                truncated: false,
                capture_error: None,
            })
        });
        let callback_path = path.clone();
        let callback_cleanup = cleanup.clone();
        defer_start_timeout_cleanup(
            worker,
            cleanup.clone(),
            Some(Box::new(move || {
                completed_tx
                    .send((
                        callback_path.exists(),
                        callback_cleanup.0.finished.load(Ordering::SeqCst),
                    ))
                    .unwrap();
            })),
        );

        assert!(completed_rx
            .recv_timeout(Duration::from_millis(20))
            .is_err());
        assert!(path.exists());
        release_tx.send(()).unwrap();
        assert_eq!(
            completed_rx.recv_timeout(Duration::from_secs(2)).unwrap(),
            (false, true)
        );
    }

    #[test]
    fn stop_timeout_does_not_kill_worker_and_cancel_cleans_late_wav_after_join() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("late.wav");
        let tmp_path = path.with_extension("wav.tmp");
        let (release_tx, release_rx) = std::sync::mpsc::channel();
        let writer_path = path.clone();
        let capture_thread = thread::spawn(move || {
            release_rx.recv().unwrap();
            std::fs::write(&writer_path, b"late wav").unwrap();
            Ok(CaptureResult {
                path: writer_path,
                duration_ms: 1,
                sample_rate: SAMPLE_RATE,
                samples: Vec::new(),
                truncated: false,
                capture_error: None,
            })
        });
        let cleanup = CaptureCleanupHandle::new(Some(path.clone()));
        let session = CaptureSession {
            stop: Arc::new(AtomicBool::new(false)),
            meter: Arc::new(Mutex::new(MeterSample::silent())),
            thread: Some(capture_thread),
            fallback: Arc::new(AtomicBool::new(false)),
            cleanup: cleanup.clone(),
        };

        let started = std::time::Instant::now();
        let err = match session.stop_and_discard_with_timeout(Duration::from_millis(20)) {
            Err(err) => err,
            Ok(_) => panic!("expected capture stop timeout"),
        };
        assert!(started.elapsed() < Duration::from_secs(1));
        assert!(err.to_string().contains("remains detached"));
        assert!(!cleanup.0.finished.load(Ordering::SeqCst));

        // A canceled take marks the late result for deletion. Keep a tmp artifact too,
        // matching the file that may exist while the capture writer is still active.
        std::fs::write(&tmp_path, b"partial wav").unwrap();
        release_tx.send(()).unwrap();

        let deadline = std::time::Instant::now() + Duration::from_secs(2);
        while (!cleanup.0.finished.load(Ordering::SeqCst) || path.exists() || tmp_path.exists())
            && std::time::Instant::now() < deadline
        {
            thread::sleep(Duration::from_millis(5));
        }
        assert!(cleanup.0.finished.load(Ordering::SeqCst));
        assert!(!path.exists());
        assert!(!tmp_path.exists());
    }

    #[test]
    fn stop_timeout_delivers_late_capture_result_to_owner() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("late-result.wav");
        let (release_tx, release_rx) = std::sync::mpsc::channel();
        let writer_path = path.clone();
        let capture_thread = thread::spawn(move || {
            release_rx.recv().unwrap();
            std::fs::write(&writer_path, b"late wav").unwrap();
            Ok(CaptureResult {
                path: writer_path,
                duration_ms: 125,
                sample_rate: SAMPLE_RATE,
                samples: vec![0.1; 8],
                truncated: false,
                capture_error: None,
            })
        });
        let cleanup = CaptureCleanupHandle::new(Some(path.clone()));
        let session = CaptureSession {
            stop: Arc::new(AtomicBool::new(false)),
            meter: Arc::new(Mutex::new(MeterSample::silent())),
            thread: Some(capture_thread),
            fallback: Arc::new(AtomicBool::new(false)),
            cleanup: cleanup.clone(),
        };
        let (late_tx, late_rx) = std::sync::mpsc::channel();

        let outcome = session
            .stop_with_timeout_and_late_result(Duration::from_millis(20), move |result| {
                late_tx.send(result).unwrap()
            });
        assert!(matches!(outcome, CaptureStopOutcome::TimedOut(_)));
        assert!(!cleanup.0.finished.load(Ordering::SeqCst));

        release_tx.send(()).unwrap();
        let result = late_rx
            .recv_timeout(Duration::from_secs(2))
            .expect("late capture result was not delivered")
            .unwrap();
        assert_eq!(result.duration_ms, 125);
        assert_eq!(result.path, path);
        assert!(cleanup.0.finished.load(Ordering::SeqCst));
        assert!(result.path.is_file());
    }

    #[test]
    fn validates_capture_errors_and_size_truncation_before_sample_use() {
        let base = CaptureResult {
            path: PathBuf::from("sample.wav"),
            duration_ms: 100,
            sample_rate: SAMPLE_RATE,
            samples: vec![0.0; 8],
            truncated: false,
            capture_error: None,
        };
        assert!(validate_capture_result(&base).is_ok());

        let stream_error = CaptureResult {
            path: base.path.clone(),
            duration_ms: base.duration_ms,
            sample_rate: base.sample_rate,
            samples: base.samples.clone(),
            truncated: base.truncated,
            capture_error: Some("input device disconnected".into()),
        };
        assert_eq!(
            validate_capture_result(&stream_error),
            Err(AppError::AudioCaptureFailed(
                "input device disconnected".into()
            ))
        );

        let truncated = CaptureResult {
            truncated: true,
            ..base
        };
        assert_eq!(
            validate_capture_result(&truncated),
            Err(AppError::RecordingTruncated)
        );
    }

    #[test]
    fn finished_join_handle_is_detected() {
        let thread = thread::spawn(|| {
            Ok(CaptureResult {
                path: PathBuf::new(),
                duration_ms: 0,
                sample_rate: SAMPLE_RATE,
                samples: Vec::new(),
                truncated: true,
                capture_error: None,
            })
        });
        while !thread.is_finished() {
            thread::sleep(Duration::from_millis(1));
        }
        let session = CaptureSession {
            stop: Arc::new(AtomicBool::new(false)),
            meter: Arc::new(Mutex::new(MeterSample::silent())),
            thread: Some(thread),
            fallback: Arc::new(AtomicBool::new(false)),
            cleanup: CaptureCleanupHandle::new(None),
        };
        assert!(session.is_finished());
        let result = session.stop().unwrap();
        assert!(result.truncated);
    }

    #[test]
    fn recovery_rejects_unsupported_headers_without_modifying_file() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("recover.wav");
        write_pcm16_wav(&path, SAMPLE_RATE, &[0.25; 48]).unwrap();
        let original = std::fs::read(&path).unwrap();
        for offset in [0, 8, 12, 16, 20, 22, 24, 34, 36] {
            let mut invalid = original.clone();
            invalid[offset] ^= 0xff;
            std::fs::write(&path, &invalid).unwrap();
            assert_eq!(recover_incomplete_capture_wav(&path).unwrap(), None);
            assert_eq!(std::fs::read(&path).unwrap(), invalid);
        }
        std::fs::write(&path, b"RIFF").unwrap();
        assert_eq!(recover_incomplete_capture_wav(&path).unwrap(), None);
        assert_eq!(std::fs::read(&path).unwrap(), b"RIFF");
        assert!(matches!(
            recover_incomplete_capture_wav(&dir.path().join("missing.wav")),
            Err(AppError::StorageFailed(_))
        ));
    }

    #[test]
    fn recovery_drops_only_incomplete_last_sample_and_repairs_lengths() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("odd-tail.wav");
        write_pcm16_wav(&path, SAMPLE_RATE, &[0.25; 48]).unwrap();
        let mut bytes = std::fs::read(&path).unwrap();
        bytes[4..8].fill(0);
        bytes[40..44].fill(0);
        bytes.push(0xff);
        std::fs::write(&path, bytes).unwrap();
        let result = recover_incomplete_capture_wav(&path).unwrap().unwrap();
        assert_eq!(result.duration_ms, 1);
        assert!(!result.truncated);
        assert_eq!(std::fs::metadata(&path).unwrap().len(), 44 + 48 * 2);
        let (samples, rate) = read_pcm16_wav_with_rate(&path).unwrap();
        assert_eq!(samples.len(), 48);
        assert_eq!(rate, SAMPLE_RATE);
        assert!(samples.iter().all(|sample| (*sample - 0.25).abs() < 0.001));
    }

    #[test]
    fn ring_overflow_is_counted_and_meter_does_not_prevent_pcm_drain() {
        let (mut producer, mut consumer) = RingBuffer::<f32>::new(4);
        let meter = Arc::new(Mutex::new(MeterSample::silent()));
        let overflow = Arc::new(AtomicU32::new(0));
        push_frames(&mut producer, &[1.0; 12], 2, &meter, &overflow, 6);
        assert_eq!(overflow.load(Ordering::Relaxed), 2);
        assert_eq!(meter.lock().rms, 1.0);
        assert_eq!(meter.lock().peak, 1.0);
        assert_eq!(meter.lock().levels[METER_BINS - 1], 1.0);
        let mut pending = vec![0.5];
        drain_consumer(&mut consumer, &mut pending).unwrap();
        assert_eq!(pending, [0.5, 1.0, 1.0, 1.0, 1.0]);
        assert!(consumer.pop().is_err());
    }

    #[test]
    fn pcm_flush_preserves_short_chunk_and_stops_at_exact_frame_limit() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("cap.wav");
        let spec = WavSpec {
            channels: 1,
            sample_rate: SAMPLE_RATE,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        };
        let mut writer = WavWriter::create(&path, spec).unwrap();
        let mut resampler = LinearStreamResampler::new(SAMPLE_RATE, SAMPLE_RATE);
        let mut pending = vec![0.5; 511];
        let mut written = MAX_PCM16_FRAMES - 1;
        assert!(!flush_pcm_chunk(&mut pending, &mut resampler, &mut writer, &mut written).unwrap());
        assert_eq!(pending.len(), 511);
        assert_eq!(written, MAX_PCM16_FRAMES - 1);
        pending.push(0.5);
        assert!(flush_pcm_chunk(&mut pending, &mut resampler, &mut writer, &mut written).unwrap());
        assert!(pending.is_empty());
        assert_eq!(written, MAX_PCM16_FRAMES);
        writer.finalize().unwrap();
        let samples = read_pcm16_wav(&path).unwrap();
        assert_eq!(samples.len(), 1);
        assert!((samples[0] - 0.5).abs() < 0.001);
    }

    #[test]
    fn wav_io_reports_missing_paths_and_clips_out_of_range_pcm() {
        let dir = tempdir().unwrap();
        let missing = dir.path().join("missing").join("sample.wav");
        assert!(matches!(
            read_pcm16_wav(&missing),
            Err(AppError::StorageFailed(_))
        ));
        assert!(matches!(
            capture_wav_duration_ms(&missing),
            Err(AppError::StorageFailed(_))
        ));
        assert!(matches!(
            write_pcm16_wav(&missing, SAMPLE_RATE, &[0.0]),
            Err(AppError::StorageFailed(_))
        ));
        let path = dir.path().join("clipped.wav");
        write_pcm16_wav(&path, SAMPLE_RATE, &[-2.0, -0.5, 0.5, 2.0]).unwrap();
        assert!(!path.with_extension("wav.tmp").exists());
        let (samples, rate) = read_pcm16_wav_with_rate(&path).unwrap();
        assert_eq!(rate, SAMPLE_RATE);
        for (actual, expected) in samples.iter().zip([-1.0, -0.5, 0.5, 1.0]) {
            assert!((*actual - expected).abs() < 0.001);
        }
    }
}
