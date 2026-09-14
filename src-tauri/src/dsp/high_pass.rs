use serde::{Deserialize, Serialize};

use super::metrics::db_to_lin;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct HighPassConfig {
    pub cutoff_hz: f32,
    pub sample_rate: f32,
}

impl Default for HighPassConfig {
    fn default() -> Self {
        Self {
            cutoff_hz: 80.0,
            sample_rate: 48_000.0,
        }
    }
}

#[derive(Debug, Clone)]
pub struct HighPass {
    config: HighPassConfig,
    b0: f32,
    b1: f32,
    b2: f32,
    a1: f32,
    a2: f32,
    x1: f32,
    x2: f32,
    y1: f32,
    y2: f32,
}

impl HighPass {
    pub fn new(config: HighPassConfig) -> Result<Self, String> {
        if config.cutoff_hz <= 0.0 || config.cutoff_hz >= config.sample_rate / 2.0 {
            return Err("high-pass cutoff out of range".into());
        }
        let mut filter = Self {
            config,
            b0: 0.0,
            b1: 0.0,
            b2: 0.0,
            a1: 0.0,
            a2: 0.0,
            x1: 0.0,
            x2: 0.0,
            y1: 0.0,
            y2: 0.0,
        };
        filter.recompute();
        Ok(filter)
    }

    fn recompute(&mut self) {
        let w0 = 2.0 * std::f32::consts::PI * self.config.cutoff_hz / self.config.sample_rate;
        let cos = w0.cos();
        let sin = w0.sin();
        let q = std::f32::consts::FRAC_1_SQRT_2;
        let alpha = sin / (2.0 * q);
        let b0 = (1.0 + cos) / 2.0;
        let b1 = -(1.0 + cos);
        let b2 = (1.0 + cos) / 2.0;
        let a0 = 1.0 + alpha;
        let a1 = -2.0 * cos;
        let a2 = 1.0 - alpha;
        self.b0 = b0 / a0;
        self.b1 = b1 / a0;
        self.b2 = b2 / a0;
        self.a1 = a1 / a0;
        self.a2 = a2 / a0;
    }

    pub fn reset(&mut self) {
        self.x1 = 0.0;
        self.x2 = 0.0;
        self.y1 = 0.0;
        self.y2 = 0.0;
    }

    pub fn process(&mut self, input: &[f32], output: &mut [f32]) {
        for (i, &x) in input.iter().enumerate() {
            let y = self.b0 * x + self.b1 * self.x1 + self.b2 * self.x2
                - self.a1 * self.y1
                - self.a2 * self.y2;
            self.x2 = self.x1;
            self.x1 = x;
            self.y2 = self.y1;
            self.y1 = y;
            output[i] = y;
        }
    }

    #[allow(clippy::needless_range_loop)]
    pub fn process_in_place(&mut self, samples: &mut [f32]) {
        for i in 0..samples.len() {
            let x = samples[i];
            let y = self.b0 * x + self.b1 * self.x1 + self.b2 * self.x2
                - self.a1 * self.y1
                - self.a2 * self.y2;
            self.x2 = self.x1;
            self.x1 = x;
            self.y2 = self.y1;
            self.y1 = y;
            samples[i] = y;
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct GainConfig {
    pub db: f32,
}

impl Default for GainConfig {
    fn default() -> Self {
        Self { db: 0.0 }
    }
}

pub fn apply_gain(samples: &mut [f32], config: GainConfig) {
    let g = db_to_lin(config.db);
    for s in samples {
        *s *= g;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn high_pass_silence() {
        let mut hp = HighPass::new(HighPassConfig::default()).unwrap();
        let input = [0.0f32; 64];
        let mut out = [0.0f32; 64];
        hp.process(&input, &mut out);
        assert!(out.iter().all(|s| s.abs() < 1e-6));
    }

    #[test]
    fn high_pass_in_place_matches_out_of_place() {
        let input: Vec<f32> = (0..128).map(|i| (i as f32 * 0.07).sin() * 0.4).collect();
        let mut a = HighPass::new(HighPassConfig::default()).unwrap();
        let mut b = HighPass::new(HighPassConfig::default()).unwrap();
        let mut out = vec![0.0; input.len()];
        a.process(&input, &mut out);
        let mut inplace = input.clone();
        b.process_in_place(&mut inplace);
        for (left, right) in out.iter().zip(inplace.iter()) {
            assert!((left - right).abs() < 1e-6);
        }
    }

    #[test]
    fn gain_plus_six() {
        let mut samples = [0.5f32];
        apply_gain(&mut samples, GainConfig { db: 6.0 });
        assert!((samples[0] - 0.5 * db_to_lin(6.0)).abs() < 1e-5);
    }
}
