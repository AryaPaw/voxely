# Testing

`bun run verify` runs frontend typecheck, lint, format, coverage and Rust fmt/clippy/tests with `--locked` when the MSVC linker is available.

Coverage include: `src/lib` product modules (session copy, i18n, overlay wave, utils, theme) and shadcn primitives.

Layers:

1. Unit/integration on every PR (`bun run verify`, `cargo test --locked`).
2. App E2E (Tauri/WebdriverIO) is still not in the default gate.
3. CI installer smoke on GitHub `windows-2022` after a signed NSIS build. This is not Windows Sandbox.
4. Windows Sandbox (`scripts/run-windows-sandbox.ps1`): required for claiming install/uninstall. If Sandbox is unavailable, status is `BLOCKED`.
5. Host runtime: live dictation after Sandbox PASS.

Do not run live OpenRouter tests in default CI.
