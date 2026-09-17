use serde::{Deserialize, Serialize};
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
use super::resample::{f32_to_i16, i16_to_f32, resample_to_48k, to_mono};
use crate::dsp::metrics::SAMPLE_RATE;
use crate::error::AppError;

const QUEUE_CAP: usize = 48_000 * 2;
pub const MAX_WAV_BYTES: u64 = 20 * 1024 * 1024;
pub const MAX_PCM16_FRAMES: u64 = (MAX_WAV_BYTES - 44) / 2;
const CAPTURE_POLL: Duration = Duration::from_millis(16);
const START_TIMEOUT: Duration = Duration::from_secs(8);
const HUNG_JOIN_GRACE: Duration = Duration::from_millis(50);
const JOIN_BOUND: Duration = Duration::from_secs(8);

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
}

pub struct CaptureResult {
    pub path: PathBuf,
    pub duration_ms: u64,
    pub sample_rate: u32,
    pub samples: Vec<f32>,
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
        Self::spawn(device_name, Some(dest))
    }

    pub fn start_monitor(device_name: Option<&str>) -> Result<Self, AppError> {
        Self::spawn(device_name, None)
    }

    fn spawn(device_name: Option<&str>, dest: Option<PathBuf>) -> Result<Self, AppError> {
        let device_name = device_name.map(str::to_string);
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
            }),
            Ok(Err(err)) => {
                let _ = thread.join();
                Err(err)
            }
            Err(_) => {
                stop.store(true, Ordering::SeqCst);
                thread::spawn(move || {
                    let _ = thread.join();
                });
                thread::sleep(HUNG_JOIN_GRACE);
                Err(AppError::AudioCaptureFailed(
                    "microphone did not start in time".into(),
                ))
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

    pub fn stop(mut self) -> Result<CaptureResult, AppError> {
        self.stop.store(true, Ordering::SeqCst);
        let handle = self
            .thread
            .take()
            .ok_or_else(|| AppError::AudioCaptureFailed("capture thread missing".into()))?;
        if handle.is_finished() {
            return handle
                .join()
                .map_err(|_| AppError::AudioCaptureFailed("capture thread panicked".into()))?;
        }
        let (tx, rx) = std::sync::mpsc::channel();
        thread::spawn(move || {
            let _ = tx.send(handle.join());
        });
        match rx.recv_timeout(JOIN_BOUND) {
            Ok(Ok(result)) => result,
            Ok(Err(_)) => Err(AppError::AudioCaptureFailed(
                "capture thread panicked".into(),
            )),
            Err(_) => Err(AppError::AudioCaptureFailed(
                "capture thread did not stop in time".into(),
            )),
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
            thread::spawn(move || {
                let _ = thread.join();
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

    let err_fn = |err| {
        tracing::error!(error = %err, "cpal stream error");
    };

    let stream = match sample_format {
        SampleFormat::F32 => device.build_input_stream(
            &config,
            move |data: &[f32], _| {
                push_frames(&mut producer, data, channels, &meter_cb, &overflow_cb, hop)
            },
            err_fn,
            None,
        ),
        SampleFormat::I16 => device.build_input_stream(
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
            err_fn,
            None,
        ),
        SampleFormat::U16 => device.build_input_stream(
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
            err_fn,
            None,
        ),
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
    })
}

fn run_monitor(mut built: BuiltCapture, stop: Arc<AtomicBool>) -> Result<CaptureResult, AppError> {
    while !stop.load(Ordering::SeqCst) {
        drain_discard(&mut built.consumer)?;
        thread::sleep(CAPTURE_POLL);
    }
    drop(built.stream);
    Ok(CaptureResult {
        path: PathBuf::new(),
        duration_ms: 0,
        sample_rate: SAMPLE_RATE,
        samples: Vec::new(),
        truncated: false,
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
    let mut written = 0u64;
    let mut truncated = false;
    while !stop.load(Ordering::SeqCst) {
        drain_consumer(&mut built.consumer, &mut leftover)?;
        if built.overflow.load(Ordering::Relaxed) > 0 {
            truncated = true;
            stop.store(true, Ordering::SeqCst);
            break;
        }
        if flush_pcm_chunk(&mut leftover, built.input_rate, &mut writer, &mut written)? {
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
    let _ = flush_pcm_chunk(&mut leftover, built.input_rate, &mut writer, &mut written);
    if !leftover.is_empty() && written < MAX_PCM16_FRAMES {
        let tail = resample_to_48k(&leftover, built.input_rate)?;
        leftover.clear();
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
    })
}

fn flush_pcm_chunk(
    leftover: &mut Vec<f32>,
    input_rate: u32,
    writer: &mut hound::WavWriter<std::io::BufWriter<std::fs::File>>,
    written: &mut u64,
) -> Result<bool, AppError> {
    const MIN_CHUNK: usize = 512;
    if leftover.len() < MIN_CHUNK {
        return Ok(false);
    }
    let take = leftover.len();
    let chunk: Vec<f32> = leftover.drain(..take).collect();
    let resampled = resample_to_48k(&chunk, input_rate)?;
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
    fn finished_join_handle_is_detected() {
        let thread = thread::spawn(|| {
            Ok(CaptureResult {
                path: PathBuf::new(),
                duration_ms: 0,
                sample_rate: SAMPLE_RATE,
                samples: Vec::new(),
                truncated: true,
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
        };
        assert!(session.is_finished());
        let result = session.stop().unwrap();
        assert!(result.truncated);
    }
}
