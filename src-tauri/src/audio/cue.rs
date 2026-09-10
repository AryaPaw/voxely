use crate::error::AppError;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CueKind {
    Start,
    Stop,
}

pub fn cue_samples(kind: CueKind, sample_rate: u32) -> Vec<f32> {
    let freq = match kind {
        CueKind::Start => 880.0,
        CueKind::Stop => 392.0,
    };
    let duration = 0.12;
    let peak = 10f32.powf(-9.0 / 20.0);
    let n = ((duration * f64::from(sample_rate)) as usize).max(1);
    (0..n)
        .map(|i| {
            let t = i as f32 / sample_rate as f32;
            let env = if t < 0.012 {
                t / 0.012
            } else if t > 0.1 {
                ((duration as f32 - t) / 0.02).max(0.0)
            } else {
                1.0
            };
            (t * freq * 2.0 * std::f32::consts::PI).sin() * peak * env
        })
        .collect()
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
    use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
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
    let mut index = 0usize;
    let err_fn = |err| tracing::warn!(error = %err, "cue stream");
    match config.sample_format() {
        cpal::SampleFormat::F32 => {
            let stream = device
                .build_output_stream(
                    &config.into(),
                    move |data: &mut [f32], _| {
                        for frame in data.chunks_mut(channels) {
                            let value = samples.get(index).copied().unwrap_or(0.0);
                            index += 1;
                            for sample in frame {
                                *sample = value;
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
            std::thread::sleep(std::time::Duration::from_millis(160));
            Ok(())
        }
        _ => Err(AppError::AudioCaptureFailed(
            "unsupported output format".into(),
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cue_peak_is_minus_twelve_to_minus_six() {
        let samples = cue_samples(CueKind::Start, 48_000);
        let db = cue_peak_db(&samples);
        assert!(db > -12.5 && db < -6.0, "peak {db}");
        assert!(samples.iter().all(|s| s.abs() <= 1.0));
    }
}
