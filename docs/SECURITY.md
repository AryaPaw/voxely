# Security

- OpenRouter API key: Windows Credential Manager via `keyring` (`com.voxely.desktop` / `openrouter-api-key`). Never SQLite, settings.json, logs, or IPC responses.
- Tauri CSP disallows remote script. Overlay capabilities are limited to session, cancel, meter, and settings. Main has the remaining commands.
- Asset protocol is scoped to `%APPDATA%\Voxely\audio/**`.
- Commands validate retry bounds and refuse concurrent jobs per recording id.
- Settings JSON is parsed as data, not executed.
- Logs omit API keys, Authorization, audio bytes, and full transcripts by default. `open_logs` opens `%APPDATA%\Voxely\logs`.
- Updates: Tauri minisign signatures are required. Authenticode is deferred; SmartScreen may warn on first download. A GitHub asset digest is not a substitute for the Tauri updater signature.
- Residual risk: unsigned Authenticode, missing GitHub repo/origin until authorized, private updater key stored only in GitHub Secrets after setup.
