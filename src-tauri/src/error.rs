use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Clone, Error, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "code", content = "detail")]
pub enum AppError {
    #[error("Microphone is unavailable")]
    MicrophoneUnavailable,
    #[error("Audio capture failed: {0}")]
    AudioCaptureFailed(String),
    #[error("Audio processing failed: {0}")]
    AudioProcessingFailed(String),
    #[error("Storage failed: {0}")]
    StorageFailed(String),
    #[error("Invalid API key")]
    InvalidApiKey,
    #[error("Invalid model: {0}")]
    InvalidModel(String),
    #[error("Request validation failed: {0}")]
    RequestValidationFailed(String),
    #[error("Network unavailable")]
    NetworkUnavailable,
    #[error("Connection failed: {0}")]
    ConnectionFailed(String),
    #[error("Request timed out")]
    RequestTimeout,
    #[error("Rate limited")]
    RateLimited,
    #[error("Provider unavailable")]
    ProviderUnavailable,
    #[error("OpenRouter server error")]
    OpenRouterServerError,
    #[error("Malformed response")]
    ResponseMalformed,
    #[error("Recording is too large to send")]
    RecordingTooLarge,
    #[error("Recording was truncated by the capture size or buffer limit")]
    RecordingTruncated,
    #[error("Retry deadline exceeded")]
    RetryDeadlineExceeded,
    #[error("Text insertion failed: {0}")]
    TextInsertionFailed(String),
    #[error("Cancelled")]
    Cancelled,
    #[error("Hotkey registration failed: {0}")]
    HotkeyFailed(String),
    #[error("Illegal session transition: {0}")]
    IllegalTransition(String),
    #[error("Transcription is already running")]
    TranscriptionInProgress,
    #[error("First-run disclosure must be acknowledged before recording")]
    DisclosureRequired,
    #[error("Recording was interrupted")]
    Interrupted,
}

impl AppError {
    pub fn user_message(&self) -> String {
        match self {
            Self::MicrophoneUnavailable => "Микрофон недоступен".into(),
            Self::AudioCaptureFailed(_) => "Не удалось записать звук".into(),
            Self::AudioProcessingFailed(_) => "Не удалось обработать запись".into(),
            Self::StorageFailed(_) => "Не удалось сохранить данные".into(),
            Self::InvalidApiKey => "Нет API-ключа OpenRouter".into(),
            Self::InvalidModel(_) => "Неверная модель".into(),
            Self::RequestValidationFailed(_) => "Неверный запрос".into(),
            Self::NetworkUnavailable => "Нет сети".into(),
            Self::ConnectionFailed(_) => "Нет соединения с OpenRouter".into(),
            Self::RequestTimeout => "Превышено время ожидания".into(),
            Self::RateLimited => "Слишком много запросов".into(),
            Self::ProviderUnavailable => "Провайдер недоступен".into(),
            Self::OpenRouterServerError => "Ошибка сервера OpenRouter".into(),
            Self::ResponseMalformed => "Некорректный ответ".into(),
            Self::RecordingTooLarge => "Размер записи превышает лимит отправки".into(),
            Self::RecordingTruncated => "Запись обрезана из-за лимита размера или буфера".into(),
            Self::RetryDeadlineExceeded => "Истекло время повторов".into(),
            Self::TextInsertionFailed(_) => "Не удалось вставить текст".into(),
            Self::Cancelled => "Отменено".into(),
            Self::HotkeyFailed(_) => "Не удалось зарегистрировать хоткей".into(),
            Self::IllegalTransition(_) => "Недопустимое состояние сессии".into(),
            Self::TranscriptionInProgress => "Расшифровка уже идёт".into(),
            Self::DisclosureRequired => {
                "Перед записью подтвердите сведения о передаче аудио и хранении данных".into()
            }
            Self::Interrupted => "Запись прервана. Можно повторить расшифровку".into(),
        }
    }

    pub fn code(&self) -> &'static str {
        match self {
            Self::MicrophoneUnavailable => "MicrophoneUnavailable",
            Self::AudioCaptureFailed(_) => "AudioCaptureFailed",
            Self::AudioProcessingFailed(_) => "AudioProcessingFailed",
            Self::StorageFailed(_) => "StorageFailed",
            Self::InvalidApiKey => "InvalidApiKey",
            Self::InvalidModel(_) => "InvalidModel",
            Self::RequestValidationFailed(_) => "RequestValidationFailed",
            Self::NetworkUnavailable => "NetworkUnavailable",
            Self::ConnectionFailed(_) => "ConnectionFailed",
            Self::RequestTimeout => "RequestTimeout",
            Self::RateLimited => "RateLimited",
            Self::ProviderUnavailable => "ProviderUnavailable",
            Self::OpenRouterServerError => "OpenRouterServerError",
            Self::ResponseMalformed => "ResponseMalformed",
            Self::RecordingTooLarge => "RecordingTooLarge",
            Self::RecordingTruncated => "RecordingTruncated",
            Self::RetryDeadlineExceeded => "RetryDeadlineExceeded",
            Self::TextInsertionFailed(_) => "TextInsertionFailed",
            Self::Cancelled => "Cancelled",
            Self::HotkeyFailed(_) => "HotkeyFailed",
            Self::IllegalTransition(_) => "IllegalTransition",
            Self::TranscriptionInProgress => "TranscriptionInProgress",
            Self::DisclosureRequired => "DisclosureRequired",
            Self::Interrupted => "Interrupted",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::AppError;

    #[test]
    fn code_matches_variant_name() {
        assert_eq!(AppError::Cancelled.code(), "Cancelled");
        assert_eq!(
            AppError::TranscriptionInProgress.code(),
            "TranscriptionInProgress"
        );
        assert_eq!(AppError::Interrupted.code(), "Interrupted");
        assert_eq!(AppError::RecordingTruncated.code(), "RecordingTruncated");
        assert_eq!(AppError::RequestTimeout.code(), "RequestTimeout");
    }

    #[test]
    fn every_error_has_a_stable_serialized_code_and_safe_user_message() {
        const PRIVATE_DETAIL: &str = "private provider token and local path";
        let cases = [
            (AppError::MicrophoneUnavailable, "MicrophoneUnavailable"),
            (
                AppError::AudioCaptureFailed(PRIVATE_DETAIL.into()),
                "AudioCaptureFailed",
            ),
            (
                AppError::AudioProcessingFailed(PRIVATE_DETAIL.into()),
                "AudioProcessingFailed",
            ),
            (
                AppError::StorageFailed(PRIVATE_DETAIL.into()),
                "StorageFailed",
            ),
            (AppError::InvalidApiKey, "InvalidApiKey"),
            (
                AppError::InvalidModel(PRIVATE_DETAIL.into()),
                "InvalidModel",
            ),
            (
                AppError::RequestValidationFailed(PRIVATE_DETAIL.into()),
                "RequestValidationFailed",
            ),
            (AppError::NetworkUnavailable, "NetworkUnavailable"),
            (
                AppError::ConnectionFailed(PRIVATE_DETAIL.into()),
                "ConnectionFailed",
            ),
            (AppError::RequestTimeout, "RequestTimeout"),
            (AppError::RateLimited, "RateLimited"),
            (AppError::ProviderUnavailable, "ProviderUnavailable"),
            (AppError::OpenRouterServerError, "OpenRouterServerError"),
            (AppError::ResponseMalformed, "ResponseMalformed"),
            (AppError::RecordingTooLarge, "RecordingTooLarge"),
            (AppError::RecordingTruncated, "RecordingTruncated"),
            (AppError::RetryDeadlineExceeded, "RetryDeadlineExceeded"),
            (
                AppError::TextInsertionFailed(PRIVATE_DETAIL.into()),
                "TextInsertionFailed",
            ),
            (AppError::Cancelled, "Cancelled"),
            (
                AppError::HotkeyFailed(PRIVATE_DETAIL.into()),
                "HotkeyFailed",
            ),
            (
                AppError::IllegalTransition(PRIVATE_DETAIL.into()),
                "IllegalTransition",
            ),
            (AppError::TranscriptionInProgress, "TranscriptionInProgress"),
            (AppError::DisclosureRequired, "DisclosureRequired"),
            (AppError::Interrupted, "Interrupted"),
        ];
        for (error, code) in cases {
            let encoded = serde_json::to_value(&error).unwrap();
            assert_eq!(encoded["code"], code);
            assert_eq!(error.code(), code);
            assert_eq!(serde_json::from_value::<AppError>(encoded).unwrap(), error);
            let message = error.user_message();
            assert!(!message.trim().is_empty(), "{code}");
            assert!(!message.contains(PRIVATE_DETAIL), "{code}");
            assert!(!message.chars().any(char::is_control), "{code}");
        }
    }
}
