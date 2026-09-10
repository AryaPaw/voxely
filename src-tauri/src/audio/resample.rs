use crate::dsp::metrics::SAMPLE_RATE;
use crate::error::AppError;

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
