# Contributing

Voxely is a Windows Tauri 2 app. The WebView is UI only. Recording, DSP, history, hotkeys, tray, and text insertion live in Rust.

## Setup

- Windows 11 x64
- Bun 1.4+
- Rust stable
- Visual Studio 2022 Build Tools with C++ (`link.exe`)

```text
bun install
bun run tauri dev
bun run verify
```

Or `.\scripts\dev.ps1`.

## Rules that matter

- Do not commit secrets. The OpenRouter key belongs in Windows Credential Manager.
- Insertion modes are `unicode` and `clipboard`. Do not revive `auto` or `sendinput`.
- `AppSettings::active_preset()` returns the stored preset. Do not rewrite `order` from MicTune.
- Overlay HUD follows the app theme.
- Keep user-facing copy in `src/lib/i18n.ts` for both `ru` and `en`.

## Docs

- [Architecture](docs/ARCHITECTURE.md)
- [Release](docs/RELEASE.md)
- [Security](docs/SECURITY.md)
- [OpenRouter](docs/OPENROUTER.md)
- [Testing](docs/TESTING.md)

License: [AGPL-3.0](LICENSE).
