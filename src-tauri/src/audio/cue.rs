use crate::error::AppError;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CueKind {
    Start,
    Stop,
}

const CUE_PEAK: f32 = 0.08;
const ATTACK_S: f32 = 0.02;
const DECAY_S: f32 = 0.12;
const CUE_DURATION_S: f32 = ATTACK_S + DECAY_S;
const ENV_FLOOR: f32 = 1e-4;
const FRONT_CHANNELS: usize = 2;

pub fn cue_envelope(t: f32) -> f32 {
    if t <= 0.0 {
        return 0.0;
    }
    if t < ATTACK_S {
        let p = t / ATTACK_S;
        ENV_FLOOR * (CUE_PEAK / ENV_FLOOR).powf(p)
    } else if t < CUE_DURATION_S {
        let p = ((t - ATTACK_S) / DECAY_S).clamp(0.0, 1.0);
        CUE_PEAK * (ENV_FLOOR / CUE_PEAK).powf(p)
    } else {
        0.0
    }
}

pub fn cue_samples(kind: CueKind, sample_rate: u32) -> Vec<f32> {
    let freq = match kind {
        CueKind::Start => 880.0,
        CueKind::Stop => 392.0,
    };
    let n = ((f64::from(CUE_DURATION_S) * f64::from(sample_rate)) as usize).max(1);
    (0..n)
        .map(|i| {
            let t = i as f32 / sample_rate as f32;
            (t * freq * 2.0 * std::f32::consts::PI).sin() * cue_envelope(t)
        })
        .collect()
}

pub fn write_cue_channels(frame: &mut [f32], value: f32) {
    for (ch, sample) in frame.iter_mut().enumerate() {
        *sample = if ch < FRONT_CHANNELS { value } else { 0.0 };
    }
}

pub fn cue_peak_db(samples: &[f32]) -> f32 {
    let peak = samples.iter().fold(0.0f32, |acc, s| acc.max(s.abs()));
    if peak <= 1e-9 {
        f32::NEG_INFINITY
    } else {
        20.0 * peak.log10()
    }
}

pub fn play_dictation_cue(kind: CueKind) {
    std::thread::spawn(move || {
        if let Err(err) = play_blocking(kind) {
            tracing::warn!(error = %err, "dictation cue failed");
        }
    });
}

fn play_blocking(kind: CueKind) -> Result<(), AppError> {
    use cpal::traits::{DeviceTrait, HostTrait};
    let host = cpal::default_host();
    let device = host
        .default_output_device()
        .ok_or_else(|| AppError::AudioCaptureFailed("no output device".into()))?;
    let config = device
        .default_output_config()
        .map_err(|e| AppError::AudioCaptureFailed(e.to_string()))?;
    let sample_rate = config.sample_rate().0;
    let channels = config.channels() as usize;
    let samples = cue_samples(kind, sample_rate);
    let drain_ms = ((samples.len() as f64 / f64::from(sample_rate)) * 1000.0) as u64 + 80;
    let err_fn = |err| tracing::warn!(error = %err, "cue stream");
    match config.sample_format() {
        cpal::SampleFormat::F32 => {
            play_f32(&device, config.into(), channels, samples, err_fn, drain_ms)
        }
        cpal::SampleFormat::I16 => {
            play_i16(&device, config.into(), channels, samples, err_fn, drain_ms)
        }
        _ => Err(AppError::AudioCaptureFailed(
            "unsupported output format".into(),
        )),
    }
}

fn play_f32(
    device: &cpal::Device,
    config: cpal::StreamConfig,
    channels: usize,
    samples: Vec<f32>,
    err_fn: impl FnMut(cpal::StreamError) + Send + 'static,
    drain_ms: u64,
) -> Result<(), AppError> {
    use cpal::traits::{DeviceTrait, StreamTrait};
    let mut index = 0usize;
    let stream = device
        .build_output_stream(
            &config,
            move |data: &mut [f32], _| {
                for frame in data.chunks_mut(channels) {
                    let value = samples.get(index).copied().unwrap_or(0.0);
                    index += 1;
                    write_cue_channels(frame, value);
                }
            },
            err_fn,
            None,
        )
        .map_err(|e| AppError::AudioCaptureFailed(e.to_string()))?;
    stream
        .play()
        .map_err(|e| AppError::AudioCaptureFailed(e.to_string()))?;
    std::thread::sleep(std::time::Duration::from_millis(drain_ms));
    Ok(())
}

fn play_i16(
    device: &cpal::Device,
    config: cpal::StreamConfig,
    channels: usize,
    samples: Vec<f32>,
    err_fn: impl FnMut(cpal::StreamError) + Send + 'static,
    drain_ms: u64,
) -> Result<(), AppError> {
    use cpal::traits::{DeviceTrait, StreamTrait};
    let mut index = 0usize;
    let stream = device
        .build_output_stream(
            &config,
            move |data: &mut [i16], _| {
                for frame in data.chunks_mut(channels) {
                    let value = samples.get(index).copied().unwrap_or(0.0);
                    index += 1;
                    let scaled = (value * f32::from(i16::MAX)).round() as i16;
                    for (ch, sample) in frame.iter_mut().enumerate() {
                        *sample = if ch < FRONT_CHANNELS { scaled } else { 0 };
                    }
                }
            },
            err_fn,
            None,
        )
        .map_err(|e| AppError::AudioCaptureFailed(e.to_string()))?;
    stream
        .play()
        .map_err(|e| AppError::AudioCaptureFailed(e.to_string()))?;
    std::thread::sleep(std::time::Duration::from_millis(drain_ms));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use approx::assert_relative_eq;

    #[test]
    fn cue_peak_matches_former_webaudio_ping() {
        let samples = cue_samples(CueKind::Start, 48_000);
        let db = cue_peak_db(&samples);
        assert!(
            db > -24.0 && db < -20.0,
            "peak {db} should stay near -22 dBFS"
        );
        assert!(samples.iter().all(|s| s.abs() <= CUE_PEAK + 1e-5));
        assert_relative_eq!(cue_envelope(0.02), CUE_PEAK, epsilon = 1e-4);
        assert!(cue_envelope(0.14) < 2e-4);
    }

    #[test]
    fn start_and_stop_use_legacy_pitches() {
        let start = cue_samples(CueKind::Start, 48_000);
        let stop = cue_samples(CueKind::Stop, 48_000);
        assert_ne!(&start, &stop);
        assert!(start.len() >= 48_000 / 10);
    }

    #[test]
    fn surround_channels_stay_silent() {
        let mut frame = [1.0f32; 6];
        write_cue_channels(&mut frame, 0.08);
        assert_relative_eq!(frame[0], 0.08);
        assert_relative_eq!(frame[1], 0.08);
        assert_eq!(frame[2], 0.0);
        assert_eq!(frame[5], 0.0);
    }
}
