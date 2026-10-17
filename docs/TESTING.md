# Testing

`bun run verify` runs frontend typecheck, lint, format, coverage and Rust fmt/clippy/tests with `--locked` when the MSVC linker is available. `scripts/verify.ps1` owns the build-cache lifecycle; the shared stage list remains in `package.json` as `verify:checks`, and CI runs those same `verify:*` stages. Failures remain fatal and cleanup runs in `finally`.

Frontend coverage includes all product `src` modules, including StatisticsPane, OverlayApp and shadcn primitives. Test discovery is scoped to `src` and `scripts`, so temporary checkouts under `.local` cannot duplicate the suite. Global lines/branches floors are 85%; StatisticsPane and OverlayApp also have per-file floors.

`rust-toolchain.toml` is the canonical compiler/tooling version for local commands, verification CI and release packaging. Install it with `rustup toolchain install --no-self-update`; don't change the global default. Upgrade this pin deliberately and run the complete gate before publication. The independent MSRV check still reads `Cargo.toml` and uses an explicit `cargo +<MSRV>` so the repository pin cannot replace the minimum-version test.

## Preventing local/CI drift

`bun run verify:preflight` fails before expensive tests when the actual Rust/Bun version differs from the repository pins, a compiler override is active, a tracked Rust file contains CRLF, or Git attributes don't force LF. It also checks that CI uses the same `verify:*` commands as the local gate and that both workflows use the pinned Bun version. The guard's negative tests run locally and in CI.

Run `bun run hooks:install` once per clone. It enables the tracked `.githooks/pre-push` hook for this repository only and refuses to replace existing custom hooks. The hook requires a clean tracked tree and the checked HEAD, then runs the complete local `bun run verify` before allowing a push. It doesn't cache successful results or accept stale reports. To undo installation: `git config --local --unset core.hooksPath`.

Git hooks aren't server enforcement and aren't copied automatically into new clones. CI remains mandatory. MSRV compilation, release compilation, installer and native checks remain separate evidence; the preflight doesn't establish that those passed.

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

### Sandbox WebView2 prerequisite

Some Sandbox images contain signed WebView2 files but stale EdgeUpdate registration. The observed image had runtime `154.0.4258.62`, while `ClientState\EBWebView` pointed at the missing `154.0.4258.53` directory. Updating only `Clients\pv` didn't fix detection. Native `GetAvailableCoreWebView2BrowserVersionString` changed from `80070002` to success after repairing the actual registered path.

For this environment, explicitly request prerequisite preparation: `pwsh -NoProfile -File scripts/run-windows-sandbox.ps1 -InstallerPath <NSIS.exe> -PrepareRuntime`.

`sandbox-prepare-webview.ps1` runs only as the Sandbox account. It requires one installed runtime, matching file versions and valid Microsoft signatures on the executable and x64 WebView DLL. It updates only that runtime's `Clients\pv`, `ClientState\pv` and `ClientState\EBWebView`; it refuses ambiguous versions, foreign paths and replacement of an existing runtime directory. The guest result records the previous and resulting registration in `runtimePreparation`.

The default runner doesn't repair the environment. A prepared-runtime PASS proves installation, the real main window, process-bound `ready`, and uninstall under that recorded prerequisite. It doesn't prove bootstrapper recovery on an image with missing or broken WebView2. Host installation and host registry changes aren't part of either mode.

Rust coverage: `pwsh -File scripts/check-rust-coverage.ps1` (CI installs pinned `cargo-llvm-cov` 0.6.16). The script uses a unique `src-tauri/target/llvm-cov-*` directory and deletes it after saving the report, so instrumented objects do not land in `target/debug`. A missing tool fails locally too; the script also accepts the repository-local `.local/tools/bin/cargo-llvm-cov.exe` installation. `dsp::timing::tests::dictation_timing` stays ignored; `dictation_timing_helpers_run` is the cheap gate. `pwsh -File scripts/check-rust-coverage.ps1 -SelfTest` checks failure propagation and acceptance of valid diagnostic percentages.

### Automatic Cargo cache maintenance

`src-tauri/target` contains both disposable compilation caches and the protected daily driver `release/voxely.exe`. Never delete the whole directory as routine maintenance.

`scripts/rust-build-cache.ps1` is the policy owner used by full verification, standalone coverage, MSRV checks, local app rebuilding and explicit cleanup:

- Hold one repository-specific file lease for the complete managed command. Nested coverage inherits a session bound to that lease, PID and process start time. A concurrent managed command is refused; a stale lock file alone doesn't block the next run.
- Before and after the command, remove owned temporary targets: `llvm-cov`, `llvm-cov-<GUID>`, versioned `msrv-*`, and the retired `local-fix-verification` directory. Normal exits and failures run cleanup; the next managed run picks up leftovers after a terminated host.
- Retain up to **15 GiB of file lengths** after cleanup. Hardlinks are counted conservatively more than once, so this isn't an exact NTFS allocation measurement. If needed, evict the whole `debug` profile first, then release intermediates (`deps`, `build`, `.fingerprint`, `incremental`, `examples`). Never select individual hashed library files.
- Preserve all release root files, installer bundles, Sandbox results and unknown directories. `-Full` also preserves those files; it no longer invokes `cargo clean`. If protected/unknown data alone exceeds the budget, fail for manual inspection rather than deleting it.
- Require **10 GiB free** before starting a managed command, after safe eviction. This is a reserve check, not a filesystem quota or a guarantee about peak usage during compilation.
- Refuse deletion while a compiler/linker is active, when a target executable other than the protected daily driver is running, or when process ownership is unavailable. Never stop those processes. Reject reparse points in the cache tree and ancestors, and validate every deletion against the owned-directory allowlist.
- Scope `CARGO_TARGET_DIR` to the canonical target during the command and restore the previous environment afterward. Conflicting external Cargo output-directory overrides are refused.

Dev keeps `debug = "line-tables-only"` and `incremental = false`; dependencies use `debug = false` to reduce artifacts without removing workspace source locations from panic backtraces. Debugging dependency internals requires an explicit diagnostic build with symbols.

Explicit maintenance: `pwsh -NoProfile -File scripts/clean-rust-artifacts.ps1`. Add `-Full` to discard all owned compilation caches while retaining runtime files. Neither form deletes source files or application history/settings. The safety fixtures run with `pwsh -NoProfile -File scripts/rust-build-cache.test.ps1` and through `verify:metadata`.

MSRV CI uses `scripts/check-msrv.ps1`: read the minimum version from Cargo.toml, build in `target/msrv-<version>-<GUID>`, remove it in `finally`. Coverage keeps its own isolated GUID target and preserves the generated coverage reports outside target. Both commands use the same lifecycle owner.

Raw `cargo` / `bunx tauri` commands don't acquire the managed lease. Don't start them concurrently with managed builds or cleanup. The idle-process check is conservative but isn't a synchronization protocol for arbitrary external commands. Prefer `bun run verify`, `bun run rebuild:local-app` and the repository helpers.
