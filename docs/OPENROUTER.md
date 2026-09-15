# OpenRouter

Endpoint: `POST https://openrouter.ai/api/v1/audio/transcriptions` as multipart `file` + `model`.

Default model: `openai/gpt-transcribe`.

Model discovery: `GET /api/v1/models?output_modalities=transcription`.

WAV is preferred for short dictation. Multipart limit 25 MB. App guard is 20 MB.

The app keeps one HTTP/1.1 rustls client (`OpenRouterTransport`) with up to two idle connections. Live STT, manual retry, Compare, and Test connection share it.

Response fields stored: `text`, `usage`, `cost`, `X-Generation-Id`, latency, attempt count.

Live tests require `OPENROUTER_API_KEY` and are not part of `bun run verify`.
