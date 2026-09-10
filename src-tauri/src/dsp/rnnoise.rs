use nnnoiseless::DenoiseState;

pub const FRAME: usize = DenoiseState::FRAME_SIZE;

pub struct Rnnoise {
    state: Box<DenoiseState<'static>>,
    leftover: Vec<f32>,
}

impl Rnnoise {
    pub fn new() -> Self {
        Self {
            state: DenoiseState::new(),
            leftover: Vec::with_capacity(FRAME),
        }
    }

    pub fn reset(&mut self) {
        self.state = DenoiseState::new();
        self.leftover.clear();
    }

    pub fn process(&mut self, input: &[f32]) -> Vec<f32> {
        let mut work = Vec::with_capacity(self.leftover.len() + input.len());
        work.extend_from_slice(&self.leftover);
        work.extend_from_slice(input);
        self.leftover.clear();
        let mut output = Vec::with_capacity(work.len());
        let mut frame_in = [0.0f32; FRAME];
        let mut frame_out = [0.0f32; FRAME];
        let mut index = 0;
        while index + FRAME <= work.len() {
            for i in 0..FRAME {
                frame_in[i] = work[index + i] * 32768.0;
            }
            let _ = self.state.process_frame(&mut frame_out, &frame_in);
            for sample in frame_out {
                output.push((sample / 32768.0).clamp(-1.0, 1.0));
            }
            index += FRAME;
        }
        self.leftover.extend_from_slice(&work[index..]);
        output
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
}
