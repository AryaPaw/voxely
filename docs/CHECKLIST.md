# Implementation checklist

- [x] Official ECC Cursor install (`install.ps1 --target cursor typescript rust`) + `ecc doctor --target cursor` OK
- [x] Tauri 2 + React + TypeScript + Tailwind
- [x] State machine + retry scheduler with tests
- [x] Capture/DSP/OpenRouter/History/insertion modules
- [x] OBS read-only audit in `.local/obs-audio-audit.md`
- [x] OBS Imported + STT Optimized presets
- [ ] Native `cargo test` / `tauri build` on a machine with MSVC `link.exe`
- [ ] Overlay latency measurement <150 ms
- [ ] Live OpenRouter opt-in fixture
- [ ] App insertion matrix (Notepad, Chrome, Cursor, VS Code, Telegram, Discord, Word)
- [ ] WebdriverIO E2E
