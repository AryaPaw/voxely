# Security

- OpenRouter API key: Windows Credential Manager via `keyring` (`com.voxely.desktop` / `openrouter-api-key`). Never SQLite, settings.json, logs, or IPC responses.
- Overlay capabilities are limited to session, cancel, meter, and settings. Compare transcripts are emitted only to the main window. Main uses custom update commands (`check_for_updates`, `install_update`); the updater plugin is not exposed to the webview.
- Asset protocol is scoped to `%APPDATA%\Voxely\audio/**` and the legacy identifier path.
- Logs omit API keys, Authorization, audio bytes, provider response bodies, and full transcripts by default.
- Voxely is consumer Windows dictation, not a medical device or clinical decision system. Cloud STT sends audio to OpenRouter. Do not use it as the sole record for health or life-critical decisions.
- Updates: Tauri minisign signatures are required. Authenticode is deferred; SmartScreen may warn on first download. A GitHub asset digest is not a substitute for the Tauri updater signature.
- Residual risk: unsigned Authenticode, missing GitHub repo/origin until authorized, private updater key stored only in GitHub Secrets after setup.
