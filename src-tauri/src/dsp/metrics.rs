pub const SAMPLE_RATE: u32 = 48_000;
pub const STT_SAMPLE_RATE: u32 = 16_000;
pub const CHANNELS: u16 = 1;

#[derive(Debug, Clone, Copy)]
pub struct AudioMetrics {
    pub peak: f32,
    pub rms: f32,
    pub clip_count: u32,
    pub estimated_noise_floor: f32,
}

pub fn metrics(samples: &[f32]) -> AudioMetrics {
    if samples.is_empty() {
        return AudioMetrics {
            peak: 0.0,
            rms: 0.0,
            clip_count: 0,
            estimated_noise_floor: 0.0,
        };
    }
    let mut peak = 0.0f32;
    let mut sum_sq = 0.0f32;
    let mut clip_count = 0u32;
    let mut abs_sorted = Vec::with_capacity(samples.len());
    for &s in samples {
        let a = s.abs();
        peak = peak.max(a);
        sum_sq += s * s;
        if a >= 0.999 {
            clip_count += 1;
        }
        abs_sorted.push(a);
    }
    abs_sorted.sort_by(|a, b| a.total_cmp(b));
    let p10 = abs_sorted[abs_sorted.len() / 10];
    AudioMetrics {
        peak,
        rms: (sum_sq / samples.len() as f32).sqrt(),
        clip_count,
        estimated_noise_floor: p10,
    }
}

pub fn db_to_lin(db: f32) -> f32 {
    10f32.powf(db / 20.0)
}

pub fn lin_to_db(lin: f32) -> f32 {
    20.0 * lin.max(1.0e-12).log10()
}

pub fn is_finite_buffer(samples: &[f32]) -> bool {
    samples.iter().all(|s| s.is_finite())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn silence_metrics() {
        let m = metrics(&[0.0; 1024]);
        assert_eq!(m.peak, 0.0);
        assert_eq!(m.rms, 0.0);
        assert_eq!(m.clip_count, 0);
    }
}
