use serde::{Deserialize, Serialize};

use super::dynamics::{Dynamics, DynamicsConfig, GateConfig, Limiter, LimiterConfig, NoiseGate};
use super::high_pass::{apply_gain, GainConfig, HighPass, HighPassConfig};
use super::metrics::{is_finite_buffer, metrics, AudioMetrics, SAMPLE_RATE, STT_SAMPLE_RATE};
use super::rnnoise::Rnnoise;
use crate::audio::resample::resample_sinc;
use crate::error::AppError;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
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
    #[serde(default = "default_rnnoise_mix")]
    pub rnnoise_mix: f32,
}

fn default_rnnoise_mix() -> f32 {
    1.0
}

impl DspPreset {
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
            rnnoise_mix: 1.0,
        }
    }

    pub fn stt_optimized_revision_4() -> Self {
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
            rnnoise_mix: 1.0,
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
                slot("expander", FilterKind::Expander, false),
                slot("gain", FilterKind::Gain, true),
                slot("gate", FilterKind::Gate, false),
                slot("limiter", FilterKind::Limiter, true),
            ],
            high_pass: HighPassConfig {
                cutoff_hz: 70.0,
                sample_rate: 48_000.0,
            },
            gain: GainConfig { db: 1.5 },
            compressor: DynamicsConfig {
                threshold_db: -18.0,
                ratio: 1.8,
                attack_ms: 10.0,
                release_ms: 100.0,
                makeup_db: 1.0,
                sample_rate: 48_000.0,
            },
            expander: DynamicsConfig::expander_stt(),
            gate: GateConfig {
                open_threshold_db: -48.0,
                close_threshold_db: -52.0,
                hold_ms: 250.0,
                release_ms: 200.0,
                sample_rate: 48_000.0,
            },
            limiter: LimiterConfig {
                threshold_db: -0.8,
                release_ms: 50.0,
                sample_rate: 48_000.0,
            },
            rnnoise_mix: 0.6,
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

    pub fn process(&mut self, samples: Vec<f32>) -> Result<(Vec<f32>, AudioMetrics), AppError> {
        self.process_inner(samples, true)
    }

    pub fn process_audio(&mut self, samples: Vec<f32>) -> Result<Vec<f32>, AppError> {
        Ok(self.process_inner(samples, false)?.0)
    }

    fn process_inner(
        &mut self,
        mut samples: Vec<f32>,
        with_metrics: bool,
    ) -> Result<(Vec<f32>, AudioMetrics), AppError> {
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
                    self.high_pass.process_in_place(&mut samples);
                }
                FilterKind::Rnnoise => {
                    let mix = self.preset.rnnoise_mix.clamp(0.0, 1.0);
                    if mix <= 0.0 {
                        continue;
                    }
                    let mut out = self.rnnoise.process(&samples);
                    out.extend(self.rnnoise.flush());
                    if out.len() < samples.len() {
                        out.resize(samples.len(), 0.0);
                    } else {
                        out.truncate(samples.len());
                    }
                    if mix >= 1.0 {
                        samples = out;
                    } else {
                        for (dry_s, wet_s) in samples.iter_mut().zip(out.iter()) {
                            *dry_s = *dry_s * (1.0 - mix) + wet_s * mix;
                        }
                    }
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
        let stats = if with_metrics {
            metrics(&samples)
        } else {
            metrics(&[])
        };
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

pub fn prepare_listen_audio(preset: DspPreset, samples: Vec<f32>) -> Result<Vec<f32>, AppError> {
    let mut pipeline = DspPipeline::new(preset)?;
    pipeline.process_audio(samples)
}

pub fn apply_listen_loudness(samples: &[f32]) -> (Vec<f32>, AudioMetrics) {
    apply_listen_gain(samples, listen_gain(samples))
}

pub fn listen_gain(samples: &[f32]) -> f32 {
    let peak = samples
        .iter()
        .fold(0.0f32, |acc, sample| acc.max(sample.abs()));
    let target = 10f32.powf(-2.0 / 20.0);
    if peak > 1e-6 {
        (target / peak).min(8.0)
    } else {
        1.0
    }
}

pub fn apply_listen_gain(samples: &[f32], gain: f32) -> (Vec<f32>, AudioMetrics) {
    let out: Vec<f32> = samples
        .iter()
        .map(|sample| (sample * gain).clamp(-0.999, 1.0))
        .collect();
    let stats = metrics(&out);
    (out, stats)
}

pub type ListenPreview = (Vec<f32>, Vec<f32>, AudioMetrics, Vec<f32>);

pub fn match_pair_loudness(a: &[f32], b: &[f32]) -> (Vec<f32>, Vec<f32>) {
    let ra = metrics(a).rms.max(1.0e-6);
    let rb = metrics(b).rms.max(1.0e-6);
    let target = ra.max(rb);
    let scale_a = target / ra;
    let scale_b = target / rb;
    let out_a: Vec<f32> = a.iter().map(|s| (s * scale_a).clamp(-0.999, 1.0)).collect();
    let out_b: Vec<f32> = b.iter().map(|s| (s * scale_b).clamp(-0.999, 1.0)).collect();
    (out_a, out_b)
}

pub fn prepare_listen_preview(
    preset: DspPreset,
    samples: Vec<f32>,
) -> Result<ListenPreview, AppError> {
    let (filtered, _) = prepare_listen(preset, samples.clone())?;
    let stt = filtered.clone();
    let (original, _) = apply_listen_gain(&samples, listen_gain(&samples));
    let (preview, _) = apply_listen_gain(&filtered, listen_gain(&filtered));
    let (original, preview) = match_pair_loudness(&original, &preview);
    let preview_metrics = metrics(&preview);
    Ok((original, preview, preview_metrics, stt))
}

pub fn samples_for_stt(samples: &[f32], input_rate: u32) -> Vec<f32> {
    if input_rate == STT_SAMPLE_RATE {
        samples.to_vec()
    } else {
        resample_sinc(samples, input_rate, STT_SAMPLE_RATE)
    }
}

pub fn prepare_transcription(
    preset: DspPreset,
    samples: Vec<f32>,
) -> Result<(Vec<f32>, u32), AppError> {
    let (processed, _) = prepare_listen(preset, samples)?;
    Ok((samples_for_stt(&processed, SAMPLE_RATE), STT_SAMPLE_RATE))
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
        let preset = DspPreset::stt_fast();
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
        let preset = DspPreset::stt_fast();
        let (out, metrics) = prepare_listen(preset, sine.clone()).unwrap();
        assert_eq!(out.len(), sine.len());
        assert!(metrics.peak > 0.0);
        assert!(metrics.peak <= 1.0);
    }

    #[test]
    fn listen_loudness_does_not_change_stt_buffer() {
        let sine: Vec<f32> = (0..4800)
            .map(|i| (i as f32 * 440.0 * 2.0 * std::f32::consts::PI / 48_000.0).sin() * 0.01)
            .collect();
        let (original, preview, metrics, stt) =
            prepare_listen_preview(DspPreset::stt_fast(), sine.clone()).unwrap();
        let (listen, _) = prepare_listen(DspPreset::stt_fast(), sine).unwrap();
        assert_eq!(stt, listen);
        assert!(preview.iter().all(|s| s.abs() <= 1.0));
        assert_eq!(metrics.clip_count, 0);
        assert!(metrics.peak > 0.0);
        let original_peak = original.iter().fold(0.0f32, |a, s| a.max(s.abs()));
        let preview_peak = preview.iter().fold(0.0f32, |a, s| a.max(s.abs()));
        assert!(original_peak > 0.0);
        assert!(preview_peak > 0.0);
    }

    #[test]
    fn listen_preview_matches_loudness_without_changing_stt() {
        let sine: Vec<f32> = (0..4800)
            .map(|i| (i as f32 * 440.0 * 2.0 * std::f32::consts::PI / 48_000.0).sin() * 0.05)
            .collect();
        let six = gain_only(6.0);
        let twelve = gain_only(12.0);
        let (original_six, preview_six, _, stt_six) =
            prepare_listen_preview(six.clone(), sine.clone()).unwrap();
        let (listen, _) = prepare_listen(six, sine.clone()).unwrap();
        assert_eq!(stt_six, listen);
        let (original_twelve, preview_twelve, _, _) = prepare_listen_preview(twelve, sine).unwrap();
        let original_six_peak = original_six.iter().fold(0.0f32, |a, s| a.max(s.abs()));
        let original_twelve_peak = original_twelve.iter().fold(0.0f32, |a, s| a.max(s.abs()));
        assert!((original_six_peak - original_twelve_peak).abs() < 0.05);
        let six_rms = metrics(&preview_six).rms;
        let orig_rms = metrics(&original_six).rms;
        assert!((six_rms - orig_rms).abs() < 0.02);
        let twelve_rms = metrics(&preview_twelve).rms;
        let orig12 = metrics(&original_twelve).rms;
        assert!((twelve_rms - orig12).abs() < 0.02);
    }

    fn gain_only(db: f32) -> DspPreset {
        let mut preset = DspPreset::stt_fast();
        preset.gain.db = db;
        for slot in &mut preset.order {
            slot.enabled = slot.kind == FilterKind::Gain;
        }
        preset
    }

    #[test]
    fn extra_gain_raises_preview_rms_without_changing_stt_order() {
        let sine: Vec<f32> = (0..4800)
            .map(|i| (i as f32 * 440.0 * 2.0 * std::f32::consts::PI / 48_000.0).sin() * 0.05)
            .collect();
        let six = gain_only(6.0);
        let twelve = gain_only(12.0);
        assert_eq!(six.order, twelve.order);
        let (_, six_metrics) = prepare_listen(six.clone(), sine.clone()).unwrap();
        let (_, twelve_metrics) = prepare_listen(twelve, sine.clone()).unwrap();
        assert!(twelve_metrics.rms > six_metrics.rms);
        let (_, stt_rate) = prepare_transcription(six, sine).unwrap();
        assert_eq!(stt_rate, STT_SAMPLE_RATE);
    }

    #[test]
    fn factory_quality_disables_expander_and_mixes_rnnoise() {
        let quality = DspPreset::stt_optimized();
        assert!(!quality
            .order
            .iter()
            .any(|slot| slot.kind == FilterKind::Expander && slot.enabled));
        assert!(quality
            .order
            .iter()
            .any(|slot| slot.kind == FilterKind::Rnnoise && slot.enabled));
        assert!((quality.rnnoise_mix - 0.6).abs() < f32::EPSILON);
    }

    #[test]
    fn rnnoise_mix_zero_keeps_input_length() {
        let sine: Vec<f32> = (0..4800)
            .map(|i| (i as f32 * 440.0 * 2.0 * std::f32::consts::PI / 48_000.0).sin() * 0.2)
            .collect();
        let mut dry = DspPreset::stt_optimized();
        dry.order.retain(|slot| slot.kind == FilterKind::Rnnoise);
        dry.rnnoise_mix = 0.0;
        let (out, _) = prepare_listen(dry, sine.clone()).unwrap();
        assert_eq!(out.len(), sine.len());
        assert_eq!(out, sine);
        let mut wet = DspPreset::stt_optimized();
        wet.order.retain(|slot| slot.kind == FilterKind::Rnnoise);
        wet.rnnoise_mix = 1.0;
        let (wet_out, _) = prepare_listen(wet, sine.clone()).unwrap();
        assert_eq!(wet_out.len(), sine.len());
        assert_ne!(wet_out, sine);
    }

    #[test]
    fn rnnoise_mix_partial_is_weighted() {
        let sine: Vec<f32> = (0..4800)
            .map(|i| (i as f32 * 440.0 * 2.0 * std::f32::consts::PI / 48_000.0).sin() * 0.2)
            .collect();
        let mut base = DspPreset::stt_optimized();
        base.order.retain(|slot| slot.kind == FilterKind::Rnnoise);
        let mut dry = base.clone();
        dry.rnnoise_mix = 0.0;
        let mut wet = base.clone();
        wet.rnnoise_mix = 1.0;
        let mut mixed = base;
        mixed.rnnoise_mix = 0.6;
        let dry_out = prepare_listen_audio(dry, sine.clone()).unwrap();
        let wet_out = prepare_listen_audio(wet, sine.clone()).unwrap();
        let mix_out = prepare_listen_audio(mixed, sine).unwrap();
        assert_eq!(mix_out.len(), dry_out.len());
        for i in 0..mix_out.len() {
            let expected = dry_out[i] * 0.4 + wet_out[i] * 0.6;
            assert!((mix_out[i] - expected).abs() < 1e-4);
        }
    }

    #[test]
    fn process_audio_matches_process_without_using_metrics() {
        let sine: Vec<f32> = (0..4800)
            .map(|i| (i as f32 * 440.0 * 2.0 * std::f32::consts::PI / 48_000.0).sin() * 0.2)
            .collect();
        let audio = prepare_listen_audio(DspPreset::stt_optimized(), sine.clone()).unwrap();
        let (with_metrics, metrics) = prepare_listen(DspPreset::stt_optimized(), sine).unwrap();
        assert_eq!(audio, with_metrics);
        assert!(metrics.peak > 0.0);
    }

    #[test]
    fn transcription_prep_uses_sinc_not_linear() {
        let sine: Vec<f32> = (0..4800)
            .map(|i| (i as f32 * 440.0 * 2.0 * std::f32::consts::PI / 48_000.0).sin() * 0.2)
            .collect();
        let (processed, _) = prepare_listen(DspPreset::stt_fast(), sine.clone()).unwrap();
        let out = samples_for_stt(&processed, SAMPLE_RATE);
        let linear =
            crate::audio::resample::resample_linear(&processed, SAMPLE_RATE, STT_SAMPLE_RATE);
        assert_ne!(out, linear);
    }

    #[test]
    fn sine_stays_finite() {
        let mut pipeline = DspPipeline::new(DspPreset::stt_optimized()).unwrap();
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
