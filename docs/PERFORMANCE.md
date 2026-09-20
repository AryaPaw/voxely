# Performance

Machine: AryaPaw-PC, Windows 11 Pro, AMD Ryzen 9 3900 12-core, ~128 GB RAM.

Daily driver is the debug binary (`bunx tauri build -d --no-bundle` → `src-tauri/target/debug/voxely.exe`). Release Criterion is an upper bound, not that binary. Dev objects use line tables only and no incremental cache; `target/` is disposable (`scripts/clean-rust-artifacts.ps1`).

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

| Profile | Case        | dsp   | down  | wav   | bundle |
| ------- | ----------- | ----- | ----- | ----- | ------ |
| debug   | Quality 1 s | 12 ms | 6 ms  | 8 ms  | 28 ms  |
| debug   | Quality 8 s | 98 ms | 27 ms | 42 ms | 168 ms |
| debug   | Fast 8 s    | 25 ms | 30 ms | 46 ms | 104 ms |
| release | Quality 1 s | 5 ms  | 1 ms  | 4 ms  | 11 ms  |
| release | Quality 8 s | 48 ms | 6 ms  | 7 ms  | 62 ms  |
| release | Fast 8 s    | 4 ms  | 7 ms  | 8 ms  | 20 ms  |

Debug Quality scale 8 s / 1 s: dsp 8.17, bundle 6.00. DSP is roughly linear with length. Bundle is less linear because WAV I/O has a fixed cost.

## Resample only (48 kHz → 16 kHz)

| Profile | Length | sinc 128/128 | rubato FftFixedIn |
| ------- | ------ | ------------ | ----------------- |
| debug   | 1 s    | 7 ms         | 21 ms             |
| debug   | 8 s    | 33 ms        | 162 ms            |
| release | 1 s    | 1 ms         | 0 ms (sub-ms)     |
| release | 8 s    | 8 ms         | 3 ms              |

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

Insert logs (debug, no transcript):

- `insert_ms`
- UTF-16 `units`
- `batches`
- window `class`
- `focus_attempts`
- last `SendInput` `sent`

Unicode insert uses bounded batches of 256-1024 planned units. History insert runs on a worker thread.

HUD: "Обработка" is capture finalize + DSP + processed WAV. "Расшифровка" is downsample + STT WAV + OpenRouter HTTP.

## VPN transport campaign (2026-09-15)

Debug exe: `src-tauri/target/debug/voxely.exe`, PID 74368, path matches, `CreationDate` 18:11:30 after `LastWriteTime` 18:10:39. Main HWND present, title `Voxely (local)`.

Public probes to `GET https://openrouter.ai/api/v1/models` (no API key, no audio):

- 20 independent curl processes (new TLS each time): 13 x HTTP 200 in 0.33-0.54 s; 7 x Schannel handshake fail at ~5.02 s (`num_connects=1`). Failures are still connect-stage EOF, not HTTP 5xx.
- 20 sequential `fetch` calls in one Bun process (connection reuse): 20/20 HTTP 200. First 259 ms, then 75-105 ms. p50 84 ms, p95 105 ms. No handshake-eof after the first success.

Ten live dictations through Voxely and timed Unicode insert into Notepad/Cursor were not driven from this session (microphone and GUI insert automation). Unit tests cover 503 then 200, pool fingerprint, captured-start HWND, 256-1024 batches, and even SendInput boundaries.

## Process-tree RAM (WebView2)

Do not treat PE/NSIS size, Criterion DSP times, or a single `voxely.exe` Working Set as WebView2 RAM. The unit is the tree: `voxely.exe` plus owned `msedgewebview2.exe` children. Always pair Private Bytes with Working Set.

Debug and release are separate series. Daily driver: `src-tauri/target/debug/voxely.exe`.

Live `%APPDATA%\Voxely` is never the history 0/100/500 fixture. Debug-only override: `VOXELY_DATA_DIR` must be `%APPDATA%\VoxelyPerf` or `VoxelyPerf-*` under `%APPDATA%`. Release builds ignore the variable.

### Commands

```
pwsh -File scripts/perf/Get-VoxelyProcessTree.ps1 -Profile debug -Scenario fresh-tray
pwsh -File scripts/perf/Invoke-VoxelyPerfCampaign.ps1 -Profile debug -Scenario fresh-tray-or-running
```

JSON schema: `docs/perf/process-tree.schema.json` (`voxely-perf/v1`). Raw trials go to `docs/perf/trials/` (gitignored).

`overlay_timing` is elapsed since overlay show, not first paint. Overlay reports `overlay_mark_frame` after React mount / first animation frame.

Wakeups are WPR/ETW only. CPU% and thread count are not wakeups.

Gates (after three campaigns): RAM regression needs Private Bytes plus a confirming metric; latency needs median and p95 plus an absolute floor. Do not fail a single noisy run.

### Baseline status

N=1 local debug snapshots on AryaPaw-PC (not a 10-30 s settled campaign, not median/p95):

- Long-running session that had already created overlay: tree Private Bytes 411,644,480, Working Set 693,575,680, 8 processes / 7 `msedgewebview2`. `commit` in that JSON used `VirtualMemorySize64` (unusable on x64).
- Fresh tray after rebuild (overlay never shown): tree Private Bytes 176,709,632, Working Set 402,804,736, Paged commit 176,709,632, 7 processes / 6 `msedgewebview2`. Host `voxely.exe` Private Bytes 5,623,808.
- 2026-09-15 after Escape/insert/HUD wave, debug exe restarted (~47 s uptime, overlay created hidden, History audio lazy): tree Private Bytes 256,630,784, Working Set 542,715,904, 8 processes / 7 `msedgewebview2`. Host `voxely.exe` Private Bytes 8,658,944. Not a 10-30 s settled campaign. Cursor `node.exe` excluded. Main destroy was not switched on; overlay stays hide-not-destroy.

Do not treat those two rows as a before/after of the same scenario. They differ in overlay lifetime and settle time. History 0/100/500, overlay cold/warm, 8 s / 60 s recording, WPR wakeups, and release series are incomplete. Until three campaigns exist, numbers are informational.

The overlay window loads `overlay.html` / OverlayApp only. Loading `index.html` in that window mounts MainApp; overlay ACL has no `get_runtime_info`, so the HUD becomes a Retry screen. Overlay is created hidden at launch. Relative `./assets` URLs, inline transparent CSS, and `https://ipc.localhost` plus `script-src` eval/inline remain in CSP. HUD logical viewport is 320x72. Overlay is placed on the insert-target monitor when a captured HWND exists, otherwise the cursor monitor. Main is centered on the foreground window's monitor unless that window is Voxely. Debug daily-driver uses the same `%APPDATA%\Voxely` as release; it skips auto-update polling. Local builds add a native window-title suffix (`Voxely (local)` / `Voxely (локальная версия)`). Sandbox is a settings nav page, not the window title. History cards fetch `recording_audio_url` and mount `<audio>` only after Listen.

### Overlay / main lifecycle

Default remains hide (not destroy) for overlay (created hidden at launch) and main-in-tray. Overlay renderer is already in the Spokenly-cheap band (~38 MB). The main renderer is the RAM outlier; destroy/recreate of main is allowed only after a later campaign shows a Private Bytes saving that beats cold-show UX. Vite MPA split is independent of HWND lifecycle. HUD logical viewport is 320x72 (pill 52px plus padding for a short glow).

## Supply chain

Claimed MSRV is `1.89.0` (lock graph: `notify-rust` 4.18). CI compiles on that toolchain (`cargo build --locked --all-targets`), then runs clippy/tests on `stable`.

Removed unused direct Rust crates: `rand`, `sha2`, `directories`, `once_cell`, `bytes`, `base64`. Dropped unused `reqwest` `blocking`, `nnnoiseless` default `bin` features, and `tracing-subscriber` `json`. Frontend dropped unused JS wrappers `@tauri-apps/plugin-opener`, `plugin-process`, `plugin-updater` (Rust plugins stay). `shadcn` stays because `src/styles.css` imports `shadcn/tailwind.css`.

`cargo audit`: `BLOCKED` (subcommand not installed). `bun audit`: moderate Vitest mocker path traversal (`GHSA-82fw-gwwq-j7x9`); production app does not ship Vitest. Vitest 4 is deferred as an isolated major.

Deferred majors (not mixed with HWND/capture): `reqwest` 0.12 vs 0.13 duplicate, `windows` 0.54/0.58/0.61, `cpal` 0.15, `rusqlite` 0.32, `keyring` 3, `rtrb` 0.3, Criterion 0.5.
