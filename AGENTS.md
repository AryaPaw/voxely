# Voxely project instructions

Voxely is a Windows desktop dictation app built with Tauri, Rust, React and Bun. This file is the canonical project instruction source for Codex. Personal defaults remain in `~/.codex/AGENTS.md`; ECC is installed through its native Codex plugin, not copied into this repository.

## Scope and Git checkpoints

Inspect Git status before each implementation wave and preserve unrelated staged, unstaged and untracked work. After a coherent green slice, review the diff and applicable verification. When committing is authorized, make a focused conventional commit; don't mix independent changes or include local tooling dumps, credentials, logs or build artifacts.

Commit, push, tag, version bump and GitHub Release are separate actions requiring the user's authorization. Existing authorization remains valid for the same active task and target. Never rewrite published tags or history without explicit authorization. Do not recreate the removed root `TODO.md`.

## Verification and acceptance

The canonical automated gate is `bun run verify`. Its `verify:*` commands are shared with `.github/workflows/verify.yml`. `bun run verify:preflight` checks the pinned Rust/Bun versions, Git LF behavior and shared CI commands. Install the repository's push guard once per clone with `bun run hooks:install`; do not bypass hooks.

Follow `docs/TESTING.md`:

- The complete Rust suite is mandatory. `scripts/check-critical-tests.ps1` requires every regression in `scripts/critical-tests.json` to execute exactly once and pass. Missing, ignored, duplicate or failed critical tests and Cargo errors block the gate.
- Keep the critical registry aligned with actual behavior. Removing or replacing a case requires a concrete explanation and independent review.
- Measure the full Rust library with `scripts/check-rust-coverage.ps1`. The global Rust percentage is diagnostic, not a publication threshold. Compiler, instrumented-test, tool and report errors remain failures. Don't exclude production surfaces or manufacture tests to improve the number.
- Inspect uncovered changed branches and unexplained coverage drops. Prioritize data loss, capture/cancel/retry races, persistence, retention, provider failures, shutdown and partial delivery.
- Frontend global 85% lines/branches and existing per-file thresholds remain mandatory. This project policy takes precedence over generic ECC/Rust percentage targets; the user approved the global Rust policy revision on 2026-10-08.
- Run the applicable independent change review before declaring a completed change. Report actual PASS, FAIL, NOT RUN, PARTIAL or BLOCKED evidence.

A change to overlay HWND, DPI, tray/taskbar icons, native cues or text insertion requires Windows runtime evidence, or an explicit BLOCKED result. Vite, Playwright, jsdom and MockRuntime do not prove WebView2 clipping, PE icons or native Unicode insertion. Never infer microphone, global hotkey, HUD, insertion, installer, upgrade or signed updater acceptance from coverage alone.

## Local runtime

After Rust/frontend changes that affect the running app, update the daily driver: `src-tauri/target/release/voxely.exe`, built with `VOXELY_LOCAL_BUILD=1`. This is the optimized local build, not `tauri dev`, a debug bundle or the installed `%LOCALAPPDATA%\Voxely` copy.

1. If the executable is locked, use `scripts/stop-local-app.ps1` for a PID-bound, idle-only graceful exit, or Quit in the tray for older builds. `rebuild-local-app.ps1 -StopRunning` combines graceful exit and rebuilding. Never force-stop active work or temporarily change persisted `closeToTray`.
2. Run `bun run rebuild:local-app`.
3. If it didn't launch the app, run `pwsh -NoProfile -File scripts/start-local-app.ps1`. This launches the exact release executable outside the invoking terminal's Windows Job. Do not substitute plain `Start-Process` from an automation terminal.
4. Verify the executable path, start time at or after the binary's modification time, and expected main/overlay HWND. The title is `Voxely (local)` or `Voxely (локальная версия)`.

For automation launches, also verify `IsProcessInJob(process, NULL)` is false. Job membership or a nearby automation-host restart alone does not prove a historical exit's cause. If the user reports a remaining defect, recheck the on-screen executable and timestamps first.

The canonical launcher attaches an independent exit observer. For diagnostics changes, verify observer attachment and the runtime journal's `ready` record. WER dumps require explicit user consent and elevated per-app setup; never enable them silently on startup. See `docs/DIAGNOSTICS.md` for bounds, collection and disabling. Missing completion records alone do not prove a crash.

## Release workflow

Use the global `ap-release-prep` workflow for an authorized publication. Run the full applicable gate and wait for it before push/tag; don't substitute a subset of Vitest for formatting or coverage.

After an authorized push/tag, monitor the resulting GitHub Actions run until a terminal result. On failure, inspect the actual failed log and fix within the authorized scope. A failed tag is not permission to delete or move it. Ask for explicit authorization if recovery requires rewriting the tag or another new consequential action. `gh release create` is not a workaround for a failed `release.yml` publish job.

## Version and build date

`package.json` is the only manually selected version source. `src-tauri/tauri.conf.json` must retain `version: "../package.json"`; Cargo metadata is synced by `scripts/set-version.ps1`.

Use the `$voxely-version` skill for version changes. Propose and obtain confirmation of the number before running `pwsh -File scripts/set-version.ps1 -To X.Y.Z` (or a confirmed `patch`, `minor`, `major`), then run `pwsh -File scripts/check-version.ps1 -Expected X.Y.Z`. Don't edit Cargo files, release documents or frontend files individually for a bump.

About reads compile-time `VOXELY_BUILD_DATE`: CI UTC day, otherwise Git `%cs`, otherwise UTC today. Don't store `APP_RELEASED_ON` or another release day in source. This repository uses GitHub Actions, not GitLab CI.

## DSP ownership

`AppSettings::active_preset()` returns the stored `DspPreset` without rewriting `order`. Don't retain `MicTune` as another source of truth. Preview input is `filter-sample.wav` only; listen-only loudness belongs on preview playback, not `prepare_transcription`.

## Text insertion

Canonical insertion is UTF-16 `KEYEVENTF_UNICODE` into the captured window. Modes are only `unicode` and `clipboard`. Clipboard mode copies and asks the user to paste; it must not send Ctrl+V or race clipboard restoration. Don't revive `auto` or `sendinput` aliases. Surface insertion errors and retain the transcript in history.

## UI token ownership

One semantic token graph: `:root` / `.dark` and one `@theme inline` in `src/styles.css`. Don't add a competing `--color-*` palette, teal leftovers, Geist without a loaded font, or hardcoded history/overlay hex colors outside semantic overlay tokens. Scrollbars, canvas, buttons and HUD use semantic tokens. Overlay follows the app's light/dark/live-system theme.
