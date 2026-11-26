use crate::error::AppError;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CueKind {
    Start,
    Stop,
}

#[derive(Debug, Default, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum CuePreset {
    #[default]
    #[serde(alias = "short")]
    Classic,
    Soft,
    Warm,
}

impl CuePreset {
    const ALL: [Self; 3] = [Self::Classic, Self::Soft, Self::Warm];
}

const CUE_PEAK: f32 = 0.32;
const ATTACK_S: f32 = 0.02;
const DECAY_S: f32 = 0.12;
const ENV_FLOOR: f32 = 1e-4;
const FRONT_CHANNELS: usize = 2;

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
        CueKind::Start => 880.0,
        CueKind::Stop => 415.3,
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

fn cue_energy(samples: &[f32]) -> f32 {
    samples.iter().map(|sample| sample * sample).sum()
}

fn match_start_energy(samples: &mut [f32], start: &[f32]) {
    let target = cue_energy(start);
    let current = cue_energy(samples);
    if current > 1e-12 {
        let gain = (target / current).sqrt();
        for sample in samples {
            *sample *= gain;
        }
    }
}

fn classic_end_tone(sample_rate: u32) -> Vec<f32> {
    let duration = ATTACK_S + DECAY_S;
    let n = ((f64::from(duration) * f64::from(sample_rate)) as usize).max(1);
    let sr = sample_rate as f32;
    let mut samples = vec![0.0; n];
    let mut phase = 0.0f32;
    let f0 = cue_hz(CueKind::Stop);
    let f1 = 311.13;
    for (i, sample) in samples.iter_mut().enumerate() {
        let t = i as f32 / sr;
        let p = (t / duration).clamp(0.0, 1.0);
        let freq = f0 + (f1 - f0) * p * p;
        phase += freq * 2.0 * std::f32::consts::PI / sr;
        *sample = phase.sin() * envelope_at(t, ATTACK_S, DECAY_S, CUE_PEAK);
    }
    samples
}

pub fn parse_cue_kind(kind: &str) -> Result<CueKind, AppError> {
    match kind {
        "start" => Ok(CueKind::Start),
        "stop" | "cancel" => Ok(CueKind::Stop),
        _ => Err(AppError::RequestValidationFailed("unknown cue".into())),
    }
}

fn cue_samples(preset: CuePreset, kind: CueKind, sample_rate: u32) -> Vec<f32> {
    let mut start = ping_tone(cue_hz(CueKind::Start), sample_rate, ATTACK_S, DECAY_S);
    normalize_peak(&mut start);
    if preset != CuePreset::Classic {
        let mut samples = render_notes(preset, kind, sample_rate);
        match_start_energy(&mut samples, &start);
        let peak = samples
            .iter()
            .fold(0.0f32, |peak, sample| peak.max(sample.abs()));
        if peak > 0.39 {
            for sample in &mut samples {
                *sample *= 0.39 / peak;
            }
        }
        return samples;
    }
    match kind {
        CueKind::Start => start,
        CueKind::Stop => {
            let mut samples = classic_end_tone(sample_rate);
            normalize_peak(&mut samples);
            match_start_energy(&mut samples, &start);
            samples
        }
    }
}

struct Note {
    offset_ms: u32,
    duration_ms: u32,
    from_hz: f32,
    to_hz: f32,
    level: f32,
    overtone: f32,
}

const fn note(
    offset_ms: u32,
    duration_ms: u32,
    from_hz: f32,
    to_hz: f32,
    level: f32,
    overtone: f32,
) -> Note {
    Note {
        offset_ms,
        duration_ms,
        from_hz,
        to_hz,
        level,
        overtone,
    }
}

fn render_notes(preset: CuePreset, kind: CueKind, sample_rate: u32) -> Vec<f32> {
    use CueKind::{Start, Stop};
    use CuePreset::{Classic, Soft, Warm};
    let notes: &[Note] = match (preset, kind) {
        (Soft, Start) => &[
            note(0, 92, 620., 635., 1., 0.),
            note(45, 100, 880., 875., 0.8, 0.),
        ],
        (Soft, Stop) => &[
            note(0, 65, 314., 314., 1., 0.23),
            note(55, 61, 249., 249., 0.78, 0.16),
        ],
        (Warm, Start) => &[
            note(0, 110, 523., 523., 1., 0.25),
            note(37, 138, 784., 784., 0.85, 0.24),
        ],
        (Warm, Stop) => &[
            note(0, 126, 420., 285., 1., 0.25),
            note(13, 90, 230., 203., 0.4, 0.),
        ],
        (Classic, _) => unreachable!("classic cues use their original envelope"),
    };
    let frames = |ms| (f64::from(ms) * f64::from(sample_rate) / 1000.0).round() as usize;
    let length = notes
        .iter()
        .map(|note| frames(note.offset_ms) + frames(note.duration_ms))
        .max()
        .unwrap_or(0);
    let mut samples = vec![0.0; length];
    for note in notes {
        let offset = frames(note.offset_ms);
        let count = frames(note.duration_ms);
        let mut phase = 0.0f32;
        for (i, sample) in samples[offset..offset + count].iter_mut().enumerate() {
            let progress = i as f32 / count.saturating_sub(1).max(1) as f32;
            let attack = (std::f32::consts::FRAC_PI_2
                * (i as f32 / (sample_rate as f32 * 0.006)).min(1.0))
            .sin()
            .powi(2);
            let release =
                (progress * std::f32::consts::FRAC_PI_2).cos().powi(2) * (-3.0 * progress).exp();
            let frequency = note.from_hz + (note.to_hz - note.from_hz) * progress;
            phase += std::f32::consts::TAU * frequency / sample_rate as f32;
            let voiced = phase.sin() + note.overtone * (2.0 * phase).sin() * (1.0 - progress);
            *sample += voiced * attack * release * note.level;
        }
    }
    samples
}

fn pcm16(value: f32) -> i16 {
    (value * f32::from(i16::MAX)).round() as i16
}

pub fn cue_preview_bank() -> Vec<u8> {
    const SAMPLE_RATE: u32 = 48_000;
    // Little-endian sample rate, then (frame count, PCM16 samples) in preset and cue order.
    let mut bytes = SAMPLE_RATE.to_le_bytes().to_vec();
    for preset in CuePreset::ALL {
        for kind in [CueKind::Start, CueKind::Stop] {
            let samples = cue_samples(preset, kind, SAMPLE_RATE);
            let length = u16::try_from(samples.len()).expect("cue samples fit in 16-bit length");
            bytes.extend_from_slice(&length.to_le_bytes());
            for value in samples {
                bytes.extend_from_slice(&pcm16(value).to_le_bytes());
            }
        }
    }
    bytes
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

pub fn play_dictation_cue(preset: CuePreset, kind: CueKind) {
    std::thread::spawn(move || {
        if let Err(err) = play_blocking(preset, kind) {
            tracing::warn!(error = %err, "dictation cue failed");
        }
    });
}

fn play_blocking(preset: CuePreset, kind: CueKind) -> Result<(), AppError> {
    use cpal::traits::{DeviceTrait, HostTrait};
    let host = cpal::default_host();
    let device = host
        .default_output_device()
        .ok_or_else(|| AppError::AudioCaptureFailed("no output device".into()))?;
    let config = device
        .default_output_config()
        .map_err(|e| AppError::AudioCaptureFailed(e.to_string()))?;
    let sample_rate = config.sample_rate();
    let channels = config.channels() as usize;
    let samples = cue_samples(preset, kind, sample_rate);
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
    err_fn: impl FnMut(cpal::Error) + Send + 'static,
    drain_ms: u64,
) -> Result<(), AppError> {
    use cpal::traits::{DeviceTrait, StreamTrait};
    let mut index = 0usize;
    let stream = device
        .build_output_stream(
            config,
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
    err_fn: impl FnMut(cpal::Error) + Send + 'static,
    drain_ms: u64,
) -> Result<(), AppError> {
    use cpal::traits::{DeviceTrait, StreamTrait};
    let mut index = 0usize;
    let stream = device
        .build_output_stream(
            config,
            move |data: &mut [i16], _| {
                for frame in data.chunks_mut(channels) {
                    let value = samples.get(index).copied().unwrap_or(0.0);
                    index += 1;
                    let scaled = pcm16(value);
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
        let samples = cue_samples(CuePreset::Classic, CueKind::Start, 48_000);
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
    fn classic_end_matches_start_energy() {
        let start = cue_samples(CuePreset::Classic, CueKind::Start, 48_000);
        let stop = cue_samples(CuePreset::Classic, CueKind::Stop, 48_000);
        let start_energy = cue_energy(&start);
        let start_db = cue_peak_db(&start);
        assert!((cue_energy(&stop) - start_energy).abs() / start_energy < 0.02);
        assert!(cue_peak_db(&stop) <= start_db + 0.05);
        assert_eq!(cue_hz(CueKind::Start), 880.0);
        assert_eq!(cue_hz(CueKind::Stop), 415.3);
        assert_ne!(&start, &stop);
        assert_eq!(stop.len(), start.len());
    }

    #[test]
    fn legacy_cancel_name_resolves_to_the_shared_end_cue() {
        assert_eq!(parse_cue_kind("stop").unwrap(), CueKind::Stop);
        assert_eq!(parse_cue_kind("cancel").unwrap(), CueKind::Stop);
        assert!(parse_cue_kind("nope").is_err());
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

    #[test]
    fn cue_preview_bank_matches_native_cues() {
        let bank = cue_preview_bank();
        let rate = u32::from_le_bytes(bank[..4].try_into().unwrap());
        assert_eq!(rate, 48_000);
        let mut offset = 4;
        for preset in CuePreset::ALL {
            let clips = [CueKind::Start, CueKind::Stop].map(|kind| {
                let samples = cue_samples(preset, kind, rate);
                let length = u16::from_le_bytes(bank[offset..offset + 2].try_into().unwrap());
                offset += 2;
                assert_eq!(usize::from(length), samples.len());
                assert!((2_880..=8_400).contains(&samples.len()));
                assert!(samples
                    .iter()
                    .all(|sample| sample.is_finite() && sample.abs() <= 0.390_001));
                assert!(cue_peak_db(&samples) > -18.0);
                for &value in &samples {
                    assert_eq!(&bank[offset..offset + 2], &pcm16(value).to_le_bytes());
                    offset += 2;
                }
                samples
            });
            assert_ne!(clips[0], clips[1]);
        }
        assert_eq!(offset, bank.len());
    }

    #[cfg(windows)]
    #[test]
    #[ignore = "requires a real Windows audio output device and plays a cue"]
    fn plays_custom_cue_on_windows_audio_output() {
        play_blocking(CuePreset::Soft, CueKind::Stop).unwrap();
    }
}
