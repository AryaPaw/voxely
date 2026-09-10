use serde::{Deserialize, Serialize};

use super::metrics::{db_to_lin, lin_to_db};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct DynamicsConfig {
    pub threshold_db: f32,
    pub ratio: f32,
    pub attack_ms: f32,
    pub release_ms: f32,
    pub makeup_db: f32,
    pub sample_rate: f32,
}

impl DynamicsConfig {
    pub fn compressor_obs() -> Self {
        Self {
            threshold_db: -18.0,
            ratio: 3.0,
            attack_ms: 6.0,
            release_ms: 120.0,
            makeup_db: 3.0,
            sample_rate: 48_000.0,
        }
    }

    pub fn expander_obs() -> Self {
        Self {
            threshold_db: -45.0,
            ratio: 2.0,
            attack_ms: 5.0,
            release_ms: 150.0,
            makeup_db: 0.0,
            sample_rate: 48_000.0,
        }
    }

    pub fn compressor_stt() -> Self {
        Self {
            threshold_db: -22.0,
            ratio: 2.5,
            attack_ms: 8.0,
            release_ms: 80.0,
            makeup_db: 2.0,
            sample_rate: 48_000.0,
        }
    }

    pub fn expander_stt() -> Self {
        Self {
            threshold_db: -50.0,
            ratio: 1.6,
            attack_ms: 8.0,
            release_ms: 180.0,
            makeup_db: 0.0,
            sample_rate: 48_000.0,
        }
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.ratio < 1.0 {
            return Err("ratio must be >= 1".into());
        }
        if self.attack_ms <= 0.0 || self.release_ms <= 0.0 {
            return Err("attack and release must be positive".into());
        }
        Ok(())
    }
}

#[derive(Debug, Clone)]
pub struct Dynamics {
    config: DynamicsConfig,
    envelope: f32,
    attack_coeff: f32,
    release_coeff: f32,
    expand: bool,
}

impl Dynamics {
    pub fn compressor(config: DynamicsConfig) -> Result<Self, String> {
        config.validate()?;
        Ok(Self::new(config, false))
    }

    pub fn expander(config: DynamicsConfig) -> Result<Self, String> {
        config.validate()?;
        Ok(Self::new(config, true))
    }

    fn new(config: DynamicsConfig, expand: bool) -> Self {
        let attack_coeff = (-1.0 / (config.attack_ms * 0.001 * config.sample_rate)).exp();
        let release_coeff = (-1.0 / (config.release_ms * 0.001 * config.sample_rate)).exp();
        Self {
            config,
            envelope: 0.0,
            attack_coeff,
            release_coeff,
            expand,
        }
    }

    pub fn reset(&mut self) {
        self.envelope = 0.0;
    }

    pub fn process(&mut self, samples: &mut [f32]) {
        let threshold = db_to_lin(self.config.threshold_db);
        let makeup = db_to_lin(self.config.makeup_db);
        for sample in samples.iter_mut() {
            let abs = sample.abs();
            let coeff = if abs > self.envelope {
                self.attack_coeff
            } else {
                self.release_coeff
            };
            self.envelope = coeff * self.envelope + (1.0 - coeff) * abs;
            let env = self.envelope.max(1.0e-9);
            let gain = if self.expand {
                if env < threshold {
                    let below = lin_to_db(env) - self.config.threshold_db;
                    db_to_lin(below * (self.config.ratio - 1.0))
                } else {
                    1.0
                }
            } else if env > threshold {
                let over = lin_to_db(env) - self.config.threshold_db;
                db_to_lin(-over * (1.0 - 1.0 / self.config.ratio))
            } else {
                1.0
            };
            *sample *= gain * makeup;
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct GateConfig {
    pub open_threshold_db: f32,
    pub close_threshold_db: f32,
    pub hold_ms: f32,
    pub release_ms: f32,
    pub sample_rate: f32,
}

impl Default for GateConfig {
    fn default() -> Self {
        Self {
            open_threshold_db: -33.0,
            close_threshold_db: -35.0,
            hold_ms: 200.0,
            release_ms: 150.0,
            sample_rate: 48_000.0,
        }
    }
}

#[derive(Debug, Clone)]
pub struct NoiseGate {
    config: GateConfig,
    open: bool,
    hold_samples: u32,
    gain: f32,
}

impl NoiseGate {
    pub fn new(config: GateConfig) -> Self {
        Self {
            config,
            open: false,
            hold_samples: 0,
            gain: 0.0,
        }
    }

    pub fn reset(&mut self) {
        self.open = false;
        self.hold_samples = 0;
        self.gain = 0.0;
    }

    pub fn process(&mut self, samples: &mut [f32]) {
        let open_t = db_to_lin(self.config.open_threshold_db);
        let close_t = db_to_lin(self.config.close_threshold_db);
        let hold = (self.config.hold_ms * 0.001 * self.config.sample_rate) as u32;
        let release_coeff =
            (-1.0 / (self.config.release_ms * 0.001 * self.config.sample_rate)).exp();
        for sample in samples.iter_mut() {
            let abs = sample.abs();
            if abs >= open_t {
                self.open = true;
                self.hold_samples = hold;
                self.gain = 1.0;
            } else if self.open && abs < close_t {
                if self.hold_samples > 0 {
                    self.hold_samples -= 1;
                } else {
                    self.gain *= release_coeff;
                    if self.gain < 0.001 {
                        self.open = false;
                        self.gain = 0.0;
                    }
                }
            }
            *sample *= self.gain;
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct LimiterConfig {
    pub threshold_db: f32,
    pub release_ms: f32,
    pub sample_rate: f32,
}

impl Default for LimiterConfig {
    fn default() -> Self {
        Self {
            threshold_db: -1.0,
            release_ms: 50.0,
            sample_rate: 48_000.0,
        }
    }
}

#[derive(Debug, Clone)]
pub struct Limiter {
    config: LimiterConfig,
    gain: f32,
    release_coeff: f32,
}

impl Limiter {
    pub fn new(config: LimiterConfig) -> Self {
        let release_coeff = (-1.0 / (config.release_ms * 0.001 * config.sample_rate)).exp();
        Self {
            config,
            gain: 1.0,
            release_coeff,
        }
    }

    pub fn reset(&mut self) {
        self.gain = 1.0;
    }

    pub fn process(&mut self, samples: &mut [f32]) {
        let ceiling = db_to_lin(self.config.threshold_db);
        for sample in samples.iter_mut() {
            let needed = if sample.abs() * self.gain > ceiling {
                ceiling / sample.abs().max(1.0e-12)
            } else {
                1.0
            };
            if needed < self.gain {
                self.gain = needed;
            } else {
                self.gain = self.release_coeff * self.gain + (1.0 - self.release_coeff);
                self.gain = self.gain.min(1.0);
            }
            *sample *= self.gain;
            if sample.abs() > ceiling {
                *sample = sample.signum() * ceiling;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dsp::metrics::metrics;

    #[test]
    fn limiter_caps_peak() {
        let mut limiter = Limiter::new(LimiterConfig::default());
        let mut samples = vec![1.2f32, -1.1, 0.5];
        limiter.process(&mut samples);
        let m = metrics(&samples);
        assert!(m.peak <= db_to_lin(-1.0) + 1e-4);
    }

    #[test]
    fn compressor_reduces_loud_sine() {
        let mut comp = Dynamics::compressor(DynamicsConfig::compressor_obs()).unwrap();
        let mut samples: Vec<f32> = (0..2048).map(|i| (i as f32 * 0.05).sin() * 0.9).collect();
        let before = metrics(&samples).rms;
        comp.process(&mut samples);
        let after = metrics(&samples).rms;
        assert!(after < before);
    }

    #[test]
    fn expander_quiet_stays_finite() {
        let mut exp = Dynamics::expander(DynamicsConfig::expander_stt()).unwrap();
        let mut samples = vec![1e-4f32; 512];
        exp.process(&mut samples);
        assert!(samples.iter().all(|s| s.is_finite()));
    }
}
