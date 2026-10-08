/// OpenRouter's multipart speech-to-text upload limit in decimal bytes, as
/// documented for `POST /api/v1/audio/transcriptions`.
pub(crate) const MAX_MULTIPART_AUDIO_FILE_BYTES: u64 = 25_000_000;
