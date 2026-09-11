use crate::error::AppError;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CueKind {
    Start,
    Stop,
    Cancel,
}

const CUE_PEAK: f32 = 0.32;
const ATTACK_S: f32 = 0.02;
const DECAY_S: f32 = 0.12;
const ENV_FLOOR: f32 = 1e-4;
const FRONT_CHANNELS: usize = 2;

const CANCEL_ATTACK_S: f32 = 0.012;
const CANCEL_DECAY_S: f32 = 0.038;
const CANCEL_GAP_S: f32 = 0.04;

pub fn cue_envelope(t: f32) -> f32 {
    envelope_at(t, ATTACK_S, DECAY_S, CUE_PEAK)
}

fn envelope_at(t: f32, attack: f32, decay: f32, peak: f32) -> f32 {
    let duration = attack + decay;
    if t <= 0.0 {
        return 0.0;
    }
    if t < attack {
        let p = t / attack;
        ENV_FLOOR * (peak / ENV_FLOOR).powf(p)
    } else if t < duration {
        let p = ((t - attack) / decay).clamp(0.0, 1.0);
        peak * (ENV_FLOOR / peak).powf(p)
    } else {
        0.0
    }
}

pub fn cue_hz(kind: CueKind) -> f32 {
    match kind {
        CueKind::Start | CueKind::Cancel => 880.0,
        CueKind::Stop => 698.46,
    }
}

fn ping_tone(freq: f32, sample_rate: u32, attack: f32, decay: f32) -> Vec<f32> {
    let duration = attack + decay;
    let phase = std::f32::consts::FRAC_PI_2 - freq * attack * 2.0 * std::f32::consts::PI;
    let n = ((f64::from(duration) * f64::from(sample_rate)) as usize).max(1);
    (0..n)
        .map(|i| {
            let t = i as f32 / sample_rate as f32;
            (t * freq * 2.0 * std::f32::consts::PI + phase).sin()
                * envelope_at(t, attack, decay, CUE_PEAK)
        })
        .collect()
}

fn normalize_peak(samples: &mut [f32]) {
    let peak = samples
        .iter()
        .fold(0.0f32, |acc, sample| acc.max(sample.abs()));
    if peak > 1e-9 {
        let gain = CUE_PEAK / peak;
        for sample in samples {
            *sample *= gain;
        }
    }
}

fn rms(samples: &[f32]) -> f32 {
    if samples.is_empty() {
        return 0.0;
    }
    let sum: f32 = samples.iter().map(|sample| sample * sample).sum();
    (sum / samples.len() as f32).sqrt()
}

pub fn cue_samples(kind: CueKind, sample_rate: u32) -> Vec<f32> {
    let mut samples = match kind {
        CueKind::Start => ping_tone(cue_hz(kind), sample_rate, ATTACK_S, DECAY_S),
        CueKind::Stop => ping_tone(cue_hz(kind), sample_rate, ATTACK_S, DECAY_S),
        CueKind::Cancel => {
            let blip = ping_tone(cue_hz(kind), sample_rate, CANCEL_ATTACK_S, CANCEL_DECAY_S);
            let gap = ((CANCEL_GAP_S * sample_rate as f32) as usize).max(1);
            let mut out = blip.clone();
            out.extend(std::iter::repeat(0.0).take(gap));
            out.extend(blip);
            out
        }
    };
    normalize_peak(&mut samples);
    samples
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
    fn cue_peak_is_audible_without_clipping() {
        let samples = cue_samples(CueKind::Start, 48_000);
        let db = cue_peak_db(&samples);
        assert!(
            db > -12.0 && db < -8.0,
            "peak {db} should stay near -10 dBFS"
        );
        assert!(samples.iter().all(|s| s.abs() <= CUE_PEAK + 1e-5));
        assert_relative_eq!(cue_envelope(0.02), CUE_PEAK, epsilon = 1e-4);
        assert!(cue_envelope(0.14) < 2e-4);
    }

    #[test]
    fn start_stop_cancel_share_peak_in_the_same_register() {
        let start = cue_samples(CueKind::Start, 48_000);
        let stop = cue_samples(CueKind::Stop, 48_000);
        let cancel = cue_samples(CueKind::Cancel, 48_000);
        let start_db = cue_peak_db(&start);
        let stop_db = cue_peak_db(&stop);
        let cancel_db = cue_peak_db(&cancel);
        assert!(
            (start_db - stop_db).abs() < 0.2,
            "start {start_db} stop {stop_db}"
        );
        assert!(
            (start_db - cancel_db).abs() < 0.2,
            "start {start_db} cancel {cancel_db}"
        );
        assert_eq!(cue_hz(CueKind::Start), 880.0);
        assert_eq!(cue_hz(CueKind::Stop), 698.46);
        assert_eq!(cue_hz(CueKind::Cancel), 880.0);
        assert_ne!(&start, &stop);
        assert_ne!(&start, &cancel);
        assert!(start.len() >= 48_000 / 10);
    }

    #[test]
    fn cancel_is_two_short_pings_without_a_long_tail() {
        let samples = cue_samples(CueKind::Cancel, 48_000);
        let blip = ((CANCEL_ATTACK_S + CANCEL_DECAY_S) * 48_000.0) as usize;
        let gap_n = (CANCEL_GAP_S * 48_000.0) as usize;
        let first = rms(&samples[..blip]);
        let gap = rms(&samples[blip..blip + gap_n]);
        let second = rms(&samples[blip + gap_n..]);
        assert!(first > 0.05, "first ping rms {first}");
        assert!(second > 0.05, "second ping rms {second}");
        assert!(gap < first * 0.12, "gap {gap} first {first}");
        assert!(gap < second * 0.12, "gap {gap} second {second}");
        assert_eq!(samples.len(), blip * 2 + gap_n);
    }

    #[test]
    fn surround_channels_stay_silent() {
        let mut frame = [1.0f32; 6];
        write_cue_channels(&mut frame, CUE_PEAK);
        assert_relative_eq!(frame[0], CUE_PEAK);
        assert_relative_eq!(frame[1], CUE_PEAK);
        assert_eq!(frame[2], 0.0);
        assert_eq!(frame[5], 0.0);
    }
}
