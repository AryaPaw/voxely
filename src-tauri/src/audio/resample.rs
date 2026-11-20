use crate::dsp::metrics::SAMPLE_RATE;
use crate::error::AppError;
use rubato::audioadapter_buffers::direct::InterleavedSlice;
use rubato::{
    Async, Fft, FixedAsync, FixedSync, Indexing, Resampler, SincInterpolationParameters,
    SincInterpolationType, WindowFunction,
};

pub fn to_mono(frames: &[f32], channels: usize) -> Vec<f32> {
    if channels <= 1 {
        return frames.to_vec();
    }
    frames
        .chunks(channels)
        .map(|frame| frame.iter().sum::<f32>() / channels as f32)
        .collect()
}

pub fn i16_to_f32(samples: &[i16]) -> Vec<f32> {
    samples.iter().map(|s| *s as f32 / 32768.0).collect()
}

pub fn f32_to_i16(samples: &[f32]) -> Vec<i16> {
    samples
        .iter()
        .map(|s| (s.clamp(-1.0, 1.0) * 32767.0).round() as i16)
        .collect()
}

pub fn resample_to_48k(input: &[f32], input_rate: u32) -> Result<Vec<f32>, AppError> {
    Ok(resample_linear(input, input_rate, SAMPLE_RATE))
}

/// Stateful band-limited conversion for the WAV writer. Fixed internal chunks
/// make the result independent of callback sizes; finish drains filter latency.
pub struct StreamResampler {
    input_rate: u32,
    output_rate: u32,
    input_frames: u64,
    output_frames: usize,
    engine: Option<Async<f32>>,
    pending: Vec<f32>,
    delay_remaining: usize,
    finished: bool,
}

impl StreamResampler {
    pub fn new(input_rate: u32, output_rate: u32) -> Result<Self, AppError> {
        if input_rate == 0 || output_rate == 0 {
            return Err(AppError::AudioProcessingFailed(
                "invalid streaming sample rate".into(),
            ));
        }
        let engine = if input_rate == output_rate {
            None
        } else {
            Some(
                Async::<f32>::new_sinc(
                    f64::from(output_rate) / f64::from(input_rate),
                    2.0,
                    &SincInterpolationParameters {
                        sinc_len: 128,
                        f_cutoff: Some(0.95),
                        interpolation: SincInterpolationType::Linear,
                        oversampling_factor: 128,
                        window: WindowFunction::BlackmanHarris2,
                    },
                    512,
                    1,
                    FixedAsync::Input,
                )
                .map_err(|error| AppError::AudioProcessingFailed(error.to_string()))?,
            )
        };
        let delay_remaining = engine.as_ref().map_or(0, Resampler::output_delay);
        Ok(Self {
            input_rate,
            output_rate,
            input_frames: 0,
            output_frames: 0,
            engine,
            pending: Vec::new(),
            delay_remaining,
            finished: false,
        })
    }

    pub fn push(&mut self, input: &[f32]) -> Result<Vec<f32>, AppError> {
        if self.finished {
            return Err(AppError::AudioProcessingFailed(
                "resampler already finished".into(),
            ));
        }
        self.input_frames = self.input_frames.saturating_add(input.len() as u64);
        let Some(engine) = self.engine.as_mut() else {
            self.output_frames += input.len();
            return Ok(input.to_vec());
        };
        self.pending.extend_from_slice(input);
        let mut output = Vec::new();
        let mut consumed = 0;
        while self.pending.len() - consumed >= engine.input_frames_next() {
            let needed = engine.input_frames_next();
            let waves = process_mono(engine, &self.pending[consumed..consumed + needed])?;
            consumed += needed;
            append_aligned_output(&waves, &mut self.delay_remaining, &mut output);
        }
        self.pending.drain(..consumed);
        self.output_frames += output.len();
        Ok(output)
    }

    pub fn finish(&mut self) -> Result<Vec<f32>, AppError> {
        if self.finished {
            return Ok(Vec::new());
        }
        self.finished = true;
        if self.input_frames == 0 || self.engine.is_none() {
            return Ok(Vec::new());
        }
        let expected = (self.input_frames as f64 * f64::from(self.output_rate)
            / f64::from(self.input_rate))
        .round()
        .max(1.0) as usize;
        let remaining = expected.saturating_sub(self.output_frames);
        let engine = self.engine.as_mut().expect("non-identity resampler");
        let mut output = Vec::new();
        if !self.pending.is_empty() {
            let waves = process_mono(engine, &self.pending)?;
            self.pending.clear();
            append_aligned_output(&waves, &mut self.delay_remaining, &mut output);
        }
        while output.len() < remaining {
            let waves = process_mono(engine, &[])?;
            if waves.is_empty() {
                return Err(AppError::AudioProcessingFailed(
                    "resampler failed to flush".into(),
                ));
            }
            append_aligned_output(&waves, &mut self.delay_remaining, &mut output);
        }
        output.truncate(remaining);
        self.output_frames = expected;
        Ok(output)
    }
}

fn process_mono(engine: &mut impl Resampler<f32>, input: &[f32]) -> Result<Vec<f32>, AppError> {
    let buffer = InterleavedSlice::new(input, 1, input.len())
        .map_err(|error| AppError::AudioProcessingFailed(error.to_string()))?;
    let partial = (input.len() < engine.input_frames_next())
        .then(|| Indexing::new().partial_len(input.len()));
    engine
        .process(&buffer, partial.as_ref())
        .map(|output| output.take_data())
        .map_err(|error| AppError::AudioProcessingFailed(error.to_string()))
}

fn append_aligned_output(samples: &[f32], delay: &mut usize, output: &mut Vec<f32>) {
    let discard = (*delay).min(samples.len());
    *delay -= discard;
    output.extend_from_slice(&samples[discard..]);
}

pub fn resample_sinc(input: &[f32], input_rate: u32, output_rate: u32) -> Vec<f32> {
    if input.is_empty() {
        return Vec::new();
    }
    if input_rate == 0 || output_rate == 0 || input_rate == output_rate {
        return input.to_vec();
    }
    let ratio = f64::from(output_rate) / f64::from(input_rate);
    let params = SincInterpolationParameters {
        sinc_len: 128,
        f_cutoff: Some(0.95),
        interpolation: SincInterpolationType::Linear,
        oversampling_factor: 128,
        window: WindowFunction::BlackmanHarris2,
    };
    let chunk = 1024.min(input.len().max(8));
    let Ok(mut resampler) =
        Async::<f32>::new_sinc(ratio, 2.0, &params, chunk, 1, FixedAsync::Input)
    else {
        return resample_linear(input, input_rate, output_rate);
    };
    let Ok(buffer) = InterleavedSlice::new(input, 1, input.len()) else {
        return resample_linear(input, input_rate, output_rate);
    };
    let Ok(resampled) = resampler.process_all(&buffer, input.len(), None) else {
        return resample_linear(input, input_rate, output_rate);
    };
    let mut output = resampled.take_data();
    let expected = ((input.len() as f64) * ratio).round().max(1.0) as usize;
    output.truncate(expected);
    output
}

pub fn resample_fft(input: &[f32], input_rate: u32, output_rate: u32) -> Vec<f32> {
    if input.is_empty() {
        return Vec::new();
    }
    if input_rate == 0 || output_rate == 0 || input_rate == output_rate {
        return input.to_vec();
    }
    let chunk = 1024.min(input.len().max(8));
    let Ok(mut resampler) = Fft::<f32>::new_custom(
        input_rate as usize,
        output_rate as usize,
        chunk,
        2,
        1,
        WindowFunction::BlackmanHarris2,
        FixedSync::Input,
    ) else {
        return resample_sinc(input, input_rate, output_rate);
    };
    let delay = resampler.output_delay();
    let ratio = f64::from(output_rate) / f64::from(input_rate);
    let mut output =
        Vec::with_capacity(((input.len() as f64) * ratio).ceil() as usize + delay + 16);
    let mut offset = 0;
    while offset < input.len() {
        let needed = resampler.input_frames_next();
        if offset + needed <= input.len() {
            let chunk_in = &input[offset..offset + needed];
            if let Ok(waves) = process_mono(&mut resampler, chunk_in) {
                output.extend_from_slice(&waves);
            }
            offset += needed;
        } else {
            let rest = &input[offset..];
            if let Ok(waves) = process_mono(&mut resampler, rest) {
                output.extend_from_slice(&waves);
            }
            break;
        }
    }
    if let Ok(waves) = process_mono(&mut resampler, &[]) {
        output.extend_from_slice(&waves);
    }
    let trimmed = if output.len() > delay {
        output.split_off(delay)
    } else {
        output
    };
    let expected = ((input.len() as f64) * ratio).round().max(1.0) as usize;
    let mut trimmed = trimmed;
    trimmed.truncate(expected);
    if trimmed.len() < expected {
        trimmed.resize(expected, 0.0);
    }
    trimmed
}

pub fn resample_linear(input: &[f32], input_rate: u32, output_rate: u32) -> Vec<f32> {
    if input.is_empty() {
        return Vec::new();
    }
    if input_rate == output_rate || input_rate == 0 || output_rate == 0 {
        return input.to_vec();
    }
    let ratio = input_rate as f64 / output_rate as f64;
    let out_len = ((input.len() as f64) / ratio).round().max(1.0) as usize;
    let last = (input.len() - 1) as f64;
    let mut output = Vec::with_capacity(out_len);
    for i in 0..out_len {
        let pos = (i as f64 * ratio).min(last);
        let index = pos.floor() as usize;
        let frac = (pos - index as f64) as f32;
        let next = (index + 1).min(input.len() - 1);
        output.push(input[index] * (1.0 - frac) + input[next] * frac);
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stereo_to_mono() {
        let mono = to_mono(&[0.2, 0.4, 0.0, 1.0], 2);
        assert!((mono[0] - 0.3).abs() < 1e-6);
    }

    #[test]
    fn identity_resample() {
        let samples = vec![0.1, 0.2, 0.3];
        assert_eq!(resample_to_48k(&samples, 48_000).unwrap(), samples);
    }

    fn stream_resample(input: &[f32], rate: u32, chunk: usize) -> Vec<f32> {
        let mut resampler = StreamResampler::new(rate, SAMPLE_RATE).unwrap();
        let mut output = Vec::new();
        for frame in input.chunks(chunk) {
            output.extend(resampler.push(frame).unwrap());
        }
        output.extend(resampler.finish().unwrap());
        assert!(resampler.finish().unwrap().is_empty());
        assert!(resampler.push(&[0.0]).is_err());
        output
    }

    #[test]
    fn streaming_resample_is_independent_of_chunk_boundaries() {
        for input_rate in [8_000, 16_000, 44_100, 48_000, 96_000, 192_000] {
            let input = tone(input_rate as usize / 10 + 31, 440.0, input_rate);
            let expected = stream_resample(&input, input_rate, input.len());
            let expected_len = (input.len() as f64 * f64::from(SAMPLE_RATE) / f64::from(input_rate))
                .round()
                .max(1.0) as usize;
            assert_eq!(expected.len(), expected_len);
            for chunk in [1, 7, 113, 512, 770, 771, 1031] {
                let actual = stream_resample(&input, input_rate, chunk);
                assert_eq!(actual, expected, "rate={input_rate}, chunk={chunk}");
            }
        }
    }

    #[test]
    fn streaming_resample_matches_one_shot_for_short_inputs() {
        for input_rate in [44_100, 96_000, 192_000] {
            for len in 1..8 {
                let input: Vec<_> = (0..len).map(|i| (i as f32 + 1.0) / 8.0).collect();
                let expected = stream_resample(&input, input_rate, input.len());
                let actual = stream_resample(&input, input_rate, 1);
                assert_eq!(actual, expected, "rate={input_rate}, frames={len}");
                assert!(actual.iter().all(|sample| sample.is_finite()));
                assert!(!actual.is_empty());
            }
        }
        assert!(StreamResampler::new(0, SAMPLE_RATE).is_err());
        assert!(StreamResampler::new(SAMPLE_RATE, 0).is_err());
        assert!(StreamResampler::new(96_000, SAMPLE_RATE)
            .unwrap()
            .finish()
            .unwrap()
            .is_empty());
    }

    #[test]
    fn streaming_capture_rejects_ultrasonic_alias_and_preserves_voice_tail() {
        for (rate, stop_frequency) in [(96_000, 42_000.0), (192_000, 90_000.0)] {
            let pass = stream_resample(&tone(rate as usize, 1_000.0, rate), rate, 771);
            let stop = stream_resample(&tone(rate as usize, stop_frequency, rate), rate, 771);
            let pass_db = 10.0 * goertzel_power(&pass[512..], SAMPLE_RATE, 1_000.0).log10();
            let alias_db = 10.0 * goertzel_power(&stop[512..], SAMPLE_RATE, 6_000.0).log10();
            assert!(
                alias_db <= pass_db - 40.0,
                "rate={rate}, alias={alias_db}, pass={pass_db}"
            );
            let mut input = vec![0.0; rate as usize / 10];
            input.extend(tone(rate as usize / 20, 1_000.0, rate));
            let output = stream_resample(&input, rate, 770);
            let tail = &output[output.len() - 480..];
            let power = tail
                .iter()
                .map(|sample| f64::from(*sample).powi(2))
                .sum::<f64>()
                / tail.len() as f64;
            assert!(power.sqrt() > 0.1, "voice tail missing at {rate} Hz");
        }
    }

    fn goertzel_power(samples: &[f32], rate: u32, freq: f32) -> f32 {
        let n = samples.len() as f32;
        if n < 8.0 {
            return 1e-20;
        }
        let k = (n * freq / rate as f32).round();
        let w = 2.0 * std::f32::consts::PI * k / n;
        let coeff = 2.0 * w.cos();
        let mut s1 = 0.0f32;
        let mut s2 = 0.0f32;
        for &x in samples {
            let s0 = x + coeff * s1 - s2;
            s2 = s1;
            s1 = s0;
        }
        (s1 * s1 + s2 * s2 - coeff * s1 * s2).max(1e-20) / n
    }

    fn tone(len: usize, hz: f32, rate: u32) -> Vec<f32> {
        (0..len)
            .map(|i| (i as f32 * hz * 2.0 * std::f32::consts::PI / rate as f32).sin() * 0.5)
            .collect()
    }

    #[test]
    fn sinc_48k_to_16k_differs_from_linear() {
        let input: Vec<f32> = (0..4800)
            .map(|i| (i as f32 * 440.0 * 2.0 * std::f32::consts::PI / 48_000.0).sin() * 0.2)
            .collect();
        let sinc = resample_sinc(&input, 48_000, 16_000);
        let linear = resample_linear(&input, 48_000, 16_000);
        assert!((sinc.len() as i32 - 1600).abs() <= 2);
        assert_ne!(sinc, linear);
        assert_ne!(sinc, linear);
        assert_eq!(resample_sinc(&input, 16_000, 16_000), input);
    }

    #[test]
    fn sinc_preserves_signal_at_the_end_of_the_clip() {
        let mut input = vec![0.0; 4800];
        input[4512..].fill(0.25);
        let output = resample_sinc(&input, 48_000, 16_000);
        assert_eq!(output.len(), 1600);
        let tail = &output[1504..];
        assert!(tail.iter().filter(|&&sample| sample > 0.20).count() >= 90);
    }

    #[test]
    fn fft_and_sinc_empty_and_short_are_finite() {
        assert!(resample_fft(&[], 48_000, 16_000).is_empty());
        assert!(resample_sinc(&[], 48_000, 16_000).is_empty());
        let short = vec![0.1, -0.1, 0.2];
        let fft = resample_fft(&short, 48_000, 16_000);
        let sinc = resample_sinc(&short, 48_000, 16_000);
        assert!(fft.iter().all(|s| s.is_finite()));
        assert!(sinc.iter().all(|s| s.is_finite()));
        assert_eq!(resample_fft(&short, 16_000, 16_000), short);
    }

    #[test]
    fn fft_48k_to_16k_length_is_about_one_third() {
        let input = tone(48_000, 1000.0, 48_000);
        let out = resample_fft(&input, 48_000, 16_000);
        assert!(out.iter().all(|s| s.is_finite()));
        assert!((out.len() as i32 - 16_000).abs() <= 2);
    }

    #[test]
    fn fft_1khz_passband_matches_sinc() {
        let input = tone(48_000, 1000.0, 48_000);
        let sinc = resample_sinc(&input, 48_000, 16_000);
        let fft = resample_fft(&input, 48_000, 16_000);
        let sinc_db = 10.0 * goertzel_power(&sinc, 16_000, 1000.0).log10();
        let fft_db = 10.0 * goertzel_power(&fft, 16_000, 1000.0).log10();
        assert!((fft_db - sinc_db).abs() <= 1.5);
    }

    #[test]
    fn downsample_9khz_is_below_1khz_passband() {
        let pass = resample_sinc(&tone(48_000, 1000.0, 48_000), 48_000, 16_000);
        let stop = resample_sinc(&tone(48_000, 9000.0, 48_000), 48_000, 16_000);
        let pass_db = 10.0 * goertzel_power(&pass, 16_000, 1000.0).log10();
        let alias_db = 10.0 * goertzel_power(&stop, 16_000, 7000.0).log10();
        assert!(alias_db <= pass_db - 25.0);
        let fft_pass = resample_fft(&tone(48_000, 1000.0, 48_000), 48_000, 16_000);
        let fft_stop = resample_fft(&tone(48_000, 9000.0, 48_000), 48_000, 16_000);
        let fft_pass_db = 10.0 * goertzel_power(&fft_pass, 16_000, 1000.0).log10();
        let fft_alias_db = 10.0 * goertzel_power(&fft_stop, 16_000, 7000.0).log10();
        assert!(fft_alias_db <= fft_pass_db - 25.0);
    }

    #[test]
    fn downsample_48k_to_16k_is_about_one_third() {
        let input = vec![0.1; 4800];
        let out = resample_linear(&input, 48_000, 16_000);
        assert!((out.len() as i32 - 1600).abs() <= 2);
    }

    use proptest::prelude::*;

    proptest::proptest! {
        #[test]
        fn resample_no_panic(len in 0usize..400) {
            let input: Vec<f32> = (0..len).map(|i| (i as f32).sin() * 0.2).collect();
            let out = resample_to_48k(&input, 44_100).unwrap();
            prop_assert!(out.iter().all(|s| s.is_finite()));
        }
    }
}
