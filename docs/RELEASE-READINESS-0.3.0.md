# Подготовка Voxely 0.3.0

Дата: 8 октября 2026. Режим: `push`, только ветка `main`. Итог: **READY для публикации исходников**, полная native-приёмка **BLOCKED**.

## Текущая политика проверки и публикация исходников

Пользователь явно согласовал пересмотр политики покрытия. Общий Rust-порог 80% заменён проверкой обязательных сценариев; полный набор тестов сохранён. Процент остаётся в LLVM JSON, файловой сводке и CI-артефакте. Ошибки компиляции, инструмента, инструментированных тестов и некорректные отчёты по-прежнему блокируют gate. Frontend-пороги не изменены.

### Исправление Windows checkout после первого push

Первый удалённый `verify` для `e55d0c8`: **RUN - FAIL**, run `37783915759`. MSRV compile и frontend checks прошли; Rust format отклонил CRLF после Windows checkout. В `rustfmt.toml` требуется `newline_style = "Unix"`, а прежний `* text=auto` не фиксировал LF в рабочем дереве.

В `.gitattributes` добавлено `*.rs text eol=lf`. Глобальные настройки Git не изменялись. Изолированный checkout с `core.autocrlf=true` создал **53 Rust-файла с LF**; `cargo fmt --all --check` на этой копии прошёл. Runtime-код, LLVM inputs и тестовые assertions не менялись; их успешные проверки переиспользованы. Повторный удалённый gate должен подтвердить исправление, его результат здесь не объявляется PASS заранее.

Повторный run `37785322782` для `d957937` подтвердил Rust format и frontend, но остановился на Clippy: GitHub `stable` уже использовал Rust **1.99.0**, локальный проверенный toolchain был **1.98.1**. Причина: `AtomicUsize::fetch_update` помечен deprecated в 1.99; `-D warnings` честно отклонил предупреждение. Suppression не добавлялся.

Добавлен канонический `rust-toolchain.toml` с проверенной версией **1.98.1**. Локальные команды, verify CI и release packaging используют этот файл через Rustup; плавающий `stable` удалён из этих build-путей. MSRV по-прежнему берётся из `Cargo.toml` и запускается явно через `cargo +<MSRV>`. Это фиксация воспроизводимой среды, а не подтверждение совместимости с 1.99: обновление compiler pin требует отдельной проверки и MSRV-совместимой миграции deprecated API. Проверки не ослаблялись. Новый удалённый run должен подтвердить итоговый workflow.

Именованный pin установлен локально. SHA256 Rustc, Cargo, rustfmt, Clippy и LLVM coverage tools совпали с инструментами предыдущего успешного gate. На pin повторно прошли fmt, Clippy all targets/features, полный Rust suite (**420 passed, 1 ignored**, **43/43** обязательных случаев), форматирование и отдельная MSRV 1.90.0 сборка. Логи: `clippy-pinned.log`, `critical-pinned.log`, `msrv-pinned.log`, `format-pinned.log`; identity-отчёты: `toolchain-identity.json`, `llvm-toolchain-identity.json`. Остальные проверки переиспользованы для неизменённых исходников и побайтно совпадающих инструментов; повторный инструментированный прогон локально не запускался.

- Канонический реестр `scripts/critical-tests.json`: **43 случая в 10 областях риска**. Runner выполняет весь Rust suite и требует точный единственный PASS каждого случая. Missing, ignored, duplicate, FAILED и ненулевой Cargo exit означают отказ.
- Критический реестр проверяет детерминированные контракты вставки, отмены и Unicode chunks. Условный Win32 foreground smoke сохранён в полном наборе, но не считается доказательством реальной доставки. Основание замены и native-границы описаны в `docs/TESTING.md` и проверены независимым ревью.
- Самопроверки обоих runners: **RUN - PASS**, включая реальный дочерний exit 42. Отрицательные фикстуры проверяют повреждённый реестр и LLVM JSON, а не только успешный пример.
- Повторный полный Rust suite с окончательным реестром: **RUN - PASS**, **420 passed, 1 ignored**, все **43** обязательных случая прошли. Лог: `.local/release-prep/policy-20261008/critical-final.log`.
- Полный `bun run verify`: **RUN - PASS**, exit 0; frontend **293 passed**, Rust **420 passed, 1 ignored**, включая инструментированный прогон; Rust coverage **73.83% lines**. Лог: `.local/release-prep/policy-20261008/verify.log`. После замечания review усилен только валидатор отчёта; окончательная функция отдельно проверена на фактическом LLVM JSON (**43 файла**, PASS), а окончательный реестр повторно прошёл полный Rust suite. Неизменённая инструментированная компиляция не повторялась. Итоги: `final-validator.json`, `critical-report.json`.
- Финальные WinGet helper tests и Icon SSOT: **RUN - PASS**. Точный staged scope проверен на whitespace errors, запрещённые пути и сигнатуры секретов; сторонние untracked Cursor-файлы сохранены вне commit.
- Три независимых review tracks: **PASS** для текущей политики и исправлений проверки. Correctness и adversarial повторены после исправления числовой валидации отчёта и замены условного Win32 случая. Security проверил границы удаления, privacy артефактов, публикационный scope и достижимость dependency advisory.
- Свежий `bun audit --prod --json`: **RUN - PASS**, exit 0. Полный `bun audit --json`: **RUN - FAIL**, exit 1, HIGH `braces@3.0.3`, GHSA-vfj7-8cjw-p6xm. Отчёты сохранены в `.local/release-prep/policy-20261008/`.

### Оставшийся advisory и границы готовности

У `braces` нет официально исправленной версии на дату проверки. Зависимость приходит через dev shadcn CLI; текущие scripts и CI его не вызывают. Приложение импортирует статический `shadcn/tailwind.css`, а `braces`, `micromatch` и `fast-glob` в собранных assets не обнаружены. Независимое security review не нашло достижимого блокирующего риска для публикации исходников в `main`. Полный audit не объявляется чистым; риск следует пересмотреть при появлении исправления либо использовании CLI с недоверенными шаблонами. [Официальный advisory](https://github.com/advisories/GHSA-vfj7-8cjw-p6xm).

Release EXE, MSRV, неподписанный NSIS и свежая установка/удаление в Windows Sandbox прошли проверки предыдущей волны ниже. Runtime-исходники с тех пор не менялись. Работающая локальная release-программа сохранена. Публикация ветки не создаёт tag или Release и не подтверждает подписанный updater.

Полная native microphone/hotkey/HUD/Escape/STT/insertion матрица, upgrade, подписанный updater и сопоставимые RAM/latency серии остаются **NOT RUN**. Полная приёмка этих сценариев остаётся **BLOCKED** до их выполнения. Причина исторических неожиданных завершений не доказана найденным deadlock. Готовность исходников и автоматических проверок не означает, что приложение проверено идеально во всех сценариях.

## Архив: исправления и прежний процентный gate

Ниже сохранены результаты до согласованной смены политики. Тогдашние FAIL и NOT READY не переписываются в PASS задним числом.

## Исправления после аудита: 8 октября 2026

Этот раздел описывает текущую волну исправлений и заменяет выводы архивной проверки ниже. Версия остаётся **0.3.0**. Публикация не выполнялась; корневой `TODO.md` не восстанавливался. Пользовательские изменения сохранены.

### Исправленные дефекты

- **UI/lifecycle deadlock:** успешное завершение STT сначала сохраняет результат и резервирует вставку, затем освобождает lifecycle mutex до HWND/UI-вызовов. Hide выполняется целиком на UI-потоке с проверкой поколения в той же операции. Ветви отмены и ошибки также освобождают mutex до UI effects.
- **Сохранность capture WAV при ошибке БД:** ошибка чтения строки больше не считается доказательством её удаления. Аудио сохраняется; удаление разрешено только после подтверждённого отсутствия записи. Реальные SQLite lookup/update faults проверяют восстановление после повторного открытия БД.
- **Сохранность processed WAV:** ссылка сохраняется до удаления raw. Ошибка unlink сохраняет raw reference, а сбой последующего metadata update не лишает БД уже сохранённой processed reference. Trigger-тесты проверяют повторный запуск и orphan cleanup.
- **Запоздалые задачи:** исходное поколение проходит от остановки capture до регистрации транскрибации. Регистрация под lifecycle проверяет shutdown, владельца записи и существующий сигнал отмены. Поздний pipeline не создаёт новую отмену и не принимает поколение новой сессии. Обновления метаданных также защищены этим поколением.
- **Compare:** ошибки capture, чтения, DSP и записи очищают соответствующие raw/tmp. Неполная новая пара listen/STT откатывается; коллизия конечных путей отклоняется до записи. Старое сохранённое аудио и состояние другой записи сохраняются.
- **Размер окна:** reset читает размеры `main` из Tauri config. Дублирующие Rust-константы удалены; конфигурационные и native-проверки описаны ниже.
- **Frontend-тесты:** настоящий keyboard drag-and-drop проверяет reorder, сохранение правил и Escape. Тест остановки записи обращается к исходной кнопке записи, отдельно от кнопки прослушивания; ошибка и возможность повторной остановки проверяются явно.
- **Дополнительные тесты:** реальные SQLite/WAV faults, retention с сохранением метрик, retry rollback, DSP, meter ownership, generated Tauri IPC, замены слов и проверка тегов версии. `1.2.3.4` больше не принимается как `1.2.3`.

### Текущие доказательства и границы

Локальные логи: `.local/release-prep/fix-20261008/`. Исходный снимок этой волны: `candidate-source.json`; итоговый набор: `candidate-final.json`. Предыдущие продуктовые изменения и staged deletion `TODO.md` не коммитились автоматически.

- Frontend: **293 passed**, 40 файлов; statements **89.36%**, branches **87.25%**, functions **84.78%**, lines **90.04%**. Typecheck, ESLint и frontend production build прошли. Эти входные файлы после проверки не менялись.
- Расширенные Rust tests: **420 passed, 1 ignored**, `rust-tests-expanded.log`, exit 0. Rust fmt и Clippy all targets/features прошли, `clippy-expanded.log`, exit 0.
- Три независимых статических ревью окончательных runtime-исправлений: **PASS**. Correctness, security/data safety и adversarial/test integrity выполнялись ревьюерами, не участвовавшими в проверяемой реализации. Ревью не заменяет выполнение тестов и native acceptance.
- Полный `bun run verify` на промежуточном кандидате: **RUN - FAIL**, `verify-final.log`, exit 1. Все этапы до Rust coverage прошли; покрытие составляло **71.44% lines** при требуемых **80%**. После дополнительных тестов и Compare-исправления повторены затронутые Rust-проверки. Финальный `rust-coverage-expanded.log`: **73.83% lines**, **420 passed, 1 ignored**, exit 1 из-за порога. Порог и исключения не менялись; неизменённые frontend-проверки переиспользованы.
- Финальная MSRV-сборка: `cargo +1.90.0 build --locked --manifest-path src-tauri/Cargo.toml --all-targets`, отдельный target directory, **RUN - PASS**, `msrv-build-expanded.log`, exit 0.
- Native reset проверен на release EXE от 14:47 MSK: развёрнутое окно 2560 × 1392 вернулось к стандартному размеру, внешний кадр 963 × 711 с Windows frame, показан toast успешного сброса. Его код после этой проверки не менялся. Это проверка reset, не подтверждение STT/cancel race.
- Финальная release-сборка с Compare-исправлением: **RUN - PASS**, `local-rebuild-expanded.log`, 7 минут 51 секунда. Прежний PID 75080 завершён штатно с code 0 через idle-only control, без изменения close policy. Новый EXE SHA256: `DC6756B8E6553E0D384BF3471A6C09E9A3C40CF99CA3BB8C29CE979AE4A7ACCA`. В 14:59:38 MSK запущен PID **70740**, видимое `Voxely (local)`, HWND **2037100**, вне Windows Job, observer **75704**. Запуск позже времени записи EXE; входные runtime-файлы после сборки не менялись. Главное окно и загруженная статистика наблюдались через native UI; `Success rate` отсутствует. Это базовая проверка запуска, не полная матрица диктовки.
- Байты `settings.json` до и после native-проверок совпали: `final-integrity.json`. Копия EXE для bundler побайтно совпадает с работающим daily driver; пользовательский EXE не заменялся bundler.
- Локальный неподписанный NSIS: **RUN - PASS**, `local-bundle-final.log`, официальный Tauri bundle в отдельном `CARGO_TARGET_DIR`, `createUpdaterArtifacts=false`, `--no-sign --no-binary-patching`. SHA256 установщика: `A4BD085C753119C1AA8E3EA82A40D607C570E7F24525CA43BD67255796D9385B`.
- Windows Sandbox: **RUN - PASS**, `sandbox-final.log`, run **8edaa3b6e1d54c7f9aa9b8e5cb5a5965**. Guest и host result совпали по run ID, SHA256 и версии 0.3.0; установка, запуск точного установленного EXE с видимым HWND и удаление выполнены. Результаты: `src-tauri/target/sandbox-runs/8edaa3b6e1d54c7f9aa9b8e5cb5a5965/`. Это свежая установка, не upgrade и не подписанный updater.

### Незакрытые блокеры

- Rust coverage **73.83% < 80%**; до этой волны было **67.93%**. Главные непроверенные ветви связаны с orchestration, жёстко связанным с `AppHandle<Wry>`, HWND, transport и микрофоном. Безопасное дальнейшее покрытие требует выделения владельцев эффектов и интеграционных сценариев; фиктивные тесты и ослабление gate не использовались.
- **Dev advisory HIGH `braces` 3.0.3:** на момент проверки официального исправления нет, advisory указывает `Patched versions: None`. Даже актуальная shadcn-цепочка приводит к этой версии. `bun audit --prod` чистый; полный audit и dry-run fix завершаются отказом. Manifest/lockfile не менялись ради подавления. [Официальный advisory](https://github.com/advisories/GHSA-vfj7-8cjw-p6xm).
- Полная native Windows/WebView2 матрица, реальная гонка Escape/STT, microphone/capture/quit, upgrade, подписанный updater и сопоставимые RAM/latency серии остаются **NOT RUN**. Причина исторических неожиданных завершений не доказана найденным deadlock.

Общий статус: **NOT READY**. Исправленные функциональные дефекты не объявляются доказанной причиной прежних падений.

## Архив: read-only проверка 8 октября, 14:09–14:18 MSK

Этот раздел заменяет прежний вывод о готовности. Нижние разделы сохранены как история предыдущих проверок, а не как доказательство принятия текущего кандидата.

Кандидат: рабочее дерево `main` относительно `58d49fb3de6713a191bdc6aa894f19addedac092`, версия **0.3.0**. Исходники приложения в этой проверке не менялись. Изменены только этот отчёт и `.gitignore`: сырые `docs/perf/trials/*.png` исключены из публикации, локальные файлы сохранены. Сторонние новые файлы `.cursor` не входят в продуктовый scope. Staging, commit, push, tag, bump версии и публикация не выполнялись.

Локальные доказательства: `.local/release-prep/0.3.0/20261008-140917/`. `candidate-before.json` фиксирует исходный снимок; `candidate-after.json` фиксирует итоговый набор с SHA256 и исключёнными raw PNG. Манифест не является инструкцией для автоматического staging.

### Свежие проверки

| Проверка                                                                                                  | Результат                                     | Доказательство и границы                                                                                                                                                                                                              |
| --------------------------------------------------------------------------------------------------------- | --------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Полный `bun run verify`                                                                                   | **RUN - FAIL**, exit 1                        | `verify.log`: настоящий Rust coverage **67.93% lines**, обязательный порог **80%**. cargo-llvm-cov 0.6.16 подключён через локальный PATH; пропуска нет. `CARGO_BUILD_JOBS=2`, `RUST_TEST_THREADS=8`                                   |
| Typecheck, ESLint, Prettier, Rust fmt, Clippy all targets/features, версия, diagnostics                   | RUN - PASS                                    | Все этапы прошли до отказа coverage в той же команде                                                                                                                                                                                  |
| Frontend tests и coverage                                                                                 | RUN - PASS                                    | 40 файлов, **291 passed**; statements 88.83%, branches 86.61%, functions 84.00%, lines 89.48%                                                                                                                                         |
| Rust tests                                                                                                | RUN - PASS                                    | **345 passed, 1 ignored**; те же результаты при instrumented coverage. Ignored тест не объявляется пройденным                                                                                                                         |
| `bun run build`                                                                                           | RUN - PASS                                    | `frontend-build.log`, exit 0; production frontend build                                                                                                                                                                               |
| MSRV 1.90                                                                                                 | RUN - PASS                                    | `msrv-build.log`: `cargo +1.90.0 build --locked --manifest-path src-tauri/Cargo.toml --all-targets`, отдельный `target/msrv-1.90`, `CARGO_BUILD_JOBS=1`, exit 0                                                                       |
| Icons, WinGet helpers, installer smoke self-test, coverage exit-code self-test, Sandbox helper self-tests | RUN - PASS                                    | `extra-checks.log`; ожидаемая инъекция exit 42 проверяет отказ gate. Установщик и VM этими self-tests не запускались                                                                                                                  |
| `bun audit --prod --json`                                                                                 | RUN - PASS                                    | `bun-audit-prod.json`: `{}`, exit 0                                                                                                                                                                                                   |
| `bun audit --json`                                                                                        | **RUN - FAIL**                                | `bun-audit.json`: HIGH `braces` 3.0.3, GHSA-vfj7-8cjw-p6xm, dev-цепочка shadcn                                                                                                                                                        |
| `bun audit fix --dry-run --json`                                                                          | **RUN - FAIL**, exit 1                        | `bun-audit-plan.json`: fixed 0, remaining 1, unfixable braces; доступного автоматического исправления не найдено. Manifest и lockfile не менялись                                                                                     |
| Cargo advisory scan                                                                                       | RUN - PASS с предупреждениями                 | `cargo-audit.json`, exit 0, 0 vulnerabilities; RustSec от 7 октября, commit `b8a1a33e246a0a9a3b5f377248c41a503defec74`. Предупреждения: async-std / RUSTSEC-2025-0052, proc-macro-error / RUSTSEC-2024-0370, glib / RUSTSEC-2024-0429 |
| Текущая release-программа                                                                                 | PARTIAL, переиспользована проверка этой волны | EXE от 13:34 MSK, SHA256 `996B27A902D589C1EADC7626C9FA112587E9A67505BE70135FB8BF24A0A2C534`; PID 66532, видимое `Voxely (local)`. Источники runtime после сборки не менялись. Это не проверка найденных гонок                         |
| Новая упаковка текущего runtime, установка/удаление и upgrade/updater                                     | NOT RUN                                       | Старый NSIS и его Sandbox PASS относятся к предыдущему коду; финальный подписанный пакет отсутствует                                                                                                                                  |
| Полная native Windows/WebView2 матрица и RAM/latency серия                                                | NOT RUN                                       | Особенно Escape около завершения STT, Busy/capture completion/quit, updater/restart. Fixtures, jsdom и живой процесс её не заменяют                                                                                                   |

### Независимое ревью текущего кандидата

Три независимых read-only субагента: correctness/regression, security/data safety, adversarial/test integrity. Исходники, тесты, приложение и данные reviewers не изменяли.

- **Correctness: FINDINGS**, три HIGH ниже. Это статически подтверждённые опасные ветви и конкурентный контрпример; native воспроизведение ещё не выполнено.
- **Security: RUN - PASS после исправления.** MEDIUM о постороннем содержимом в сырых PNG закрыт исключением из Git. Повторный reviewer подтвердил `git check-ignore`, отсутствие PNG в publication scope и сохранность локальных файлов. Новых security findings нет.
- **Adversarial: FINDINGS**, два LOW ниже; новых Critical/High/Medium в этом направлении нет. Порог coverage, exit-code propagation и привязка Sandbox результата проверены чтением кода, не выполнением native acceptance.

### Блокирующие дефекты

1. **HIGH, высокая уверенность: UI/lifecycle deadlock на завершении STT.** `src-tauri/src/app/session.rs:1688` удерживает `session_lifecycle` через `publish_stt_success`; `window.hwnd()` на async-потоке синхронно ждёт UI. Обработчик Escape на UI одновременно может ждать тот же mutex (`cancel_recording:228`). Получается цикл ожидания. Аналогичная ветвь: pipeline cancellation, строки 1531–1544. Основание: фактически используемый tauri-runtime-wry 2.12.1, getter через `window_getter` / `rx.recv()`. Нужны разделение state commit и UI effects, сохранение generation checks и insertion reservation, управляемый конкурентный тест, затем native Escape около завершения STT. Исправление shutdown owner из предыдущей волны не закрывает эти ветви.
2. **HIGH, высокая уверенность: потеря capture WAV при ошибке чтения SQLite.** В `finish_stop`, `session.rs:1433–1434`, `history.get -> Err` вызывает `remove_capture_artifacts`. Ошибка хранения не доказывает отмену или отсутствие строки. Успешно финализированное аудио физически удаляется. Нужны сохраняющий данные путь отказа, защита recoverable WAV от orphan cleanup, проверка восстановления ссылки после доступности БД и fault-injection тест. Простого удаления вызова unlink недостаточно для доказательства восстановления после перезапуска.
3. **HIGH, высокая уверенность: удаление raw до сохранения ссылки на processed.** `session.rs:1634–1635`, `apply_raw_retention:3389–3391`: при `keep_original_recordings=false` raw удаляется до `history.update`. Если update падает, в БД остаётся ссылка на отсутствующий raw, processed остаётся без ссылки и может быть удалён orphan cleanup. Дефект уже был в HEAD. Нужны сохранение processed reference до destructive cleanup, обработка ошибки unlink без потери ссылки и реальный SQLite-trigger тест с повторным открытием БД/retention.

Эти изменения затрагивают порядок владения сессией и сохранность пользовательского аудио. В рамках оценки готовности они не исправлялись частичными перестановками вызовов без регрессионного доказательства. Перед публикацией требуется отдельная согласованная волна исправления трёх путей, повтор correctness/security review, full gate и native проверка. Общий результат review не PASS.

### Остальные блокеры и замечания

- Rust coverage **67.93% < 80%**. Текущие lines: session.rs 56.96%, compare.rs 53.83%. Нужны содержательные тесты; порог и исключения не ослаблялись.
- Dev advisory `braces` остаётся. [Официальный advisory](https://github.com/advisories/GHSA-vfj7-8cjw-p6xm). Production dependency scan чистый, но полный scan красный; dry-run не нашёл исправленной версии. Подавление finding не выполнялось.
- **LOW:** нет поведенческого теста reorder/cancel для `TextReplacementSettings.onDragEnd`, включая сохранение порядка правил.
- **LOW:** стандартный размер окна дублируется в `tauri.conf.json` и Rust-константах reset; нужен единый источник либо обязательный drift test.
- Финальные NSIS, upgrade, подписанный updater и native матрица остаются непроверенными. Причина исторических неожиданных завершений по-прежнему неизвестна; найденный deadlock сам по себе не доказывает причину прошлых выходов.

## История предыдущих волн подготовки

Версия обновлена каноническим `scripts/set-version.ps1 -To 0.3.0`. Источник версии – `package.json`; Cargo синхронизирован. Commit, push, tag, GitHub Release и WinGet-публикация не выполнялись.

## Кандидат

База сравнения: `58d49fb3de6713a191bdc6aa894f19addedac092`, ветка `main`. Проверяется накопленное рабочее дерево приложения, а не только изменение номера. Посторонние локальные изменения сохранены. Перечень файлов и SHA256 фиксируется локально в `.local/release-prep/0.3.0/candidate.json`; это не список файлов для автоматического staging.

## Исправления этой подготовки

- Compare сохраняет reservation во время DSP; commit проверяет исходный nonce и shutdown под admission. Поздний результат не перезаписывает следующую запись. Добавлены регрессии с барьерами и реальными файлами.
- Публичный retention удерживает admission на всём пути. Периодический caller использует вариант с уже полученным guard, без повторного захвата. Конкурентный тест использует настоящие SQLite и WAV.
- Восстановлены глобальные frontend-пороги 85% lines/branches; UI-компоненты снова включены в coverage. Добавлены поведенческие тесты, без изменения продуктового кода ради покрытия.
- MSRV исправлен с 1.89 на 1.90: locked Tauri 2.12.1 требует 1.90. Сборка 1.89 действительно отклоняла граф зависимостей; сборка всех targets на 1.90 проходит.
- Sandbox gate проверяет уникальный run ID, SHA256 установщика, версию EXE, точный процесс, видимый HWND и удаление. Ожидания ограничены, результат атомарный; устаревший результат не принимается.
- Документация отделяет локальную release-программу от debug-сборки, пропуск coverage от PASS и checklist от native acceptance.

## Проверки

| Проверка                                                                       | Результат                       | Доказательство и границы                                                                                                                                                                                    |
| ------------------------------------------------------------------------------ | ------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `bun run verify`                                                               | PASS команды, неполный coverage | `output/release-0.3.0-verify.log`: typecheck, ESLint, форматирование, frontend coverage, Rust fmt/Clippy, 336 Rust passed / 1 ignored, версия и diagnostics. Rust coverage внутри команды был SKIP          |
| Финальный frontend coverage                                                    | PASS                            | `output/release-0.3.0-coverage-final.log`: 291 тест, statements 88.62%, branches 86.61%, functions 83.69%, lines 89.26%. Повтор после очистки mocks                                                         |
| Rust coverage, настоящий запуск                                                | **FAIL**                        | `output/release-0.3.0-rust-coverage.log`: cargo-llvm-cov 0.6.16, 336 passed / 1 ignored, lines **67.73%** при требуемых **80%**; exit 1                                                                     |
| MSRV 1.90, `build --locked --all-targets`                                      | PASS                            | `output/release-0.3.0-msrv.log`, отдельный target directory                                                                                                                                                 |
| Icons, WinGet helpers, installer smoke self-test, coverage exit-code self-test | PASS                            | Локальные проверки; self-test не устанавливает приложение                                                                                                                                                   |
| Sandbox helper self-tests                                                      | PASS                            | PowerShell 5.1; отрицательные случаи request/version/result, отказ guest-скрипта на хосте. Это не VM acceptance                                                                                             |
| `bun audit --prod --json`                                                      | PASS                            | Пустой отчёт `{}`                                                                                                                                                                                           |
| Полный `bun audit --json`                                                      | **FAIL**                        | HIGH `braces` 3.0.3, GHSA-vfj7-8cjw-p6xm, только dev-цепочка shadcn; исправленной версии на момент проверки нет                                                                                             |
| Rust advisory scan                                                             | PASS с предупреждениями         | `output/release-0.3.0-cargo-audit.json`: cargo-audit 0.22.2, 0 vulnerabilities; предупреждения async-std (dev httpmock), proc-macro-error и glib (не входят в Windows-граф). База RustSec от 7 октября 2026 |
| Локальная release-сборка и запуск 0.3.0                                        | PASS базового запуска           | `output/release-0.3.0-local-build.log`: PID 59756, новый EXE 0.3.0, видимое окно, `ready`, без Windows Job, observer 23980. Полная диктовка этим не подтверждается                                          |
| Установка/запуск/удаление в Windows Sandbox                                    | PASS                            | Run `bfd3266e1fa446be9de2eef45c4b5bd0`, exact version 0.3.0, видимый HWND и удаление. SHA256 NSIS `3A96B41B76AC7F7B3F6CBEFF692B4337EA832359DF54CB92ADE47350F96BB5B8`                                        |
| Локальный неподписанный NSIS                                                   | PASS после изоляции             | `output/release-0.3.0-bundle-isolated.log`, официальный Tauri bundle в отдельном CARGO_TARGET_DIR. Финальный NSIS побайтово совпал с проверенным Sandbox artifact                                           |
| Подписанный updater, upgrade с 0.2.13, WinGet validation новой версии          | NOT RUN                         | Публикация не выполнялась; локальный неподписанный тестовый NSIS не заменяет эти проверки                                                                                                                   |
| Полная Windows/WebView2 матрица диктовки, отмены, вставки, микрофона           | NOT RUN                         | React/unit-тесты и живой процесс не подтверждают эти сценарии                                                                                                                                               |
| Сопоставимые серии RAM/latency                                                 | NOT RUN                         | Старые разрозненные измерения не объявляются актуальной серией                                                                                                                                              |

## Независимые ревью

Три read-only направления: correctness/regression, security/data safety, adversarial/test integrity. Использована GPT с явного разрешения пользователя. Авторы исправлений не принимали собственный код как независимые reviewers.

Подтверждённые гонки Compare и retention закрыты повторным независимым чтением кода. Ослабление frontend coverage и ложный Sandbox PASS устранены и повторно проверены. Существенных незакрытых замечаний к этим исправлениям не осталось. Ревью не заменяет запуск тестов и native acceptance.

## Блокеры и следующая работа

1. Довести настоящее Rust coverage до 80% содержательными тестами. Основные пробелы: `app/session.rs` 56.34% lines, `app/compare.rs` 53.83%, а также native orchestration. Порог не снижать, файлы ради зелёного отчёта не исключать. Это отдельная существенная работа, а не косметическая правка релиза.
2. Устранить dev advisory после выхода исправленной зависимости либо отдельно оформить мотивированное решение по dev-only риску. Удаление shadcn или подавление finding ради PASS не выполнено. [Официальный advisory](https://github.com/advisories/GHSA-vfj7-8cjw-p6xm).
3. Завершить обязательную native/installer/upgrade/updater матрицу и проверки финальных подписанных артефактов перед публикацией.

Причина исторических самопроизвольных завершений остаётся неизвестной: новый журнал, observer и разрешённые пользователем WER-минидампы дают доказательства для следующего события, но не восстанавливают отсутствующий старый exit code или дамп.

## Локальная упаковка и остаточное состояние

Два запуска bundler на работающем daily-driver EXE завершились FAIL с os error 32 после генерации NSIS. Tauri 2.12.1 пытается восстановить исходный EXE даже с `--no-binary-patching`; running Windows EXE заблокирован. [Исходный код Tauri 2.12.1](https://github.com/tauri-apps/tauri/blob/tauri-cli-v2.12.1/crates/tauri-bundler/src/bundle.rs#L195).

Финальная упаковка выполнена официальной командой из отдельной копии EXE, с временным `bundle.createUpdaterArtifacts=false`, `--no-sign` и `--no-binary-patching`. SHA256 копии и daily-driver совпал: `3115FE508BC7468428D08BFA24903373333C5FCB893E31D22F73FDD97053AF88`. Это локальный тестовый NSIS, без подписанного updater artifact. Итоговый NSIS имеет тот же SHA256, что первый реально проверенный Sandbox artifact; повторная VM-проверка для идентичных байтов не требуется. Повторный запуск Sandbox не дал нового guest-result и не засчитывается как PASS. Созданные этой задачей Sandbox clients закрыты по точному пути конфигурации; чужие процессы не завершались.

**Историческое состояние на момент подготовки:** `closeToTray` была временно выключена для штатного завершения старой программы и оставалась `false`. Управление окном затем отказало с `foreground window did not report a process id`, а следующая попытка сообщила остановку пользователем физическим Escape. Дальнейшие Computer Use действия были прекращены.

**Исправлено 8 октября 2026 в 12:49 MSK:** после обращения пользователя проверены runtime-журнал и observer. В 12:42:16 MSK PID 59756 получил `main_window_close`, затем `shutdown_started`, `exit_requested=0` и `exit_complete`; observer записал exit code 0. Это штатный выход по запросу закрытия окна при оставленной настройке `false`; журнал не устанавливает, кто отправил запрос. Настройка восстановлена атомарно в `true` при остановленном приложении, прочие байты настроек сохранены. Voxely 0.3.0 снова запущена: PID 59368, видимое окно, без Windows Job, observer 62604. Этот случай не объясняет исторические завершения без exit evidence.

## Дополнение: диагностика закрытия и общий shutdown owner

После восстановления настройки добавлены наблюдение lifecycle-сообщений главного HWND, загрузка/изменение close policy, решение конкретного закрытия и результаты hide/show. Отправитель `WM_CLOSE` по этим данным не устанавливается. Для локальных пересборок добавлена отдельная PID-bound idle-only команда выхода, которая не меняет пользовательские настройки.

Выход из трея, закрытие при `closeToTray=false`, обычный `ExitRequested` и собственный restart проходят единый asynchronous owner. Независимое ревью выявило и затем подтвердило исправление UI/lifecycle deadlock, преждевременной финализации и гонки local reservation со сбоем создания worker. Core отпускает lifecycle до HWND API; owner повторяет неблокирующую финализацию до прежнего 30-секундного лимита; action локального выхода резервируется до shutdown flag. Все три финальных статических ревью PASS.

- Rust all: **345 passed, 1 ignored**; Clippy all targets/features и Rust fmt – PASS. SelfTest диагностики и local lifecycle scripts – PASS в PowerShell 5.1 и 7. Полный publication gate повторно не объявляется пройденным.
- Release-сборка – PASS, 6 минут 47 секунд. SHA256 нового daily-driver EXE: `996B27A902D589C1EADC7626C9FA112587E9A67505BE70135FB8BF24A0A2C534`.
- Native базовые сценарии – PASS: hook установлен; реальный крестик дал `system_close → close → close_to_tray=true → hidden=true`, PID 63752 остался жив; повторное открытие сохранило PID; чужой PID отклонён; штатный локальный выход дал `shutdown_cleanup_ready → exit_requested=0 → exit_complete`, observer подтвердил code 0; холодный control-only запуск завершился code 2.
- Финальный запуск в 13:37 MSK: PID **66532**, видимое `Voxely (local)`, `ready`, native hook установлен, вне Windows Job; observer **35788**. `closeToTray=true`; SHA256 всех байтов настроек до и после проверки совпал. Архив только с runtime/observer metadata: `.local/diagnostics/close-lifecycle-20261008-3368d819.zip`.
- **NOT RUN live:** Busy во время реального захвата, гонка capture completion/quit и updater/restart. Управляемые backend-тесты и базовый native idle exit не заменяют эти сценарии. Историческая причина старых неожиданных завершений остаётся неизвестной.

NSIS и замороженный release-candidate выше относятся к предыдущему снимку исходников. После этой волны они не являются финальными артефактами текущего кода: перед публикацией нужны новая упаковка, актуальный полный gate и соответствующая native-приёмка. Общий статус остаётся **NOT READY**.
