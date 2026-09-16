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
            Self::RecordingTooLarge => "Запись слишком большая".into(),
            Self::RetryDeadlineExceeded => "Истекло время повторов".into(),
            Self::TextInsertionFailed(_) => "Не удалось вставить текст".into(),
            Self::Cancelled => "Отменено".into(),
            Self::HotkeyFailed(_) => "Не удалось зарегистрировать хоткей".into(),
            Self::IllegalTransition(_) => "Недопустимое состояние сессии".into(),
            Self::TranscriptionInProgress => "Расшифровка уже идёт".into(),
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
            Self::RetryDeadlineExceeded => "RetryDeadlineExceeded",
            Self::TextInsertionFailed(_) => "TextInsertionFailed",
            Self::Cancelled => "Cancelled",
            Self::HotkeyFailed(_) => "HotkeyFailed",
            Self::IllegalTransition(_) => "IllegalTransition",
            Self::TranscriptionInProgress => "TranscriptionInProgress",
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
        assert_eq!(AppError::RequestTimeout.code(), "RequestTimeout");
    }
}
