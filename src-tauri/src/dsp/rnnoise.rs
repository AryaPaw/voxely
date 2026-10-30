use nnnoiseless::DenoiseState;

pub const FRAME: usize = DenoiseState::FRAME_SIZE;

pub struct Rnnoise {
    state: Box<DenoiseState<'static>>,
    leftover: Vec<f32>,
    work: Vec<f32>,
    output: Vec<f32>,
    frame_in: [f32; FRAME],
    frame_out: [f32; FRAME],
    discard_initial_frame: bool,
    pending_samples: usize,
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
            discard_initial_frame: true,
            pending_samples: 0,
        }
    }

    pub fn reset(&mut self) {
        self.state = DenoiseState::new();
        self.leftover.clear();
        self.work.clear();
        self.output.clear();
        self.discard_initial_frame = true;
        self.pending_samples = 0;
    }

    pub fn process(&mut self, input: &[f32]) -> Vec<f32> {
        self.process_into(input);
        self.output.clone()
    }

    pub fn process_into(&mut self, input: &[f32]) -> &[f32] {
        self.pending_samples += input.len();
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
            self.process_current_frame();
            index += FRAME;
        }
        self.leftover.extend_from_slice(&self.work[index..]);
        self.pending_samples -= self.output.len();
        &self.output
    }

    fn process_current_frame(&mut self) {
        let _ = self
            .state
            .process_frame(&mut self.frame_out, &self.frame_in);
        // The overlap-add synthesizer emits the preceding 480-sample frame.
        // Discard startup output, then drain one zero frame when finishing.
        if self.discard_initial_frame {
            self.discard_initial_frame = false;
        } else {
            self.output.extend(
                self.frame_out
                    .iter()
                    .map(|sample| (sample / 32768.0).clamp(-1.0, 1.0)),
            );
        }
    }

    pub fn flush(&mut self) -> Vec<f32> {
        if self.pending_samples == 0 {
            return Vec::new();
        }
        self.output.clear();
        if !self.leftover.is_empty() {
            self.frame_in.fill(0.0);
            for (slot, sample) in self.frame_in.iter_mut().zip(&self.leftover) {
                *slot = sample * 32768.0;
            }
            self.process_current_frame();
        }
        self.frame_in.fill(0.0);
        self.process_current_frame();
        self.output.truncate(self.pending_samples);
        let output = std::mem::take(&mut self.output);
        self.reset();
        output
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

    fn zero_padded_reference(input: &[f32]) -> Vec<f32> {
        let mut state = DenoiseState::new();
        let mut output = Vec::new();
        for (index, chunk) in input
            .chunks(FRAME)
            .chain(std::iter::once(&[][..]))
            .enumerate()
        {
            let mut frame = [0.0; FRAME];
            for (sample, source) in frame.iter_mut().zip(chunk) {
                *sample = source * 32768.0;
            }
            let mut out = [0.0; FRAME];
            state.process_frame(&mut out, &frame);
            if index > 0 {
                output.extend(out.map(|sample| (sample / 32768.0).clamp(-1.0, 1.0)));
            }
        }
        output.truncate(input.len());
        output
    }

    #[test]
    fn drain_and_alignment_match_zero_padded_reference_for_every_tail_size() {
        for len in [0, 1, FRAME - 1, FRAME, FRAME + 1, FRAME * 2, FRAME * 2 + 37] {
            let input: Vec<f32> = (0..len).map(|i| (i as f32 * 0.13).sin() * 0.2).collect();
            let expected = zero_padded_reference(&input);
            let mut denoise = Rnnoise::new();
            let mut actual = denoise.process(&input);
            actual.extend(denoise.flush());
            assert_eq!(actual.len(), len, "input length {len}");
            assert_eq!(actual, expected, "input length {len}");
            assert!(denoise.flush().is_empty());
        }
    }

    #[test]
    fn chunked_processing_and_reuse_preserve_complete_aligned_signal() {
        let input: Vec<f32> = (0..FRAME * 4 + 41)
            .map(|i| (i as f32 * 0.13).sin() * 0.2)
            .collect();
        let expected = zero_padded_reference(&input);
        let mut denoise = Rnnoise::new();
        for _ in 0..2 {
            let mut actual = Vec::new();
            for chunk in input.chunks(113) {
                actual.extend_from_slice(denoise.process_into(chunk));
            }
            actual.extend(denoise.flush());
            assert_eq!(actual, expected);
        }
        assert!(expected[expected.len() - 41..]
            .iter()
            .any(|s| s.abs() > 0.001));
    }
}
