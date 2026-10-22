//! Shorten only confidently quiet pauses in the upload copy, never retained audio.
use crate::error::AppError;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default, deny_unknown_fields, rename_all = "camelCase")]
pub struct SilenceCompressionSettings {
    pub enabled: bool,
    pub max_pause_ms: u32,
}

impl Default for SilenceCompressionSettings {
    fn default() -> Self {
        Self {
            enabled: true,
            max_pause_ms: 2_000,
        }
    }
}

impl SilenceCompressionSettings {
    pub fn validate(&self) -> Result<(), AppError> {
        if !(500..=5_000).contains(&self.max_pause_ms) {
            return Err(AppError::RequestValidationFailed(
                "retained pause must be 500-5000ms".into(),
            ));
        }
        Ok(())
    }
}

pub fn compress_silence(
    samples: &mut Vec<f32>,
    sample_rate: u32,
    settings: &SilenceCompressionSettings,
) -> Result<(), AppError> {
    settings.validate()?;
    if !settings.enabled || samples.is_empty() {
        return Ok(());
    }
    if sample_rate == 0 || samples.iter().any(|s| !s.is_finite()) {
        return Err(AppError::AudioProcessingFailed(
            "invalid silence compression input".into(),
        ));
    }
    let frame_len = (sample_rate as usize / 100).max(1);
    let keep = (u64::from(sample_rate) * u64::from(settings.max_pause_ms) / 1000) as usize;
    // Avoid tiny edits around the requested limit. Keep both edges of a pause,
    // including at least 250ms beside speech with the minimum allowed setting.
    let minimum = keep + (sample_rate as usize / 2).max(1);
    let rms_limit = 10.0f64.powf(-65.0 / 20.0);
    let peak_limit = 10.0f64.powf(-55.0 / 20.0);
    let mut quiet_start = None;
    let mut removals = Vec::new();
    let mut finish = |start: usize, end: usize| {
        if end - start >= minimum {
            removals.push((start + keep / 2, end - (keep - keep / 2)));
        }
    };
    for (index, frame) in samples.chunks(frame_len).enumerate() {
        let sum = frame.iter().map(|&s| f64::from(s).powi(2)).sum::<f64>();
        let peak = frame
            .iter()
            .fold(0.0f64, |peak, &s| peak.max(f64::from(s).abs()));
        // Conservative level detection, not speech recognition. Ambiguous noise
        // and quiet speech above either threshold are retained, without guessing.
        if sum / frame.len() as f64 <= rms_limit * rms_limit && peak <= peak_limit {
            quiet_start.get_or_insert(index * frame_len);
        } else if let Some(start) = quiet_start.take() {
            finish(start, index * frame_len);
        }
    }
    if let Some(start) = quiet_start {
        finish(start, samples.len());
    }
    let mut read = 0;
    let mut write = 0;
    let mut joins = Vec::with_capacity(removals.len());
    for (start, end) in removals {
        samples.copy_within(read..start, write);
        write += start - read;
        joins.push(write);
        read = end;
    }
    let tail = samples.len() - read;
    samples.copy_within(read.., write);
    samples.truncate(write + tail);
    // A 5ms fade lies entirely inside retained quiet audio, protecting speech
    // and avoiding a discontinuity when low-level background samples are joined.
    let fade_len = (sample_rate as usize / 200).max(1);
    for join in joins {
        let length = fade_len.min(join).min(samples.len() - join);
        for i in 0..length {
            let gain = i as f32 / length as f32;
            samples[join - 1 - i] *= gain;
            samples[join + i] *= gain;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    const RATE: u32 = 16_000;
    fn recording(pause: usize) -> Vec<f32> {
        let mut samples = vec![0.01; RATE as usize];
        samples.extend(vec![0.00001; RATE as usize * pause]);
        samples.extend(vec![-0.02; RATE as usize]);
        samples
    }

    #[test]
    fn long_quiet_pause_is_capped_and_speech_is_unchanged() {
        let mut samples = recording(10);
        compress_silence(&mut samples, RATE, &SilenceCompressionSettings::default()).unwrap();
        assert_eq!(samples.len(), RATE as usize * 4);
        assert_eq!(&samples[..RATE as usize], vec![0.01; RATE as usize]);
        assert_eq!(
            &samples[samples.len() - RATE as usize..],
            vec![-0.02; RATE as usize]
        );
        assert!(samples.iter().all(|s| s.is_finite()));
    }

    #[test]
    fn disabled_short_pauses_and_quiet_speech_are_bit_exact() {
        let original = recording(10);
        let mut samples = original.clone();
        let off = SilenceCompressionSettings {
            enabled: false,
            ..Default::default()
        };
        compress_silence(&mut samples, RATE, &off).unwrap();
        assert_eq!(samples, original);
        let original = recording(2);
        let mut samples = original.clone();
        compress_silence(&mut samples, RATE, &SilenceCompressionSettings::default()).unwrap();
        assert_eq!(samples, original);
        // Quiet signal: about -61 dBFS. Peak guard also protects brief transients.
        for original in [
            vec![0.0009; RATE as usize * 10],
            (0..RATE as usize * 10)
                .map(|i| if i % 160 == 0 { 0.003 } else { 0.0 })
                .collect(),
        ] {
            let mut samples = original.clone();
            compress_silence(&mut samples, RATE, &SilenceCompressionSettings::default()).unwrap();
            assert_eq!(samples, original);
        }
    }

    #[test]
    fn handles_edges_all_silence_partial_frames_and_custom_pause() {
        let mut samples = vec![0.0; RATE as usize * 10 + 7];
        compress_silence(&mut samples, RATE, &SilenceCompressionSettings::default()).unwrap();
        assert_eq!(samples.len(), RATE as usize * 2);
        let mut samples = vec![0.0; RATE as usize * 3];
        samples.extend(vec![0.01; RATE as usize]);
        samples.extend(vec![0.0; RATE as usize * 4]);
        compress_silence(&mut samples, RATE, &SilenceCompressionSettings::default()).unwrap();
        assert_eq!(samples.len(), RATE as usize * 5);
        assert_eq!(
            &samples[RATE as usize * 2..RATE as usize * 3],
            vec![0.01; RATE as usize]
        );
        let mut samples = recording(10);
        compress_silence(
            &mut samples,
            RATE,
            &SilenceCompressionSettings {
                enabled: true,
                max_pause_ms: 500,
            },
        )
        .unwrap();
        assert_eq!(samples.len(), RATE as usize * 2 + 8_000);
        let mut empty = vec![];
        compress_silence(&mut empty, RATE, &SilenceCompressionSettings::default()).unwrap();
        assert!(empty.is_empty());
    }

    #[test]
    fn multiple_pauses_preserve_every_voice_segment_and_smooth_quiet_joins() {
        let mut samples = vec![0.02; RATE as usize];
        samples.extend(vec![0.0002; RATE as usize * 6]);
        samples.extend(vec![-0.03; RATE as usize]);
        samples.extend(vec![0.0002; RATE as usize * 8]);
        samples.extend(vec![0.04; RATE as usize + 7]);
        compress_silence(&mut samples, RATE, &Default::default()).unwrap();
        assert_eq!(samples.len(), RATE as usize * 7 + 7);
        assert_eq!(&samples[..RATE as usize], vec![0.02; RATE as usize]);
        assert_eq!(
            &samples[RATE as usize * 3..RATE as usize * 4],
            vec![-0.03; RATE as usize]
        );
        assert_eq!(&samples[RATE as usize * 6..], vec![0.04; RATE as usize + 7]);
        for join in [RATE as usize * 2, RATE as usize * 5] {
            assert_eq!(samples[join - 1], 0.0);
            assert_eq!(samples[join], 0.0);
            assert!(samples[join - 80..join + 80]
                .iter()
                .all(|sample| (0.0..=0.0002).contains(sample)));
        }
    }

    #[test]
    fn validates_config_and_rejects_nonfinite_audio() {
        for max_pause_ms in [0, 499, 5001, u32::MAX] {
            assert!(SilenceCompressionSettings {
                enabled: false,
                max_pause_ms
            }
            .validate()
            .is_err());
        }
        for value in [f32::NAN, f32::INFINITY] {
            assert!(compress_silence(
                &mut vec![value],
                RATE,
                &SilenceCompressionSettings::default()
            )
            .is_err());
        }
        assert!(
            compress_silence(&mut vec![0.0], 0, &SilenceCompressionSettings::default()).is_err()
        );
    }
}
