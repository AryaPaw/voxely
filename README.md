# Voxely

Windows voice dictation. Global hotkey `Ctrl+Shift+Space`, tray-first, OpenRouter `openai/gpt-transcribe`, bounded retries.

## Requirements

- Windows 11 x64
- Bun 1.4+
- Rust stable
- Visual Studio 2022 Build Tools with C++ (`link.exe`)

## Commands

```
bun install
bun run tauri dev
bun run verify
```

From this repo with MSVC in a new shell:

```powershell
cmd /c "call `"C:\Program Files (x86)\Microsoft Visual Studio\2022\BuildTools\VC\Auxiliary\Build\vcvars64.bat`" && bun run tauri dev"
```

Or `.\scripts\dev.ps1`.

API key is stored in Windows Credential Manager, not in git or SQLite.

License: AGPL-3.0 (see `LICENSE`).
