use crate::dsp::metrics::SAMPLE_RATE;
use crate::error::AppError;
use rubato::{
    Resampler, SincFixedIn, SincInterpolationParameters, SincInterpolationType, WindowFunction,
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
        f_cutoff: 0.95,
        interpolation: SincInterpolationType::Linear,
        oversampling_factor: 128,
        window: WindowFunction::BlackmanHarris2,
    };
    let chunk = 1024.min(input.len().max(8));
    let Ok(mut resampler) = SincFixedIn::<f32>::new(ratio, 2.0, params, chunk, 1) else {
        return resample_linear(input, input_rate, output_rate);
    };
    let mut output = Vec::with_capacity(((input.len() as f64) * ratio).ceil() as usize + 16);
    let mut offset = 0;
    while offset < input.len() {
        let needed = resampler.input_frames_next();
        if offset + needed <= input.len() {
            let chunk_in = &input[offset..offset + needed];
            if let Ok(waves) = resampler.process(&[chunk_in], None) {
                output.extend_from_slice(&waves[0]);
            }
            offset += needed;
        } else {
            let rest = &input[offset..];
            if let Ok(waves) = resampler.process_partial(Some(&[rest]), None) {
                output.extend_from_slice(&waves[0]);
            }
            break;
        }
    }
    if let Ok(waves) = resampler.process_partial::<&[f32]>(None, None) {
        output.extend_from_slice(&waves[0]);
    }
    let expected = ((input.len() as f64) * ratio).round().max(1.0) as usize;
    output.truncate(expected);
    output
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
