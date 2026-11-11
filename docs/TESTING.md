# Testing

`bun run verify` runs frontend typecheck, lint, format, coverage and Rust fmt/clippy/tests with `--locked` when the MSVC linker is available. `scripts/verify.ps1` owns the build-cache lifecycle; the shared stage list remains in `package.json` as `verify:checks`, and CI runs those same `verify:*` stages. Failures remain fatal and cleanup runs in `finally`.

Frontend coverage includes all product `src` modules, including StatisticsPane, OverlayApp and shadcn primitives. Test discovery is scoped to `src` and `scripts`, so temporary checkouts under `.local` cannot duplicate the suite. Global lines/branches floors are 85%; StatisticsPane and OverlayApp also have per-file floors.

`rust-toolchain.toml` is the canonical compiler/tooling version for local commands, verification CI and release packaging. Install it with `rustup toolchain install --no-self-update`; don't change the global default. Upgrade this pin deliberately and run the complete gate before publication. The independent MSRV check still reads `Cargo.toml` and uses an explicit `cargo +<MSRV>` so the repository pin cannot replace the minimum-version test.

## Preventing local/CI drift

`bun run verify:preflight` fails before expensive tests when the actual Rust/Bun version differs from the repository pins, a compiler override is active, a tracked Rust file contains CRLF, or Git attributes don't force LF. It also checks that CI uses the same `verify:*` commands as the local gate and that both workflows use the pinned Bun version. The guard's negative tests run locally and in CI.

Run `bun run hooks:install` once per clone. It enables the tracked `.githooks/pre-push` hook for this repository only and refuses to replace existing custom hooks. The hook requires a clean tracked tree and the checked HEAD, then runs the complete local `bun run verify` before allowing a push. It doesn't cache successful results or accept stale reports. To undo installation: `git config --local --unset core.hooksPath`.

Git hooks aren't server enforcement and aren't copied automatically into new clones. CI remains mandatory. MSRV compilation, release compilation, installer and native checks remain separate evidence; the preflight doesn't establish that those passed.

## Experimental native HUD

`pwsh -NoProfile -File scripts/rebuild-local-app.ps1 -StopRunning -NativeHud` builds and starts the opt-in Rust/Win32 HUD without an overlay WebView. The main window stays on React. No setting is persisted: starting without `-NativeHud` restores the standard HUD. A failed native window initialization falls back to the standard renderer and logs the error. The experiment uses existing theme tokens and HUD translations; no microphone, API, or persistence code is replaced.

The ignored Windows fixture `native_rust_hud_visibility_alpha_dpi_and_lifetime` creates isolated native windows without microphone/API access or user settings/history. Run it explicitly on an interactive desktop with `cargo test --locked --manifest-path src-tauri/Cargo.toml native_rust_hud_visibility_alpha_dpi_and_lifetime -- --ignored --test-threads=1 --nocapture`. It doesn't establish microphone or insertion acceptance. For daily-driver acceptance, check hover cancellation, Escape, focus, light/dark/system themes and multiple monitor DPI settings.

The prototype doesn't yet expose UI Automation accessibility or report native paint through the WebView `first_frame` timeline. It isn't a default-renderer replacement or evidence of lower memory usage.

## Acceptance policy

The user approved replacing the global Rust 80% floor on 2026-10-08. A global percentage mixes portable logic, native orchestration and test code; it doesn't establish that cancellation or audio recovery works.

- The complete Rust suite remains mandatory. `scripts/check-critical-tests.ps1` runs it once, then checks that every named regression in `scripts/critical-tests.json` executed exactly once and passed. Missing, ignored, duplicate and failed critical tests, or a nonzero Cargo exit, fail the gate.
- The registry protects capture limits and cancellation, duplicate stop, audio/SQLite recovery, original-generation ownership, retry, usage/history, retention, shutdown, partial insertion, Compare and provider failures. It is the canonical mapping of critical scenarios to tests. Review registry changes alongside the implementation; removing a case requires justification and independent review.
- Rust coverage remains measured for the full library, with per-file summaries. It is diagnostic, without a global numeric floor. Review uncovered changed branches and unexplained drops. Compiler, tool, instrumented-test and invalid-report errors still fail; no `continue-on-error` hides them.
- Frontend thresholds remain unchanged. Existing tests are retained; no test is deleted because it is outside the critical registry.
- A green branch CI run confirms automated verification. Native acceptance claims require the corresponding runtime evidence; full microphone/hotkey/HUD/insertion, upgrade and signed updater checks remain separate and must not be inferred from coverage.

Results are generated under `coverage/rust/`: the critical-test result, a bounded failure log, LLVM summary JSON and Markdown. CI uploads `coverage/` on success and failure, retaining artifacts for 14 days. These generated reports are not committed.

Frontend сохраняет отчёты в `coverage/frontend/`. Vitest очищает только этот подкаталог: результаты Rust и критических тестов в соседнем `coverage/rust/` сохраняются при повторном запуске frontend coverage. Регрессия `scripts/coverage-output.test.ts` проверяет, что каталоги не вложены друг в друга.

The default native EDIT tests create only hidden owned windows and validate identity, lifetime, stop-target marker replacement and copy-only fallback through an in-memory clipboard. They never activate a window, send keys or replace the user's clipboard. The opt-in `windows_int::text_injector::native_tests::native_unicode_delivery_to_owned_edit_is_exact_for_cyrillic_and_emoji` test starts in window A, selects B at explicit stop, recreates B's EDIT child and requires exact Unicode delivery to B while A stays empty. It pumps Win32 messages with a bounded deadline. Foreground denial or an empty field fails it; no conditional assertion can turn failed delivery into a pass. Actual end-to-end dictation remains separate evidence.

The opt-in `windows_int::text_injector::hwnd_tests::native_start_target_survives_foreground_switch` test creates two owned Win32 windows, captures A, switches to B and verifies copy-only delivery without sending keys or replacing the user's clipboard. It also rejects changed HWND identity and missing/replaced lifetime markers. Foreground denial is a failure, not a conditional pass. `VOXELY_NATIVE_FOCUS_WAIT=1` gives an interactive desktop controller up to 45 seconds to activate the exact fixture window. The fixture pumps its Win32 messages; it doesn't claim full WebView2 or end-to-end Unicode acceptance.

The opt-in `app::session::overlay_visual_tests::native_overlay_transparency_survives_visibility_cycles` and `app::session::overlay_visual_tests::native_warmed_overlay_shows_after_hidden_idle` tests require a built `dist` and a visible Windows desktop. They render the production HUD through real WebView2 in a temporary profile. The warmed case starts with the normal pre-created hidden window, then uses `show_overlay` to publish Recording. Owned background/client bounds and pointer hit testing distinguish a visible HUD from an occluded or misplaced sample. A legacy negative control turns black when DWM alpha is disabled; the production reveal path must restore alpha. Forty rapid hide/show cycles verify stable non-layered styles and no focus activation. The tests use no microphone and never touch user settings/history. They don't reconstruct an unlogged Windows event.

## Verification layers

Explicit microphone stress acceptance: set `VOXELY_NATIVE_CAPTURE_TEST=1`, `VOXELY_NATIVE_CAPTURE_DIR` to a disposable directory, and optionally `VOXELY_NATIVE_CAPTURE_DEVICE` to the selected input device ID. Run `cargo test --locked --manifest-path src-tauri/Cargo.toml --lib audio::capture::native_stress_tests::native_capture_survives_load_and_forced_overflow -- --exact --ignored --nocapture --test-threads=1`. Bound the process to 240 seconds. This ignored hardware test requires user consent: it records 8-, 20- and 60-second captures under up to four CPU workers and three 18-second captures with a real 12-second writer stall. It checks actual overflow, continued capture until explicit stop, readable finalized WAVs and retained audio after recovery. Audio stays in the disposable directory and is removed; no API, history or settings writes occur. Cleanup guards discard late capture results on failure; the controller must also verify process exit before deleting the exact disposable root, including after a timeout. It exercises native CPAL and the production capture writer, not the GUI/hotkey/HUD flow.

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

Raw `cargo` / `bun x --bun tauri` commands don't acquire the managed lease. Don't start them concurrently with managed builds or cleanup. The idle-process check is conservative but isn't a synchronization protocol for arbitrary external commands. Prefer `bun run --bun verify`, `bun run --bun rebuild:local-app` and the repository helpers.

Если чужой компилятор запускается уже во время проверки, финальная очистка ждёт его завершения до 15 минут, сохраняя lease и временные файлы. Истечение ожидания остаётся ошибкой. Начальная проверка занятости и повторная проверка непосредственно перед удалением не ослаблены; процессы не останавливаются.

## Windows-манифест и основной интерфейс

Основной интерфейс использует Tauri + React. GPUI, его адаптеры и фикстуры удалены. Старые флаги `--native-ui*` и `--cleanup-native-ui-fixture*` отклоняются до инициализации рабочего профиля; они не запускают обычную программу и не меняют настройки, учётные данные или автозапуск. Самостоятельный Win32 HUD с флагом `--native-hud` сохранён.

Единственный Windows-манифест хранится в `src-tauri/windows.manifest.xml` и встраивается штатным ресурсным builder Tauri. Он включает Common Controls v6, PerMonitorV2, asInvoker и прежнюю политику SegmentHeap. Rust test harness явно линкует те же ресурсы. Обычные тесты проверяют встроенный манифест реального exe, активный DPI-контекст, версию загруженного comctl32 и экспорт TaskDialogIndirect без показа диалога.

Из critical registry удалены проверки исчезнувших GPUI экранов, player, fixture cleanup и GPUI checkpoint. Проверки стабильных Tauri capture/cancel/retry, preview leases, сохранения настроек и shutdown сохранены. Группа `desktop-lifecycle` содержит проверки стабильного preview и новых защит запуска/манифеста. Это изменение области продукта, а не снижение порогов или пропуск обязательных тестов.
