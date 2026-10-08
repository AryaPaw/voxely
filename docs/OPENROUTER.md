# OpenRouter

Endpoint: `POST https://openrouter.ai/api/v1/audio/transcriptions` as multipart `file` + `model`.

Default model: `openai/gpt-transcribe`.

Model discovery: `GET /api/v1/models?output_modalities=transcription`.

Multipart transcription uploads are limited to 25,000,000 bytes. Voxely derives its
48 kHz mono PCM16 capture limit from this cap; the resulting 16 kHz STT WAV can
reach 25,000,000 bytes (about 13 minutes of audio). The source capture WAV is
about 75 MB because it uses three times the STT sample rate.

OpenRouter documents a 60-second upstream provider timeout. Larger audio files
fit the upload size cap but may still time out during transcription; split long
recordings if that happens.

Voxely's default local deadline for the complete transcription and retry
operation is 12 minutes. This deadline is independent of the upload size cap
and the upstream provider timeout.

The app keeps one HTTP/1.1 rustls client (`OpenRouterTransport`) with up to two idle connections. Live STT, manual retry, Compare, and Test connection share it.

Response fields stored: `text`, `usage`, `cost`, `X-Generation-Id`, latency, attempt count.

Live tests require `OPENROUTER_API_KEY` and are not part of `bun run verify`.
