use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

use crate::dsp::mic_tune::MicTune;
use crate::dsp::pipeline::DspPreset;
use crate::error::AppError;
use crate::transcription::retry::RetryPolicy;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct AppSettings {
    pub hotkey: String,
    pub start_with_windows: bool,
    pub close_to_tray: bool,
    pub notifications: bool,
    pub input_device: String,
    pub keep_original_recordings: bool,
    pub theme: String,
    pub language: String,
    pub model: String,
    pub custom_model: Option<String>,
    pub insertion_mode: String,
    pub retention: String,
    pub storage_limit: String,
    pub debug_logging: bool,
    pub retry: RetrySettings,
    pub active_preset_id: String,
    pub presets: Vec<DspPreset>,
    pub first_run_complete: bool,
    #[serde(default)]
    pub config_revision: u32,
    #[serde(default)]
    pub mic_tune: MicTune,
    #[serde(default = "default_ui_language")]
    pub ui_language: String,
    #[serde(default = "default_auto_update")]
    pub auto_update_enabled: bool,
}

fn default_ui_language() -> String {
    "auto".into()
}

fn default_auto_update() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct RetrySettings {
    pub automatic_retries: bool,
    pub additional_retries: u32,
    pub connect_timeout_ms: u64,
    pub request_timeout_ms: u64,
    pub initial_retry_delay_ms: u64,
    pub max_retry_delay_ms: u64,
    pub total_operation_timeout_ms: u64,
}

impl Default for RetrySettings {
    fn default() -> Self {
        let p = RetryPolicy::default();
        Self {
            automatic_retries: p.automatic_retries,
            additional_retries: p.additional_retries,
            connect_timeout_ms: p.connect_timeout.as_millis() as u64,
            request_timeout_ms: p.request_timeout.as_millis() as u64,
            initial_retry_delay_ms: p.initial_retry_delay.as_millis() as u64,
            max_retry_delay_ms: p.max_retry_delay.as_millis() as u64,
            total_operation_timeout_ms: p.total_operation_timeout.as_millis() as u64,
        }
    }
}

impl RetrySettings {
    pub fn validate(&self) -> Result<(), AppError> {
        if self.additional_retries > 5 {
            return Err(AppError::RequestValidationFailed(
                "additional retries must be 0-5".into(),
            ));
        }
        if self.connect_timeout_ms < 250 {
            return Err(AppError::RequestValidationFailed(
                "timeouts must be at least 250ms".into(),
            ));
        }
        if self.request_timeout_ms < 5_000 {
            return Err(AppError::RequestValidationFailed(
                "request timeout must be at least 5000ms".into(),
            ));
        }
        if self.total_operation_timeout_ms < self.request_timeout_ms {
            return Err(AppError::RequestValidationFailed(
                "total timeout must exceed request timeout".into(),
            ));
        }
        if self.total_operation_timeout_ms < 60_000 {
            return Err(AppError::RequestValidationFailed(
                "total timeout must be at least 60s".into(),
            ));
        }
        Ok(())
    }

    pub fn to_policy(&self) -> RetryPolicy {
        use std::time::Duration;
        RetryPolicy {
            automatic_retries: self.automatic_retries,
            additional_retries: self.additional_retries,
            connect_timeout: Duration::from_millis(self.connect_timeout_ms),
            request_timeout: Duration::from_millis(self.request_timeout_ms),
            initial_retry_delay: Duration::from_millis(self.initial_retry_delay_ms),
            max_retry_delay: Duration::from_millis(self.max_retry_delay_ms),
            total_operation_timeout: Duration::from_millis(self.total_operation_timeout_ms),
        }
    }
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            hotkey: "Ctrl+Shift+Space".into(),
            start_with_windows: false,
            close_to_tray: true,
            notifications: true,
            input_device: "default".into(),
            keep_original_recordings: true,
            theme: "dark".into(),
            language: "auto".into(),
            model: "openai/gpt-transcribe".into(),
            custom_model: None,
            insertion_mode: "auto".into(),
            retention: "3d".into(),
            storage_limit: "1gb".into(),
            debug_logging: false,
            retry: RetrySettings::default(),
            active_preset_id: "stt-fast".into(),
            presets: vec![
                DspPreset::stt_fast(),
                DspPreset::stt_optimized(),
                DspPreset::obs_imported(),
            ],
            first_run_complete: false,
            config_revision: 3,
            mic_tune: MicTune::default(),
            ui_language: default_ui_language(),
            auto_update_enabled: default_auto_update(),
        }
    }
}

impl AppSettings {
    pub fn load(path: &Path) -> Result<Self, AppError> {
        if !path.exists() {
            return Ok(Self::default());
        }
        let text =
            std::fs::read_to_string(path).map_err(|e| AppError::StorageFailed(e.to_string()))?;
        let mut loaded: Self =
            serde_json::from_str(&text).map_err(|e| AppError::StorageFailed(e.to_string()))?;
        let original_revision = loaded.config_revision;
        if original_revision < 1 {
            loaded.migrate_factory_defaults();
        }
        if original_revision < 2 {
            loaded.migrate_fast_stt();
        }
        if original_revision < 3 {
            loaded.migrate_long_stt_timeouts();
            loaded.config_revision = 3;
            let _ = loaded.save(path);
        }
        Ok(loaded)
    }

    pub fn migrate_factory_defaults(&mut self) {
        if self.theme == "system" {
            self.theme = "dark".into();
        }
        if self.retention == "30d" {
            self.retention = "3d".into();
        }
    }

    pub fn migrate_fast_stt(&mut self) {
        if !self.presets.iter().any(|p| p.id == "stt-fast") {
            self.presets.insert(0, DspPreset::stt_fast());
        }
        if self.active_preset_id == "stt-optimized" {
            self.active_preset_id = "stt-fast".into();
        }
    }

    pub fn migrate_long_stt_timeouts(&mut self) {
        if self.retry.request_timeout_ms < 20_000 {
            self.retry.request_timeout_ms = 20_000;
        }
        if self.retry.total_operation_timeout_ms < 180_000 {
            self.retry.total_operation_timeout_ms = 12 * 60 * 1000;
        }
        if self.retry.connect_timeout_ms < 3_000 {
            self.retry.connect_timeout_ms = 8_000;
        }
    }

    pub fn save(&self, path: &Path) -> Result<(), AppError> {
        self.retry.validate()?;
        self.mic_tune.validate()?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| AppError::StorageFailed(e.to_string()))?;
        }
        let tmp: PathBuf = path.with_extension("json.tmp");
        let json =
            serde_json::to_vec_pretty(self).map_err(|e| AppError::StorageFailed(e.to_string()))?;
        std::fs::write(&tmp, json).map_err(|e| AppError::StorageFailed(e.to_string()))?;
        std::fs::rename(tmp, path).map_err(|e| AppError::StorageFailed(e.to_string()))?;
        Ok(())
    }

    pub fn active_preset(&self) -> DspPreset {
        self.presets
            .iter()
            .find(|p| p.id == self.active_preset_id)
            .cloned()
            .unwrap_or_else(DspPreset::stt_fast)
            .apply_mic_tune(&self.mic_tune)
    }

    pub fn storage_limit_bytes(&self) -> Option<u64> {
        match self.storage_limit.as_str() {
            "500mb" => Some(500 * 1024 * 1024),
            "1gb" => Some(1024 * 1024 * 1024),
            "5gb" => Some(5 * 1024 * 1024 * 1024),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn roundtrip() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("s.json");
        let settings = AppSettings::default();
        settings.save(&path).unwrap();
        let loaded = AppSettings::load(&path).unwrap();
        assert_eq!(loaded.model, "openai/gpt-transcribe");
        assert_eq!(loaded.hotkey, "Ctrl+Shift+Space");
        assert_eq!(loaded.theme, "dark");
        assert_eq!(loaded.retention, "3d");
        assert_eq!(loaded.active_preset_id, "stt-fast");
        assert_eq!(loaded.config_revision, 3);
        assert_eq!(loaded.mic_tune, MicTune::default());
        assert_eq!(loaded.retry.request_timeout_ms, 20_000);
        assert_eq!(loaded.retry.total_operation_timeout_ms, 12 * 60 * 1000);
    }

    #[test]
    fn migrates_factory_theme_and_retention() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("s.json");
        let mut factory = AppSettings::default();
        factory.theme = "system".into();
        factory.retention = "30d".into();
        factory.config_revision = 0;
        factory.save(&path).unwrap();
        let loaded = AppSettings::load(&path).unwrap();
        assert_eq!(loaded.theme, "dark");
        assert_eq!(loaded.retention, "3d");
        assert_eq!(loaded.active_preset_id, "stt-fast");
        assert_eq!(loaded.config_revision, 3);
    }

    #[test]
    fn migrates_optimized_preset_to_fast() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("s.json");
        let mut settings = AppSettings::default();
        settings.active_preset_id = "stt-optimized".into();
        settings.presets.retain(|preset| preset.id != "stt-fast");
        settings.config_revision = 1;
        settings.save(&path).unwrap();
        let loaded = AppSettings::load(&path).unwrap();
        assert_eq!(loaded.active_preset_id, "stt-fast");
        assert!(loaded.presets.iter().any(|preset| preset.id == "stt-fast"));
    }

    #[test]
    fn migrates_short_stt_timeouts() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("s.json");
        let mut settings = AppSettings::default();
        settings.retry.request_timeout_ms = 12_000;
        settings.retry.total_operation_timeout_ms = 45_000;
        settings.config_revision = 2;
        std::fs::write(&path, serde_json::to_vec_pretty(&settings).unwrap()).unwrap();
        let loaded = AppSettings::load(&path).unwrap();
        assert_eq!(loaded.retry.request_timeout_ms, 20_000);
        assert_eq!(loaded.retry.total_operation_timeout_ms, 12 * 60 * 1000);
        assert_eq!(loaded.config_revision, 3);
    }

    #[test]
    fn retry_validation() {
        let mut s = RetrySettings::default();
        s.additional_retries = 9;
        assert!(s.validate().is_err());
    }
}
