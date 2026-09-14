use nnnoiseless::DenoiseState;

pub const FRAME: usize = DenoiseState::FRAME_SIZE;

pub struct Rnnoise {
    state: Box<DenoiseState<'static>>,
    leftover: Vec<f32>,
    work: Vec<f32>,
    output: Vec<f32>,
    frame_in: [f32; FRAME],
    frame_out: [f32; FRAME],
}

impl Rnnoise {
    pub fn new() -> Self {
        Self {
            state: DenoiseState::new(),
            leftover: Vec::with_capacity(FRAME),
            work: Vec::new(),
            output: Vec::new(),
            frame_in: [0.0; FRAME],
            frame_out: [0.0; FRAME],
        }
    }

    pub fn reset(&mut self) {
        self.state = DenoiseState::new();
        self.leftover.clear();
        self.work.clear();
        self.output.clear();
    }

    pub fn process(&mut self, input: &[f32]) -> Vec<f32> {
        self.process_into(input);
        self.output.clone()
    }

    pub fn process_into(&mut self, input: &[f32]) -> &[f32] {
        self.work.clear();
        self.work.extend_from_slice(&self.leftover);
        self.work.extend_from_slice(input);
        self.leftover.clear();
        self.output.clear();
        let mut index = 0;
        while index + FRAME <= self.work.len() {
            for i in 0..FRAME {
                self.frame_in[i] = self.work[index + i] * 32768.0;
            }
            let _ = self
                .state
                .process_frame(&mut self.frame_out, &self.frame_in);
            for sample in self.frame_out {
                self.output.push((sample / 32768.0).clamp(-1.0, 1.0));
            }
            index += FRAME;
        }
        self.leftover.extend_from_slice(&self.work[index..]);
        &self.output
    }

    pub fn flush(&mut self) -> Vec<f32> {
        if self.leftover.is_empty() {
            return Vec::new();
        }
        let mut padded = self.leftover.clone();
        padded.resize(FRAME, 0.0);
        self.leftover.clear();
        self.process(&padded)
    }
}

impl Default for Rnnoise {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn silence_stays_quiet() {
        let mut denoise = Rnnoise::new();
        let out = denoise.process(&[0.0; FRAME * 2]);
        assert!(out.iter().all(|s| s.abs() < 0.05));
    }

    #[test]
    fn short_buffer_does_not_panic() {
        let mut denoise = Rnnoise::new();
        let out = denoise.process(&[0.01, -0.02, 0.0]);
        assert!(out.is_empty() || out.iter().all(|s| s.is_finite()));
        let flushed = denoise.flush();
        assert!(flushed.iter().all(|s| s.is_finite()));
    }

    #[test]
    fn reuse_matches_fresh_process() {
        let mut first = Rnnoise::new();
        let mut second = Rnnoise::new();
        let input = vec![0.01f32; FRAME * 3];
        let a = first.process(&input);
        let b = second.process_into(&input).to_vec();
        assert_eq!(a.len(), b.len());
        for (left, right) in a.iter().zip(b.iter()) {
            assert!((left - right).abs() < 1e-6);
        }
    }
}
