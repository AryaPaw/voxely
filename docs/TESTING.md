# Testing

`bun run verify` runs frontend typecheck, lint, format, coverage and Rust fmt/clippy/tests with `--locked` when the MSVC linker is available. `scripts/verify.ps1` owns the build-cache lifecycle; the shared stage list remains in `package.json` as `verify:checks`, and CI runs those same `verify:*` stages. Failures remain fatal and cleanup runs in `finally`.

Vitest запускается на Node 24 LTS, закреплённом в `package.json` (`engines.node`). Установка зависимостей и остальные команды выполняются через Bun. Vitest 5 использует IPC дочерних процессов Node, которого нет в Bun 1.4.5.

Frontend coverage includes all product `src` modules, including StatisticsPane, OverlayApp and shadcn primitives. Test discovery is scoped to `src` and `scripts`, so temporary checkouts under `.local` cannot duplicate the suite. Global lines/branches floors are 85%; StatisticsPane and OverlayApp also have per-file floors.

`rust-toolchain.toml` is the canonical compiler/tooling version for local commands, verification CI and release packaging. Install it with `rustup toolchain install --no-self-update`; don't change the global default. Upgrade this pin deliberately and run the complete gate before publication. The independent MSRV check still reads `Cargo.toml` and uses an explicit `cargo +<MSRV>` so the repository pin cannot replace the minimum-version test.

## Проверка зависимостей

`bun run verify:security` входит в локальный gate и CI. Он проверяет JS-advisory через Bun и Cargo.lock по свежей базе RustSec. В CI устанавливается `cargo-audit` 0.22.2; локально допускается `.local/tools/bin/cargo-audit.exe`. Ошибки инструментов, сети и отчётов не считаются успешной проверкой. Уязвимости блокируют gate; информационные предупреждения RustSec выводятся отдельно.

Для `deepmerge@4.3.1` upstream пока не выпустил исправление GHSA-gcqx-w48m-w2h4. Патч `patches/deepmerge@4.3.1.patch` применяется штатным Bun и блокирует перенос `__proto__` из обоих входов. Номер версии upstream остаётся прежним, поэтому обычный `bun audit` продолжает показывать advisory. Security gate признаёт устранённым только этот точный advisory и только после проверки установленного CJS/UMD-кода на контрпримерах, включая вложенные объекты и target без прототипа. Отсутствующий или неработающий патч, любой другой advisory и изменение версии требуют повторной оценки. После выхода исправленного upstream-релиза патч и специальную проверку следует удалить вместе с обновлением зависимости.

## Preventing local/CI drift

`bun run verify:preflight` fails before expensive tests when Rust, Bun or Node differs from the repository pin, a compiler override is active, a tracked Rust file contains CRLF, or Git attributes don't force LF. It also checks that CI uses the same `verify:*` commands as the local gate and that both workflows use the pinned Bun version. The guard's negative tests run locally and in CI.

Run `bun run hooks:install` once per clone. It enables the tracked `.githooks/pre-push` hook for this repository only and refuses to replace existing custom hooks. The hook requires a clean tracked tree and the checked HEAD, then runs the complete local `bun run verify` before allowing a push. It doesn't cache successful results or accept stale reports. To undo installation: `git config --local --unset core.hooksPath`.

Git hooks aren't server enforcement and aren't copied automatically into new clones. CI remains mandatory. MSRV compilation, release compilation, installer and native checks remain separate evidence; the preflight doesn't establish that those passed.

`pwsh -NoProfile -File scripts/check-build-date.ps1` входит в `verify:metadata` локально и в CI. Изолированный Git/Cargo-стенд подключает настоящий модуль даты из `build_support/build_date.rs` и проверяет обновление даты без изменения исходников: коммит в текущую ветку, упакованные refs, detached HEAD и Git worktree. Повторная сборка без изменений должна использовать кэш. Проверка не меняет рабочую историю Voxely; временные данные удаляются только после сохранения диагностического архива в `.local`.

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

Rust coverage: `pwsh -File scripts/check-rust-coverage.ps1` (CI installs pinned `cargo-llvm-cov` 0.6.16). Инструментированные объекты помещаются в отдельный `src-tauri/target/llvm-cov`, очищаемый под общей блокировкой управляемой операции после сохранения отчёта. Они не попадают в `target/debug`; пустые каталоги профилей с `.cargo-lock` сохраняются для ожидающих Cargo-процессов. A missing tool fails locally too; the script also accepts the repository-local `.local/tools/bin/cargo-llvm-cov.exe` installation. `dsp::timing::tests::dictation_timing` stays ignored; `dictation_timing_helpers_run` is the cheap gate. `pwsh -File scripts/check-rust-coverage.ps1 -SelfTest` checks failure propagation and acceptance of valid diagnostic percentages.

### Automatic Cargo cache maintenance

`src-tauri/target` contains both disposable compilation caches and the protected daily driver `release/voxely.exe`. Never delete the whole directory as routine maintenance.

`scripts/rust-build-cache.ps1` is the policy owner used by full verification, standalone coverage, MSRV checks, local app rebuilding and explicit cleanup:

- Hold one repository-specific file lease for the complete managed command. Nested coverage inherits a session bound to that lease, PID and process start time. A concurrent managed command is refused; a stale lock file alone doesn't block the next run.
- До и после команды очищаются артефакты собственных временных targets: `llvm-cov`, versioned `msrv-*`, прежние каталоги с GUID и `local-fix-verification`. Пустые каталоги с `.cargo-lock` сохраняются: ожидающий Cargo должен продолжить работу с тем же файлом блокировки. Новые запуски используют фиксированные приватные имена `llvm-cov` и `msrv-<version>`, поэтому число таких каталогов не растёт с каждым запуском. Очистка выполняется и после ошибки; остатки после завершения хоста подбирает следующая управляемая операция.
- Retain up to **15 GiB of file lengths** after cleanup. Hardlinks are counted conservatively more than once, so this isn't an exact NTFS allocation measurement. If needed, evict the whole `debug` profile first, then release intermediates (`deps`, `build`, `.fingerprint`, `incremental`, `examples`). Never select individual hashed library files.
- Preserve all release root files, installer bundles, Sandbox results and unknown directories. `-Full` also preserves those files; it no longer invokes `cargo clean`. If protected/unknown data alone exceeds the budget, fail for manual inspection rather than deleting it.
- Require **10 GiB free** before starting a managed command, after safe eviction. This is a reserve check, not a filesystem quota or a guarantee about peak usage during compilation.
- На Windows очистка удерживает совместимые с Cargo блокировки конкретных профилей `.cargo-lock` на всё время удаления. Если занят хотя бы один профиль, уже захваченные блокировки освобождаются перед ограниченным ожиданием. Компиляторы с отдельным target не мешают Voxely; явное использование общего target защищается как общий ресурс. Файлы блокировок не удаляются и не пересоздаются. Саму сборку скрипт запускает после освобождения этих блокировок.
- Работающий cache-exe, кроме защищённого daily driver, запрещает очистку. Сохраняются проверки владельца процесса, reparse points в дереве и родительских путях, разрешённых каталогов и неизвестных данных. Новые профили временного target, не покрытые захваченными блокировками, не удаляются. Процессы скрипт не останавливает.
- Scope `CARGO_TARGET_DIR` to the canonical target during the command and restore the previous environment afterward. Conflicting external Cargo output-directory overrides are refused.

Dev keeps `debug = "line-tables-only"` and `incremental = false`; dependencies use `debug = false` to reduce artifacts without removing workspace source locations from panic backtraces. Debugging dependency internals requires an explicit diagnostic build with symbols.

Explicit maintenance: `pwsh -NoProfile -File scripts/clean-rust-artifacts.ps1`. Add `-Full` to discard all owned compilation caches while retaining runtime files. Neither form deletes source files or application history/settings. Проверки безопасности: `pwsh -NoProfile -File scripts/rust-build-cache.test.ps1`; совместимость блокировок с настоящим Cargo: `pwsh -NoProfile -File scripts/rust-build-cache.native.test.ps1`. Обе проверки входят в `verify:metadata` и используют изолированные каталоги. Native-проверка подтверждает независимость чужого target, защиту занятого общего target и ожидание Cargo до конца удаления.

MSRV CI использует `scripts/check-msrv.ps1`: минимальная версия читается из Cargo.toml, сборка выполняется в `target/msrv-<version>`, артефакты очищаются в `finally`. Coverage использует отдельный `target/llvm-cov`, а отчёты сохраняет вне target. Общий lifecycle owner сериализует операции и сохраняет файлы блокировок, включая ожидателей Cargo. Старые `profraw`/`profdata` удаляются как из корня coverage-target, так и из вложенных namespace рядом с заблокированными профилями, включая `llvm-cov-target`.

Прямые `cargo` / `bun x --bun tauri` не получают managed lease Voxely. Обычный Cargo синхронизируется с очисткой через блокировки своих профилей; общий lifecycle, бюджет и приватные временные targets обслуживают `bun run verify`, `bun run --bun rebuild:local-app` и репозиторные helpers. Сборки других проектов с отдельными targets могут работать параллельно. Произвольные файловые писатели, не соблюдающие Cargo-lock или managed lease, не входят в эту гарантию синхронизации.

Если Cargo удерживает блокировку профиля внутри target Voxely, финальная очистка ждёт её освобождения до 15 минут, сохраняя lease и временные файлы. Истечение ожидания остаётся ошибкой. Блокировки конкретных профилей удерживаются до конца удаления; проверка reparse points повторяется после ожидания. Процессы скрипт не останавливает.

## Windows-манифест и основной интерфейс

Основной интерфейс использует Tauri + React. GPUI, его адаптеры и фикстуры удалены. Старые флаги `--native-ui*` и `--cleanup-native-ui-fixture*` отклоняются до инициализации рабочего профиля; они не запускают обычную программу и не меняют настройки, учётные данные или автозапуск. Самостоятельный Win32 HUD с флагом `--native-hud` сохранён.

Единственный Windows-манифест хранится в `src-tauri/windows.manifest.xml` и встраивается штатным ресурсным builder Tauri. Он включает Common Controls v6, PerMonitorV2, asInvoker и прежнюю политику SegmentHeap. Rust test harness явно линкует те же ресурсы. Обычные тесты проверяют встроенный манифест реального exe, активный DPI-контекст, версию загруженного comctl32 и экспорт TaskDialogIndirect без показа диалога.

Из critical registry удалены проверки исчезнувших GPUI экранов, player, fixture cleanup и GPUI checkpoint. Проверки стабильных Tauri capture/cancel/retry, preview leases, сохранения настроек и shutdown сохранены. Группа `desktop-lifecycle` содержит проверки стабильного preview и новых защит запуска/манифеста. Это изменение области продукта, а не снижение порогов или пропуск обязательных тестов.
