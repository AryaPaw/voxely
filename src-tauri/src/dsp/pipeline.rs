use serde::{Deserialize, Serialize};

use super::dynamics::{Dynamics, DynamicsConfig, GateConfig, Limiter, LimiterConfig, NoiseGate};
use super::high_pass::{apply_gain, GainConfig, HighPass, HighPassConfig};
use super::metrics::{is_finite_buffer, metrics, AudioMetrics, SAMPLE_RATE, STT_SAMPLE_RATE};
use super::rnnoise::Rnnoise;
use crate::audio::resample::resample_linear;
use crate::error::AppError;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum FilterKind {
    HighPass,
    Rnnoise,
    Gain,
    Compressor,
    Expander,
    Gate,
    Limiter,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct FilterSlot {
    pub id: String,
    pub kind: FilterKind,
    pub enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct DspPreset {
    pub id: String,
    pub name: String,
    pub order: Vec<FilterSlot>,
    pub high_pass: HighPassConfig,
    pub gain: GainConfig,
    pub compressor: DynamicsConfig,
    pub expander: DynamicsConfig,
    pub gate: GateConfig,
    pub limiter: LimiterConfig,
}

impl DspPreset {
    pub fn obs_imported() -> Self {
        Self {
            id: "obs-imported".into(),
            name: "OBS Imported".into(),
            order: vec![
                slot("expander", FilterKind::Expander, true),
                slot("rnnoise", FilterKind::Rnnoise, true),
                slot("gate", FilterKind::Gate, false),
                slot("compressor", FilterKind::Compressor, true),
                slot("gain", FilterKind::Gain, true),
                slot("limiter", FilterKind::Limiter, true),
            ],
            high_pass: HighPassConfig::default(),
            gain: GainConfig { db: 2.0 },
            compressor: DynamicsConfig::compressor_obs(),
            expander: DynamicsConfig::expander_obs(),
            gate: GateConfig::default(),
            limiter: LimiterConfig {
                threshold_db: -1.0,
                release_ms: 60.0,
                sample_rate: 48_000.0,
            },
        }
    }

    pub fn stt_fast() -> Self {
        Self {
            id: "stt-fast".into(),
            name: "Быстрая диктовка".into(),
            order: vec![
                slot("highpass", FilterKind::HighPass, true),
                slot("gain", FilterKind::Gain, true),
                slot("limiter", FilterKind::Limiter, true),
            ],
            high_pass: HighPassConfig {
                cutoff_hz: 80.0,
                sample_rate: 48_000.0,
            },
            gain: GainConfig { db: 1.5 },
            compressor: DynamicsConfig::compressor_stt(),
            expander: DynamicsConfig::expander_stt(),
            gate: GateConfig::default(),
            limiter: LimiterConfig {
                threshold_db: -1.0,
                release_ms: 40.0,
                sample_rate: 48_000.0,
            },
        }
    }

    pub fn stt_optimized() -> Self {
        Self {
            id: "stt-optimized".into(),
            name: "Качество (медленнее)".into(),
            order: vec![
                slot("highpass", FilterKind::HighPass, true),
                slot("rnnoise", FilterKind::Rnnoise, true),
                slot("compressor", FilterKind::Compressor, true),
                slot("expander", FilterKind::Expander, true),
                slot("gain", FilterKind::Gain, true),
                slot("gate", FilterKind::Gate, false),
                slot("limiter", FilterKind::Limiter, true),
            ],
            high_pass: HighPassConfig {
                cutoff_hz: 70.0,
                sample_rate: 48_000.0,
            },
            gain: GainConfig { db: 1.5 },
            compressor: DynamicsConfig::compressor_stt(),
            expander: DynamicsConfig::expander_stt(),
            gate: GateConfig {
                open_threshold_db: -48.0,
                close_threshold_db: -52.0,
                hold_ms: 250.0,
                release_ms: 200.0,
                sample_rate: 48_000.0,
            },
            limiter: LimiterConfig {
                threshold_db: -1.0,
                release_ms: 40.0,
                sample_rate: 48_000.0,
            },
        }
    }
}

fn slot(id: &str, kind: FilterKind, enabled: bool) -> FilterSlot {
    FilterSlot {
        id: id.into(),
        kind,
        enabled,
    }
}

pub struct DspPipeline {
    preset: DspPreset,
    high_pass: HighPass,
    rnnoise: Rnnoise,
    compressor: Dynamics,
    expander: Dynamics,
    gate: NoiseGate,
    limiter: Limiter,
}

impl DspPipeline {
    pub fn new(preset: DspPreset) -> Result<Self, AppError> {
        Ok(Self {
            high_pass: HighPass::new(preset.high_pass).map_err(AppError::AudioProcessingFailed)?,
            rnnoise: Rnnoise::new(),
            compressor: Dynamics::compressor(preset.compressor)
                .map_err(AppError::AudioProcessingFailed)?,
            expander: Dynamics::expander(preset.expander)
                .map_err(AppError::AudioProcessingFailed)?,
            gate: NoiseGate::new(preset.gate),
            limiter: Limiter::new(preset.limiter),
            preset,
        })
    }

    pub fn process(&mut self, mut samples: Vec<f32>) -> Result<(Vec<f32>, AudioMetrics), AppError> {
        if samples.is_empty() {
            return Ok((samples, metrics(&[])));
        }
        let order = self.preset.order.clone();
        for slot in &order {
            if !slot.enabled {
                continue;
            }
            match slot.kind {
                FilterKind::HighPass => {
                    let mut out = vec![0.0; samples.len()];
                    self.high_pass.process(&samples, &mut out);
                    samples = out;
                }
                FilterKind::Rnnoise => {
                    let mut out = self.rnnoise.process(&samples);
                    out.extend(self.rnnoise.flush());
                    if out.len() < samples.len() {
                        out.resize(samples.len(), 0.0);
                    } else {
                        out.truncate(samples.len());
                    }
                    samples = out;
                }
                FilterKind::Gain => apply_gain(&mut samples, self.preset.gain),
                FilterKind::Compressor => self.compressor.process(&mut samples),
                FilterKind::Expander => self.expander.process(&mut samples),
                FilterKind::Gate => self.gate.process(&mut samples),
                FilterKind::Limiter => self.limiter.process(&mut samples),
            }
            if !is_finite_buffer(&samples) {
                return Err(AppError::AudioProcessingFailed(
                    "non-finite samples after filter".into(),
                ));
            }
        }
        let stats = metrics(&samples);
        Ok((samples, stats))
    }
}

pub fn prepare_listen(
    preset: DspPreset,
    samples: Vec<f32>,
) -> Result<(Vec<f32>, AudioMetrics), AppError> {
    let mut pipeline = DspPipeline::new(preset)?;
    pipeline.process(samples)
}

pub fn prepare_transcription(
    preset: DspPreset,
    samples: Vec<f32>,
) -> Result<(Vec<f32>, u32), AppError> {
    let (processed, _) = prepare_listen(preset, samples)?;
    Ok((
        resample_linear(&processed, SAMPLE_RATE, STT_SAMPLE_RATE),
        STT_SAMPLE_RATE,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_passthrough() {
        let mut pipeline = DspPipeline::new(DspPreset::stt_fast()).unwrap();
        let (out, _) = pipeline.process(vec![]).unwrap();
        assert!(out.is_empty());
    }

    #[test]
    fn transcription_prep_downsamples_to_stt_rate() {
        let sine: Vec<f32> = (0..4800)
            .map(|i| (i as f32 * 440.0 * 2.0 * std::f32::consts::PI / 48_000.0).sin() * 0.2)
            .collect();
        let preset =
            DspPreset::stt_fast().apply_mic_tune(&crate::dsp::mic_tune::MicTune::default());
        let (out, rate) = prepare_transcription(preset, sine).unwrap();
        assert_eq!(rate, 16_000);
        assert!(out.iter().all(|s| s.is_finite()));
        assert!(out.len() < 4800);
    }

    #[test]
    fn listen_prep_keeps_capture_rate() {
        let sine: Vec<f32> = (0..4800)
            .map(|i| (i as f32 * 440.0 * 2.0 * std::f32::consts::PI / 48_000.0).sin() * 0.2)
            .collect();
        let preset =
            DspPreset::stt_fast().apply_mic_tune(&crate::dsp::mic_tune::MicTune::default());
        let (out, metrics) = prepare_listen(preset, sine.clone()).unwrap();
        assert_eq!(out.len(), sine.len());
        assert!(metrics.peak > 0.0);
        assert!(metrics.peak <= 1.0);
    }

    #[test]
    fn sine_stays_finite() {
        let mut pipeline = DspPipeline::new(DspPreset::obs_imported()).unwrap();
        let sine: Vec<f32> = (0..4800)
            .map(|i| (i as f32 * 440.0 * 2.0 * std::f32::consts::PI / 48_000.0).sin() * 0.2)
            .collect();
        let (out, metrics) = pipeline.process(sine).unwrap();
        assert!(out.iter().all(|s| s.is_finite()));
        assert!(metrics.peak <= 1.0);
    }

    use proptest::prelude::*;

    proptest::proptest! {
        #[test]
        fn random_buffers_finite(samples in proptest::collection::vec(-1.0f32..1.0, 0..1024)) {
            let mut pipeline = DspPipeline::new(DspPreset::stt_optimized()).unwrap();
            let (out, _) = pipeline.process(samples).unwrap();
            prop_assert!(out.iter().all(|s| s.is_finite()));
        }
    }
}
