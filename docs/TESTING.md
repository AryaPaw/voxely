# Testing

`bun run verify` runs frontend typecheck, lint, format, coverage and Rust fmt/clippy/tests with `--locked` when the MSVC linker is available.

Frontend coverage includes all product `src` modules, including StatisticsPane, OverlayApp and shadcn primitives. Global lines/branches floors are 85%; StatisticsPane and OverlayApp also have per-file floors.

## Acceptance policy

The user approved replacing the global Rust 80% floor on 2026-10-08. A global percentage mixes portable logic, native orchestration and test code; it doesn't establish that cancellation or audio recovery works.

- The complete Rust suite remains mandatory. `scripts/check-critical-tests.ps1` runs it once, then checks that every named regression in `scripts/critical-tests.json` executed exactly once and passed. Missing, ignored, duplicate and failed critical tests, or a nonzero Cargo exit, fail the gate.
- The registry protects capture limits and cancellation, duplicate stop, audio/SQLite recovery, original-generation ownership, retry, usage/history, retention, shutdown, partial insertion, Compare and provider failures. It is the canonical mapping of critical scenarios to tests. Review registry changes alongside the implementation; removing a case requires justification and independent review.
- Rust coverage remains measured for the full library, with per-file summaries. It is diagnostic, without a global numeric floor. Review uncovered changed branches and unexplained drops. Compiler, tool, instrumented-test and invalid-report errors still fail; no `continue-on-error` hides them.
- Frontend thresholds remain unchanged. Existing tests are retained; no test is deleted because it is outside the critical registry.
- A green branch CI run confirms automated verification. Native acceptance claims require the corresponding runtime evidence; full microphone/hotkey/HUD/insertion, upgrade and signed updater checks remain separate and must not be inferred from coverage.

Results are generated under `coverage/rust/`: the critical-test result, a bounded failure log, LLVM summary JSON and Markdown. CI uploads `coverage/` on success and failure, retaining artifacts for 14 days. These generated reports are not committed.

The foreground-dependent Win32 edit-window smoke test remains in the complete suite, but isn't a critical delivery proof: Windows may deny foreground activation, and its conditional assertion then doesn't establish insertion. The critical registry requires deterministic cancellation, partial-delivery and Unicode-chunk contracts instead. Actual foreground delivery remains a separate native acceptance requirement.

## Verification layers

Layers:

1. Unit/integration on every PR (`bun run verify`, `cargo test --locked`). Tests exercise insertion policy, overlay revisions, real SQLite persistence faults, WAV recovery after restart, original-generation admission, and lifecycle release before a simulated UI wait. State-only commands also run through Tauri's MockRuntime and generated IPC handlers, with temporary data. These checks don't prove the complete native STT/HUD/insertion flow. Vite/jsdom and MockRuntime don't prove WebView2, HWND or global hotkeys; dedicated Win32 test windows cover only their named scenarios.
2. App E2E (Tauri/WebdriverIO) is still not in the default gate.
3. CI installer smoke on GitHub `windows-2022` after a signed NSIS build. GitHub Release `make_latest` is true only after that smoke step in the same job. This is not Windows Sandbox.
4. Windows Sandbox (`scripts/run-windows-sandbox.ps1`): required for claiming install/uninstall. If Sandbox is unavailable, status is `BLOCKED`.
5. Host runtime: live dictation after Sandbox PASS. Insertion/HUD claims that change HWND, `SendInput`, or overlay visibility also need the optimized local daily driver (`src-tauri/target/release/voxely.exe`) and `pwsh -File scripts/runtime-matrix.ps1`. That helper reports process evidence; its printed manual matrix still requires separate execution. Vite/jsdom does not prove those surfaces.

Rust critical guard self-tests: `pwsh -File scripts/check-critical-tests.ps1 -SelfTest`. These exercise the runner's failure handling; they do not replace the real Rust suite.

Rust coverage: `pwsh -File scripts/check-rust-coverage.ps1` (CI installs pinned `cargo-llvm-cov` 0.6.16). The script uses a unique `src-tauri/target/llvm-cov-*` directory and deletes it after saving the report, so instrumented objects do not land in `target/debug`. A locally missing tool is reported as skipped, not successful coverage. `dsp::timing::tests::dictation_timing` stays ignored; `dictation_timing_helpers_run` is the cheap gate. `pwsh -File scripts/check-rust-coverage.ps1 -SelfTest` checks failure propagation and acceptance of valid diagnostic percentages.

`src-tauri/target` is a cache, not the app. A debug profile with full symbols plus incremental plus mixed rustc versions grew past 70 GB here. Dev now uses `debug = "line-tables-only"` and `incremental = false`. To wipe caches: `pwsh -File scripts/clean-rust-artifacts.ps1` (or `-Full` for `cargo clean`). MSRV checks must use a separate `CARGO_TARGET_DIR` (for example `src-tauri/target/msrv-1.90`, matching `Cargo.toml`), never the daily-driver `debug` tree.
