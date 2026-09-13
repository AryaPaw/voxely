use criterion::{criterion_group, criterion_main, Criterion};
use voxely_lib::dsp::pipeline::{samples_for_stt, DspPipeline, DspPreset};

fn sine(seconds: usize) -> Vec<f32> {
    let n = 48_000 * seconds;
    (0..n)
        .map(|i| (i as f32 * 440.0 * 2.0 * std::f32::consts::PI / 48_000.0).sin() * 0.2)
        .collect()
}

fn bench_dsp(c: &mut Criterion) {
    let one = sine(1);
    c.bench_function("dsp_stt_1s", |b| {
        b.iter(|| {
            let mut pipeline = DspPipeline::new(DspPreset::stt_optimized()).unwrap();
            let _ = pipeline.process_audio(one.clone()).unwrap();
        });
    });
    c.bench_function("resample_sinc_1s", |b| {
        b.iter(|| samples_for_stt(&one, 48_000));
    });
}

criterion_group!(benches, bench_dsp);
criterion_main!(benches);
