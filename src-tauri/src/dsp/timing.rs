use std::time::{Duration, Instant};

use tempfile::tempdir;

use crate::audio::capture::write_pcm16_wav;
use crate::audio::resample::{resample_fft, resample_sinc};
use crate::dsp::metrics::{SAMPLE_RATE, STT_SAMPLE_RATE};
use crate::dsp::pipeline::{prepare_listen_audio, samples_for_stt, DspPreset};

#[derive(Debug, Clone, Copy)]
pub struct StageMs {
    pub dsp: u128,
    pub downsample: u128,
    pub wav: u128,
    pub bundle: u128,
}

pub fn tone(seconds: f32, hz: f32, rate: u32, amp: f32) -> Vec<f32> {
    let n = (seconds * rate as f32).round() as usize;
    (0..n)
        .map(|i| (i as f32 * hz * 2.0 * std::f32::consts::PI / rate as f32).sin() * amp)
        .collect()
}

pub fn median_ms(times: &mut [u128]) -> u128 {
    times.sort_unstable();
    times[times.len() / 2]
}

pub fn p95_ms(times: &mut [u128]) -> u128 {
    if times.is_empty() {
        return 0;
    }
    times.sort_unstable();
    let idx = ((times.len() as f64 - 1.0) * 0.95).ceil() as usize;
    times[idx.min(times.len() - 1)]
}

pub fn measure_dsp(preset: DspPreset, samples: &[f32], runs: usize) -> u128 {
    let mut times = Vec::with_capacity(runs);
    let _ = prepare_listen_audio(preset.clone(), samples.to_vec());
    for _ in 0..runs {
        let start = Instant::now();
        let _ = prepare_listen_audio(preset.clone(), samples.to_vec()).expect("dsp");
        times.push(start.elapsed().as_millis());
    }
    median_ms(&mut times)
}

pub fn measure_resample(samples: &[f32], fft: bool, runs: usize) -> u128 {
    let mut times = Vec::with_capacity(runs);
    if fft {
        let _ = resample_fft(samples, SAMPLE_RATE, STT_SAMPLE_RATE);
    } else {
        let _ = resample_sinc(samples, SAMPLE_RATE, STT_SAMPLE_RATE);
    }
    for _ in 0..runs {
        let start = Instant::now();
        if fft {
            let _ = resample_fft(samples, SAMPLE_RATE, STT_SAMPLE_RATE);
        } else {
            let _ = resample_sinc(samples, SAMPLE_RATE, STT_SAMPLE_RATE);
        }
        times.push(start.elapsed().as_millis());
    }
    median_ms(&mut times)
}

pub fn measure_wav_roundtrip(samples: &[f32], runs: usize) -> u128 {
    let dir = tempdir().expect("tmpdir");
    let path = dir.path().join("t.wav");
    let mut times = Vec::with_capacity(runs);
    write_pcm16_wav(&path, SAMPLE_RATE, samples).expect("wav");
    for _ in 0..runs {
        let start = Instant::now();
        write_pcm16_wav(&path, SAMPLE_RATE, samples).expect("wav");
        times.push(start.elapsed().as_millis());
    }
    median_ms(&mut times)
}

pub fn measure_bundle(preset: DspPreset, samples: &[f32], runs: usize) -> StageMs {
    let dir = tempdir().expect("tmpdir");
    let processed = dir.path().join("p.wav");
    let stt = dir.path().join("s.wav");
    let mut dsp = Vec::with_capacity(runs);
    let mut down = Vec::with_capacity(runs);
    let mut wav = Vec::with_capacity(runs);
    let mut bundle = Vec::with_capacity(runs);
    let _ = prepare_listen_audio(preset.clone(), samples.to_vec());
    for _ in 0..runs {
        let bundle_start = Instant::now();
        let dsp_start = Instant::now();
        let out = prepare_listen_audio(preset.clone(), samples.to_vec()).expect("dsp");
        dsp.push(dsp_start.elapsed().as_millis());
        let down_start = Instant::now();
        let stt_pcm = samples_for_stt(&out, SAMPLE_RATE);
        down.push(down_start.elapsed().as_millis());
        let wav_start = Instant::now();
        write_pcm16_wav(&processed, SAMPLE_RATE, &out).expect("p");
        write_pcm16_wav(&stt, STT_SAMPLE_RATE, &stt_pcm).expect("s");
        wav.push(wav_start.elapsed().as_millis());
        bundle.push(bundle_start.elapsed().as_millis());
    }
    StageMs {
        dsp: median_ms(&mut dsp),
        downsample: median_ms(&mut down),
        wav: median_ms(&mut wav),
        bundle: median_ms(&mut bundle),
    }
}

pub fn format_report(label: &str, stages: StageMs) -> String {
    format!(
        "{label} dsp={}ms down={}ms wav={}ms bundle={}ms",
        stages.dsp, stages.downsample, stages.wav, stages.bundle
    )
}

pub fn wait_ms(ms: u128) -> Duration {
    Duration::from_millis(ms as u64)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn median_and_p95_use_sorted_samples() {
        let mut samples = [10u128, 1, 2, 3, 4];
        assert_eq!(median_ms(&mut samples), 3);
        let mut samples = [1u128, 2, 3, 4, 100];
        assert_eq!(p95_ms(&mut samples), 100);
        assert_eq!(p95_ms(&mut []), 0);
    }

    #[test]
    #[ignore]
    fn dictation_timing() {
        let one = tone(1.0, 440.0, SAMPLE_RATE, 0.2);
        let eight = tone(8.0, 440.0, SAMPLE_RATE, 0.2);
        let quality = DspPreset::stt_optimized();
        let fast = DspPreset::stt_fast();
        let q1 = measure_bundle(quality.clone(), &one, 10);
        let q8 = measure_bundle(quality, &eight, 10);
        let f8 = measure_bundle(fast, &eight, 10);
        let sinc = measure_resample(&one, false, 10);
        let fft = measure_resample(&one, true, 10);
        let sinc8 = measure_resample(&eight, false, 10);
        let fft8 = measure_resample(&eight, true, 10);
        let wav = measure_wav_roundtrip(&one, 10);
        eprintln!(
            "profile={}",
            if cfg!(debug_assertions) {
                "dev"
            } else {
                "release"
            }
        );
        eprintln!("{}", format_report("quality_1s", q1));
        eprintln!("{}", format_report("quality_8s", q8));
        eprintln!("{}", format_report("fast_8s", f8));
        eprintln!("resample_sinc_1s={sinc}ms resample_fft_1s={fft}ms wav_1s={wav}ms");
        eprintln!("resample_sinc_8s={sinc8}ms resample_fft_8s={fft8}ms");
        eprintln!(
            "scale_8s/1s dsp={:.2} bundle={:.2}",
            q8.dsp as f64 / q1.dsp.max(1) as f64,
            q8.bundle as f64 / q1.bundle.max(1) as f64
        );
    }
}
