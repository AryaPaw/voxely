# Security

- OpenRouter API key: Windows Credential Manager via `keyring` (`com.voxely.desktop` / `openrouter-api-key`). Never SQLite, settings.json, logs, or IPC responses.
- Tauri CSP disallows remote script. Overlay and main share a tight capability set.
- Commands validate retry bounds and refuse concurrent jobs per recording id.
- OBS JSON is parsed as data, not executed.
- Logs omit API keys, Authorization, audio bytes, and full transcripts by default.
