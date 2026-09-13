# Performance

Machine: AryaPaw-PC, Windows 11 Pro, AMD Ryzen 9 3900 12-core, ~128 GB RAM.

Daily driver is the debug binary (`bunx tauri build -d --no-bundle` → `src-tauri/target/debug/voxely.exe`). Release Criterion is an upper bound, not that binary.

Dev DSP crates (`nnnoiseless`, `rubato`, `rustfft`, `realfft`, `easyfft`) use `opt-level = 3` in `[profile.dev.package.*]`.

## Commands

Debug Instant harness (median of 10 after one warm-up):

```
cargo test --locked --lib -- --ignored dictation_timing --nocapture
```

Release Instant harness:

```
cargo test --release --locked --lib -- --ignored dictation_timing --nocapture
```

Release Criterion:

```
cargo bench --locked --bench dsp_pipeline
```

## Local dictation bundle

Stages: DSP (Quality/Fast, no `metrics()`), sinc 48 kHz → 16 kHz, processed + STT WAV writes. No HTTP.

| Profile | Case | dsp | down | wav | bundle |
| --- | --- | --- | --- | --- | --- |
| debug | Quality 1 s | 12 ms | 6 ms | 8 ms | 28 ms |
| debug | Quality 8 s | 98 ms | 27 ms | 42 ms | 168 ms |
| debug | Fast 8 s | 25 ms | 30 ms | 46 ms | 104 ms |
| release | Quality 1 s | 5 ms | 1 ms | 4 ms | 11 ms |
| release | Quality 8 s | 48 ms | 6 ms | 7 ms | 62 ms |
| release | Fast 8 s | 4 ms | 7 ms | 8 ms | 20 ms |

Debug Quality scale 8 s / 1 s: dsp 8.17, bundle 6.00. DSP is roughly linear with length. Bundle is less linear because WAV I/O has a fixed cost.

## Resample only (48 kHz → 16 kHz)

| Profile | Length | sinc 128/128 | rubato FftFixedIn |
| --- | --- | --- | --- |
| debug | 1 s | 7 ms | 21 ms |
| debug | 8 s | 33 ms | 162 ms |
| release | 1 s | 1 ms | 0 ms (sub-ms) |
| release | 8 s | 8 ms | 3 ms |

Gate to replace production sinc: ≥20% faster full local bundle **or** ≥50 ms saved on 10 s in debug, and release bundle not >10% slower. Fft is slower in debug (the daily driver) and only ~5 ms faster on 8 s in release (~8% of the Quality bundle). Production stays on sinc. Linear and naive decimation stay forbidden.

WAV write of 1 s PCM16: 6 ms debug, 2 ms release.

## Runtime spans

Dictation logs (no audio, transcript, or API key):

- `capture_finalize_ms`
- `raw_read_ms` (0 on the in-memory dictation path)
- `dsp_ms`
- `metrics_ms` (0 on dictation)
- `processed_write_ms`
- `downsample_ms`
- `stt_write_ms`
- `stt_http_ms`

HUD: "Обработка" is capture finalize + DSP + processed WAV. "Расшифровка" is downsample + STT WAV + OpenRouter HTTP.
