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
    #[serde(default = "default_compare_models")]
    pub compare_models: Vec<String>,
    #[serde(default)]
    pub write_seq: u64,
}

fn default_ui_language() -> String {
    "auto".into()
}

fn default_auto_update() -> bool {
    true
}

fn default_compare_models() -> Vec<String> {
    vec![
        "openai/gpt-transcribe".into(),
        "openai/whisper-large-v3".into(),
    ]
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
        if self.connect_timeout_ms < 8_000 {
            return Err(AppError::RequestValidationFailed(
                "connect timeout must be at least 8000ms".into(),
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
            insertion_mode: "unicode".into(),
            retention: "3d".into(),
            storage_limit: "1gb".into(),
            debug_logging: false,
            retry: RetrySettings::default(),
            active_preset_id: "stt-optimized".into(),
            presets: vec![
                DspPreset::stt_fast(),
                DspPreset::stt_optimized(),
                DspPreset::obs_imported(),
            ],
            first_run_complete: false,
            config_revision: 5,
            mic_tune: MicTune::default(),
            ui_language: default_ui_language(),
            auto_update_enabled: default_auto_update(),
            compare_models: default_compare_models(),
            write_seq: 0,
        }
    }
}

impl AppSettings {
    pub fn reset_user_settings(keep_first_run_complete: bool) -> Self {
        let mut settings = Self::default();
        settings.first_run_complete = keep_first_run_complete;
        settings
    }

    pub fn load(path: &Path) -> Result<Self, AppError> {
        Ok(Self::load_with_recovery(path)?.0)
    }

    pub fn load_with_recovery(path: &Path) -> Result<(Self, bool), AppError> {
        if !path.exists() {
            return Ok((Self::default(), false));
        }
        let text =
            std::fs::read_to_string(path).map_err(|e| AppError::StorageFailed(e.to_string()))?;
        let mut loaded: Self = match serde_json::from_str(&text) {
            Ok(value) => value,
            Err(_) => {
                let backup = path.with_extension("json.corrupt");
                let _ = std::fs::write(&backup, &text);
                return Ok((Self::default(), true));
            }
        };
        let original_revision = loaded.config_revision;
        if original_revision < 5 {
            if original_revision < 4 && path.exists() {
                let backup = path.with_extension("json.bak");
                let _ = std::fs::copy(path, backup);
            }
            if original_revision < 1 {
                loaded.migrate_factory_defaults();
            }
            if original_revision < 2 {
                loaded.migrate_fast_stt();
            }
            if original_revision < 3 {
                loaded.migrate_long_stt_timeouts();
            }
            if original_revision < 4 {
                loaded.migrate_honest_dsp_and_insert();
            }
            if original_revision < 5 {
                loaded.migrate_stock_fast_to_quality();
            }
            loaded.config_revision = 5;
            let _ = loaded.save(path);
        }
        if loaded.apply_connect_timeout_floor() {
            let _ = loaded.save(path);
        }
        Ok((loaded, false))
    }

    pub fn migrate_factory_defaults(&mut self) {
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
        if self.retry.connect_timeout_ms < 8_000 {
            self.retry.connect_timeout_ms = 8_000;
        }
    }

    pub fn apply_connect_timeout_floor(&mut self) -> bool {
        if self.retry.connect_timeout_ms < 8_000 {
            self.retry.connect_timeout_ms = 8_000;
            true
        } else {
            false
        }
    }

    pub fn migrate_stock_fast_to_quality(&mut self) {
        let legacy_quality = DspPreset::stt_optimized_revision_4();
        let factory_fast = DspPreset::stt_fast();
        for preset in &mut self.presets {
            if preset.id == "stt-optimized" && *preset == legacy_quality {
                *preset = DspPreset::stt_optimized();
            }
        }
        if self.active_preset_id == "stt-fast" {
            let stock_fast = self.presets.iter().any(|preset| {
                preset.id == "stt-fast"
                    && (*preset == factory_fast
                        || *preset
                            == factory_fast
                                .apply_mic_tune(&crate::dsp::mic_tune::MicTune::default()))
            });
            if stock_fast {
                self.active_preset_id = "stt-optimized".into();
            }
        }
    }

    pub fn migrate_honest_dsp_and_insert(&mut self) {
        if self.insertion_mode == "auto" || self.insertion_mode == "sendinput" {
            self.insertion_mode = "unicode".into();
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
            .unwrap_or_else(DspPreset::stt_optimized)
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
    fn corrupt_settings_are_backed_up_and_defaults_load() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("s.json");
        std::fs::write(&path, "{not json").unwrap();
        let (loaded, recovered) = AppSettings::load_with_recovery(&path).unwrap();
        assert!(recovered);
        assert_eq!(loaded.model, AppSettings::default().model);
        assert!(path.with_extension("json.corrupt").exists());
        let original = std::fs::read_to_string(&path).unwrap();
        assert_eq!(original, "{not json");
    }

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
        assert_eq!(loaded.active_preset_id, "stt-optimized");
        assert_eq!(loaded.config_revision, 5);
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
        assert_eq!(loaded.theme, "system");
        assert_eq!(loaded.retention, "3d");
        assert_eq!(loaded.active_preset_id, "stt-optimized");
        assert_eq!(loaded.config_revision, 5);
    }

    #[test]
    fn migrates_optimized_preset_to_fast() {
        let mut settings = AppSettings::default();
        settings.active_preset_id = "stt-optimized".into();
        settings.presets.retain(|preset| preset.id != "stt-fast");
        settings.migrate_fast_stt();
        assert_eq!(settings.active_preset_id, "stt-fast");
        assert!(settings
            .presets
            .iter()
            .any(|preset| preset.id == "stt-fast"));
    }

    #[test]
    fn revision_5_moves_stock_fast_to_quality() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("s.json");
        let mut settings = AppSettings::default();
        settings.active_preset_id = "stt-fast".into();
        settings.presets = vec![
            DspPreset::stt_fast(),
            DspPreset::stt_optimized_revision_4(),
            DspPreset::obs_imported(),
        ];
        settings.config_revision = 4;
        std::fs::write(&path, serde_json::to_vec_pretty(&settings).unwrap()).unwrap();
        let loaded = AppSettings::load(&path).unwrap();
        assert_eq!(loaded.active_preset_id, "stt-optimized");
        assert_eq!(loaded.config_revision, 5);
        let quality = loaded
            .presets
            .iter()
            .find(|preset| preset.id == "stt-optimized")
            .unwrap();
        assert!(!quality
            .order
            .iter()
            .any(|slot| slot.kind == crate::dsp::pipeline::FilterKind::Expander && slot.enabled));
        assert!((quality.rnnoise_mix - 0.6).abs() < f32::EPSILON);
    }

    #[test]
    fn revision_5_keeps_custom_fast() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("s.json");
        let mut settings = AppSettings::default();
        settings.active_preset_id = "stt-fast".into();
        settings.presets[0].gain.db = 9.0;
        settings.config_revision = 4;
        std::fs::write(&path, serde_json::to_vec_pretty(&settings).unwrap()).unwrap();
        let loaded = AppSettings::load(&path).unwrap();
        assert_eq!(loaded.active_preset_id, "stt-fast");
        assert_eq!(loaded.config_revision, 5);
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
        assert_eq!(loaded.config_revision, 5);
    }

    #[test]
    fn lifts_sub_eight_second_connect_timeout_on_current_revision() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("s.json");
        let mut settings = AppSettings::default();
        settings.retry.connect_timeout_ms = 4_994;
        settings.config_revision = 5;
        std::fs::write(&path, serde_json::to_vec_pretty(&settings).unwrap()).unwrap();
        let loaded = AppSettings::load(&path).unwrap();
        assert_eq!(loaded.retry.connect_timeout_ms, 8_000);
        let persisted: AppSettings =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(persisted.retry.connect_timeout_ms, 8_000);
    }

    #[test]
    fn factory_preset_graphs_differ() {
        let fast = DspPreset::stt_fast();
        let quality = DspPreset::stt_optimized();
        assert_ne!(fast.order, quality.order);
        let settings = AppSettings::default();
        assert_eq!(settings.active_preset().order, quality.order);
    }

    #[test]
    fn migrates_insert_aliases_to_unicode() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("s.json");
        let mut settings = AppSettings::default();
        settings.insertion_mode = "sendinput".into();
        settings.config_revision = 3;
        std::fs::write(&path, serde_json::to_vec_pretty(&settings).unwrap()).unwrap();
        let loaded = AppSettings::load(&path).unwrap();
        assert_eq!(loaded.insertion_mode, "unicode");
        assert!(path.with_extension("json.bak").exists());
    }

    #[test]
    fn retry_rejects_too_many_attempts() {
        let mut s = RetrySettings::default();
        s.additional_retries = 9;
        assert!(s.validate().is_err());
        s = RetrySettings::default();
        s.connect_timeout_ms = 4_994;
        assert!(s.validate().is_err());
    }

    #[test]
    fn reset_user_settings_restores_factory_and_keeps_first_run() {
        let reset = AppSettings::reset_user_settings(true);
        assert_eq!(reset.hotkey, AppSettings::default().hotkey);
        assert_eq!(reset.theme, AppSettings::default().theme);
        assert_eq!(reset.active_preset_id, "stt-optimized");
        assert_eq!(reset.presets.len(), 3);
        assert!(reset.first_run_complete);
        let full = AppSettings::reset_user_settings(false);
        assert!(!full.first_run_complete);
    }
}
