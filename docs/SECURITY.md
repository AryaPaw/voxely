# Security

- API keys are stored separately in Windows Credential Manager via `keyring` under `com.voxely.desktop`. OpenRouter uses `openrouter-api-key` and OpenAI uses `openai-api-key`. SQLite, settings files, logs and IPC responses exclude both keys.
- Overlay capabilities are limited to session, cancel, meter, and settings. Compare transcripts are emitted only to the main window. Main uses custom update commands (`check_for_updates`, `install_update`); the updater plugin is not exposed to the webview.
- Asset protocol is scoped to `%APPDATA%\Voxely\audio/**` and the legacy identifier path.
- Logs omit API keys, Authorization, audio bytes, provider response bodies, and full transcripts by default.
- Cloud transcription sends prepared audio to `openrouter.ai` or `api.openai.com` according to the provider chosen when recording starts. Retries use the recorded provider. Compare uses OpenRouter. Voxely is for everyday dictation. Do not use it as the sole record for health or life-critical decisions.
- Updates: Tauri minisign signatures are required. Authenticode is deferred; SmartScreen may warn on first download. A GitHub asset digest is not a substitute for the Tauri updater signature.
- Residual risk: unsigned Authenticode, missing GitHub repo/origin until authorized, private updater key stored only in GitHub Secrets after setup.
