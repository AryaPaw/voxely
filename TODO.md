# TODO

После волны 0.2.11 и долга из post-remediation плана закрыты авто-стоп лимита записи, busy-check рестарта обновления, cancel history retry, явный fallback микрофона, bounded join, Compare slot/run ids и quota temp WAV, MicTune больше не переписывает DSP SSOT, Chromium идёт через UTF-16 Unicode, coverage включает Main/History/Compare/Filter, `make_latest` только после installer smoke.

Остаётся вне этого кода:

- Native matrix на daily-driver `src-tauri/target/debug/voxely.exe` (см. `scripts/runtime-matrix.ps1`)
- Installer/Sandbox отдельно от host
- Authenticode
- Clinical/SaMD: отдельное продуктовое решение, не bugfix
