# Architecture

Voxely is a Windows-first Tauri 2 desktop app. The WebView is only UI. Recording, DSP, SQLite, OpenRouter, hotkeys, tray, and text insertion live in Rust.

## Data flow

1. Global hotkey Pressed (not Released, not key repeat) toggles the session state machine.
2. Capture callback writes PCM into a bounded ring buffer. A writer thread persists WAV. No network in the audio callback.
3. After stop, a DSP worker reads raw audio, writes processed WAV, then OpenRouter multipart STT runs with a bounded retry scheduler.
4. Processed WAV is the retry source of truth.
5. Text insertion uses the HWND captured at record start. If the foreground window changed, History is updated and nothing is typed into a new app.
6. Session generation invalidates late STT results after cancel or a new start.
7. Updates use the official Tauri updater. Install is deferred while dictation is cancellable.

## Domains

## Domains

- `app`: lifecycle, tray, hotkey, session machine
- `audio`: devices, capture, resample, WAV
- `dsp`: filters, presets, OBS mapping
- `transcription`: OpenRouter client + retry policy
- `history`: SQLite + retention
- `windows_int`: credentials, injector
- `updates`: Tauri updater coordinator and version policy
- `obs`: local scene collection import

## Overlay

Created as a frameless always-on-top window that is not focused. Size is 360x48 logical. Overlay IPC is limited to session state, cancel, meter, and settings.

Main UI is split by screen: `MainApp` shell, `sectionNav`, `sections/*`, `history/*`. Strings live in `src/lib/i18n.ts`.
