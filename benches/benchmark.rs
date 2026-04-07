use criterion::{criterion_group, criterion_main, Criterion};

fn generate_clicks(bpm: f64, sample_rate: u32, duration_secs: f64) -> Vec<f32> {
    let sr = sample_rate as f64;
    let total = (duration_secs * sr) as usize;
    let period = (60.0 / bpm * sr) as usize;
    let click_len = (sr * 0.005) as usize;

    let mut samples = vec![0.0f32; total];
    let mut pos = 0;
    while pos < total {
        for i in 0..click_len.min(total - pos) {
            let t = i as f32 / click_len as f32;
            samples[pos + i] =
                (t * std::f32::consts::PI * 2.0 * 100.0 / sr as f32).sin() * (1.0 - t) * 0.8;
        }
        pos += period;
    }
    samples
}

fn bench_detect(c: &mut Criterion) {
    let samples_5s = generate_clicks(128.0, 44100, 5.0);
    let samples_30s = generate_clicks(128.0, 44100, 30.0);
    let samples_180s = generate_clicks(128.0, 44100, 180.0);

    c.bench_function("detect_5s_44100", |b| {
        b.iter(|| open_bpm::detect(&samples_5s, 44100))
    });

    c.bench_function("detect_30s_44100", |b| {
        b.iter(|| open_bpm::detect(&samples_30s, 44100))
    });

    c.bench_function("detect_180s_44100", |b| {
        b.iter(|| open_bpm::detect(&samples_180s, 44100))
    });
}

criterion_group!(benches, bench_detect);
criterion_main!(benches);
