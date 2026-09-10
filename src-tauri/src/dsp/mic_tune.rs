use serde::{Deserialize, Serialize};

use crate::dsp::pipeline::{DspPreset, FilterKind, FilterSlot};
use crate::error::AppError;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct MicTune {
    pub gain_db: f32,
    pub highpass_hz: f32,
    pub denoise: u8,
    pub punch: u8,
}

impl Default for MicTune {
    fn default() -> Self {
        Self {
            gain_db: 1.5,
            highpass_hz: 80.0,
            denoise: 0,
            punch: 18,
        }
    }
}

impl MicTune {
    pub fn validate(&self) -> Result<(), AppError> {
        if !(-12.0..=18.0).contains(&self.gain_db) {
            return Err(AppError::RequestValidationFailed(
                "gain must be between -12 and 18 dB".into(),
            ));
        }
        if !(20.0..=200.0).contains(&self.highpass_hz) {
            return Err(AppError::RequestValidationFailed(
                "high-pass must be 20-200 Hz".into(),
            ));
        }
        if self.denoise > 100 || self.punch > 100 {
            return Err(AppError::RequestValidationFailed(
                "denoise and punch must be 0-100".into(),
            ));
        }
        Ok(())
    }
}

impl DspPreset {
    pub fn apply_mic_tune(&self, tune: &MicTune) -> Self {
        let mut preset = self.clone();
        preset.gain.db = tune.gain_db;
        preset.high_pass.cutoff_hz = tune.highpass_hz;
        let punch = f32::from(tune.punch);
        let denoise = f32::from(tune.denoise);
        preset.compressor.ratio = 1.15 + punch / 100.0 * 2.2;
        preset.compressor.threshold_db = -15.0 - punch * 0.1;
        preset.compressor.makeup_db = punch * 0.02;
        preset.expander.ratio = 1.2 + denoise / 100.0 * 1.1;
        preset.expander.threshold_db = -44.0 - denoise * 0.1;
        let gate_on = preset
            .order
            .iter()
            .find(|slot| slot.kind == FilterKind::Gate)
            .is_some_and(|slot| slot.enabled);
        preset.order = vec![
            slot("highpass", FilterKind::HighPass, true),
            slot("rnnoise", FilterKind::Rnnoise, tune.denoise >= 20),
            slot("expander", FilterKind::Expander, tune.denoise >= 50),
            slot("gate", FilterKind::Gate, gate_on),
            slot("compressor", FilterKind::Compressor, tune.punch >= 10),
            slot("gain", FilterKind::Gain, true),
            slot("limiter", FilterKind::Limiter, true),
        ];
        preset
    }
}

fn slot(id: &str, kind: FilterKind, enabled: bool) -> FilterSlot {
    FilterSlot {
        id: id.into(),
        kind,
        enabled,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_keeps_voice_natural() {
        let tuned = DspPreset::stt_fast().apply_mic_tune(&MicTune::default());
        assert!(!enabled(&tuned, FilterKind::Rnnoise));
        assert!(enabled(&tuned, FilterKind::Compressor));
        assert!(enabled(&tuned, FilterKind::Limiter));
        assert!((tuned.gain.db - 1.5).abs() < f32::EPSILON);
        assert!((tuned.high_pass.cutoff_hz - 80.0).abs() < f32::EPSILON);
    }

    #[test]
    fn zero_punch_disables_compressor() {
        let mut tune = MicTune::default();
        tune.punch = 0;
        let tuned = DspPreset::stt_fast().apply_mic_tune(&tune);
        assert!(!enabled(&tuned, FilterKind::Compressor));
    }

    #[test]
    fn denoise_ladder() {
        let mut tune = MicTune::default();
        tune.denoise = 19;
        let off = DspPreset::stt_fast().apply_mic_tune(&tune);
        assert!(!enabled(&off, FilterKind::Rnnoise));
        assert!(!enabled(&off, FilterKind::Expander));
        tune.denoise = 20;
        let light = DspPreset::stt_fast().apply_mic_tune(&tune);
        assert!(enabled(&light, FilterKind::Rnnoise));
        assert!(!enabled(&light, FilterKind::Expander));
        tune.denoise = 50;
        let heavy = DspPreset::stt_fast().apply_mic_tune(&tune);
        assert!(enabled(&heavy, FilterKind::Rnnoise));
        assert!(enabled(&heavy, FilterKind::Expander));
    }

    #[test]
    fn validate_rejects_out_of_range() {
        let mut tune = MicTune::default();
        tune.gain_db = 40.0;
        assert!(tune.validate().is_err());
        tune.gain_db = 1.5;
        tune.denoise = 101;
        assert!(tune.validate().is_err());
    }

    fn enabled(preset: &DspPreset, kind: FilterKind) -> bool {
        preset
            .order
            .iter()
            .find(|slot| slot.kind == kind)
            .is_some_and(|slot| slot.enabled)
    }
}
