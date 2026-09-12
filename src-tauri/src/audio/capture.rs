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
}

pub struct CaptureResult {
    pub path: PathBuf,
    pub duration_ms: u64,
    pub sample_rate: u32,
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
        let stop_t = stop.clone();
        let meter_t = meter.clone();
        let (ready_tx, ready_rx) = std::sync::mpsc::channel();
        let thread = thread::spawn(move || {
            start_stream_and_write(device_name.as_deref(), dest, stop_t, meter_t, ready_tx)
        });
        match ready_rx.recv_timeout(Duration::from_secs(8)) {
            Ok(Ok(())) => Ok(Self {
                stop,
                meter,
                thread: Some(thread),
            }),
            Ok(Err(err)) => {
                let _ = thread.join();
                Err(err)
            }
            Err(_) => {
                stop.store(true, Ordering::SeqCst);
                let _ = thread.join();
                Err(AppError::AudioCaptureFailed(
                    "microphone did not start in time".into(),
                ))
            }
        }
    }

    pub fn meter(&self) -> MeterSample {
        self.meter.lock().clone()
    }

    pub fn stop(mut self) -> Result<CaptureResult, AppError> {
        self.stop.store(true, Ordering::SeqCst);
        self.thread
            .take()
            .ok_or_else(|| AppError::AudioCaptureFailed("capture thread missing".into()))?
            .join()
            .map_err(|_| AppError::AudioCaptureFailed("capture thread panicked".into()))?
    }

    pub fn discard(self) {
        let _ = self.stop();
    }
}

fn start_stream_and_write(
    device_name: Option<&str>,
    dest: Option<PathBuf>,
    stop: Arc<AtomicBool>,
    meter: Arc<Mutex<MeterSample>>,
    ready_tx: std::sync::mpsc::Sender<Result<(), AppError>>,
) -> Result<CaptureResult, AppError> {
    match start_stream_inner(device_name, meter) {
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
) -> Result<BuiltCapture, AppError> {
    let device = resolve_device(device_name).map_err(|_| AppError::MicrophoneUnavailable)?;
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
        let mut dump = Vec::new();
        drain_consumer(&mut built.consumer, &mut dump)?;
        thread::sleep(Duration::from_millis(5));
    }
    drop(built.stream);
    Ok(CaptureResult {
        path: PathBuf::new(),
        duration_ms: 0,
        sample_rate: SAMPLE_RATE,
    })
}

fn run_capture(
    mut built: BuiltCapture,
    dest: PathBuf,
    stop: Arc<AtomicBool>,
) -> Result<CaptureResult, AppError> {
    let mut pending = Vec::new();
    while !stop.load(Ordering::SeqCst) {
        drain_consumer(&mut built.consumer, &mut pending)?;
        thread::sleep(Duration::from_millis(5));
    }
    drop(built.stream);
    thread::sleep(Duration::from_millis(20));
    drain_consumer(&mut built.consumer, &mut pending)?;
    if built.overflow.load(Ordering::Relaxed) > 0 {
        tracing::warn!(
            drops = built.overflow.load(Ordering::Relaxed),
            "capture overflow"
        );
    }
    let resampled = resample_to_48k(&pending, built.input_rate)?;
    write_pcm16_wav(&dest, SAMPLE_RATE, &resampled)?;
    let duration_ms = (resampled.len() as u64 * 1000) / SAMPLE_RATE as u64;
    Ok(CaptureResult {
        path: dest,
        duration_ms,
        sample_rate: SAMPLE_RATE,
    })
}

fn drain_consumer(
    consumer: &mut rtrb::Consumer<f32>,
    pending: &mut Vec<f32>,
) -> Result<(), AppError> {
    while let Ok(sample) = consumer.pop() {
        pending.push(sample);
        if pending.len() as u64 * 2 + 44 > MAX_WAV_BYTES {
            return Err(AppError::RecordingTooLarge);
        }
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
}
