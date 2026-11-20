pub(crate) mod limits;
pub mod openrouter;
pub mod retry;

use serde::{Deserialize, Serialize};

pub(crate) const OPENAI_MODEL: &str = "gpt-transcribe";

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TranscriptionProvider {
    #[default]
    OpenRouter,
    OpenAi,
}

impl TranscriptionProvider {
    pub(crate) fn id(self) -> &'static str {
        match self {
            Self::OpenRouter => "openrouter",
            Self::OpenAi => "openai",
        }
    }

    pub(crate) fn from_recording(value: &str) -> Result<Self, crate::error::AppError> {
        match value {
            "openrouter" => Ok(Self::OpenRouter),
            "openai" => Ok(Self::OpenAi),
            _ => Err(crate::error::AppError::RequestValidationFailed(
                "unknown recording provider".into(),
            )),
        }
    }

    pub(crate) fn base_url(self) -> &'static str {
        match self {
            Self::OpenRouter => openrouter::default_base_url(),
            Self::OpenAi => "https://api.openai.com/v1",
        }
    }

    pub(crate) fn model(self, openrouter_model: &str) -> &str {
        match self {
            Self::OpenRouter => openrouter_model,
            Self::OpenAi => OPENAI_MODEL,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn legacy_recordings_and_settings_keep_openrouter_and_retry_provider_is_explicit() {
        let mut previous = serde_json::to_value(crate::settings::AppSettings::default()).unwrap();
        previous
            .as_object_mut()
            .unwrap()
            .remove("transcriptionProvider");
        let restored: crate::settings::AppSettings = serde_json::from_value(previous).unwrap();
        assert_eq!(
            restored.transcription_provider,
            TranscriptionProvider::OpenRouter
        );
        let recording = crate::history::repository::new_recording(restored.model.clone());
        assert_eq!(
            TranscriptionProvider::from_recording(&recording.provider).unwrap(),
            TranscriptionProvider::OpenRouter
        );
        assert_eq!(
            TranscriptionProvider::from_recording("openai").unwrap(),
            TranscriptionProvider::OpenAi
        );
        assert!(TranscriptionProvider::from_recording("unknown").is_err());
    }
}
