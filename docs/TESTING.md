# Testing

`bun run verify` runs frontend typecheck, lint, format, coverage and Rust fmt/clippy/tests with `--locked` when the MSVC linker is available.

Coverage include: product lib modules plus MainApp, HistoryPane, ComparePane, FilterSettings, TranscriptionSettings, OverlayApp, and shadcn primitives.

Layers:

1. Unit/integration on every PR (`bun run verify`, `cargo test --locked`). Insertion policy, overlay revision, history persistence, and the live STT path (persist completed, history event, hide overlay, idle, insert) are behavioral tests. Vite/jsdom does not prove HWND, global hotkeys, or Unicode insert.
2. App E2E (Tauri/WebdriverIO) is still not in the default gate.
3. CI installer smoke on GitHub `windows-2022` after a signed NSIS build. GitHub Release `make_latest` is true only after that smoke step in the same job. This is not Windows Sandbox.
4. Windows Sandbox (`scripts/run-windows-sandbox.ps1`): required for claiming install/uninstall. If Sandbox is unavailable, status is `BLOCKED`.
5. Host runtime: live dictation after Sandbox PASS. Insertion/HUD claims that change HWND, `SendInput`, or overlay visibility also need the daily-driver debug exe (`src-tauri/target/debug/voxely.exe`) and `pwsh -File scripts/runtime-matrix.ps1`. Vite/jsdom does not prove those surfaces.

Rust coverage: `pwsh -File scripts/check-rust-coverage.ps1` (CI installs `cargo-llvm-cov`). `dsp::timing::tests::dictation_timing` stays ignored; `dictation_timing_helpers_run` is the cheap gate.
