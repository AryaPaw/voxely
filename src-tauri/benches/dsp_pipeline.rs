use criterion::{criterion_group, criterion_main, Criterion};
use voxely_lib::dsp::pipeline::{DspPipeline, DspPreset};

fn bench_dsp(c: &mut Criterion) {
    let sine: Vec<f32> = (0..48_000)
        .map(|i| (i as f32 * 440.0 * 2.0 * std::f32::consts::PI / 48_000.0).sin() * 0.2)
        .collect();
    c.bench_function("dsp_stt_1s", |b| {
        b.iter(|| {
            let mut pipeline = DspPipeline::new(DspPreset::stt_optimized()).unwrap();
            let _ = pipeline.process(sine.clone()).unwrap();
        });
    });
}

criterion_group!(benches, bench_dsp);
criterion_main!(benches);
