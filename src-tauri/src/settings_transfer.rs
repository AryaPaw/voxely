//! Versioned settings interchange. Credentials and local lifecycle state never enter the file.
use crate::{
    error::AppError,
    settings::{validate_settings_graph, AppSettings},
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::io::Read;

pub const MAX_BYTES: usize = 1024 * 1024;
const LOCAL_FIELDS: &[&str] = &[
    "writeSeq",
    "configRevision",
    "firstRunComplete",
    "retentionPolicyConfirmed",
];

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct SettingsFile {
    format: String,
    version: u32,
    settings: Value,
}

fn portable(settings: &AppSettings) -> Result<Value, AppError> {
    let mut value = serde_json::to_value(settings).map_err(invalid)?;
    for field in LOCAL_FIELDS {
        value
            .as_object_mut()
            .expect("settings object")
            .remove(*field);
    }
    Ok(value)
}

fn invalid(error: impl std::fmt::Display) -> AppError {
    AppError::RequestValidationFailed(format!("Invalid Voxely settings file: {error}"))
}

pub fn export(settings: &AppSettings) -> Result<String, AppError> {
    serde_json::to_string_pretty(&SettingsFile {
        format: "voxely-settings".into(),
        version: 1,
        settings: portable(settings)?,
    })
    .map_err(invalid)
}

pub fn read(reader: impl Read) -> Result<String, AppError> {
    let mut bytes = Vec::new();
    reader
        .take((MAX_BYTES + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|err| AppError::StorageFailed(err.to_string()))?;
    if bytes.len() > MAX_BYTES {
        return Err(invalid("file exceeds 1 MiB"));
    }
    String::from_utf8(bytes).map_err(invalid)
}

pub fn import(text: &str, current: &AppSettings) -> Result<AppSettings, AppError> {
    if text.len() > MAX_BYTES {
        return Err(invalid("file exceeds 1 MiB"));
    }
    let file: SettingsFile =
        serde_json::from_str(text.trim_start_matches('\u{feff}')).map_err(invalid)?;
    if file.format != "voxely-settings" || file.version != 1 {
        return Err(invalid("unsupported format or version"));
    }
    let fields = file
        .settings
        .as_object()
        .ok_or_else(|| invalid("settings must be an object"))?;
    let allowed = portable(&AppSettings::default())?;
    if fields.is_empty() || fields.keys().any(|key| allowed.get(key).is_none()) {
        return Err(invalid("empty settings or unknown fields"));
    }
    // Merge missing fields with current values, not defaults from another installation.
    let mut merged = serde_json::to_value(current).map_err(invalid)?;
    merged
        .as_object_mut()
        .expect("settings object")
        .extend(fields.clone());
    let mut next: AppSettings = serde_json::from_value(merged).map_err(invalid)?;
    // Retention changes can delete recordings and use their own preview/confirmation workflow.
    next.retention = current.retention.clone();
    next.storage_limit = current.storage_limit.clone();
    next.write_seq = 0;
    next.retry.validate()?;
    next.mic_tune.validate()?;
    validate_settings_graph(&next)?;
    crate::app::shortcuts::parse_hotkey(&next.hotkey)?;
    Ok(next)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_preserves_user_settings_and_local_state() {
        let mut source = AppSettings::default();
        source.theme = "light".into();
        source.text_replacements.enabled = true;
        source.silence_compression.enabled = false;
        source.silence_compression.max_pause_ms = 1500;
        source.write_seq = 999;
        let text = export(&source).unwrap();
        for field in LOCAL_FIELDS {
            assert!(!text.contains(field));
        }
        assert!(!text.contains("apiKey"));
        let mut current = AppSettings::default();
        current.first_run_complete = true;
        current.retention = "forever".into();
        current.storage_limit = "10gb".into();
        let result = import(&text, &current).unwrap();
        assert_eq!(result.theme, "light");
        assert_eq!(result.presets, source.presets);
        assert_eq!(result.text_replacements, source.text_replacements);
        assert_eq!(result.silence_compression, source.silence_compression);
        assert!(result.first_run_complete);
        assert_eq!(result.retention, "forever");
        assert_eq!(result.storage_limit, "10gb");
        assert_eq!(result.write_seq, 0);
    }

    #[test]
    fn rejects_invalid_files_without_mutating_current() {
        let current = AppSettings::default();
        for text in [
            "garbage",
            "{}",
            r#"{"format":"voxely-settings","version":2,"settings":{}}"#,
            r#"{"format":"voxely-settings","version":1,"settings":{"apiKey":"secret"}}"#,
            r#"{"format":"voxely-settings","version":1,"settings":{"writeSeq":5}}"#,
            r#"{"format":"voxely-settings","version":1,"settings":{"theme":"bad"}}"#,
            r#"{"format":"voxely-settings","version":1,"settings":{"hotkey":"invalid"}}"#,
            r#"{"format":"voxely-settings","version":1,"settings":{"presets":[]}}"#,
        ] {
            assert!(import(text, &current).is_err(), "accepted {text}");
        }
        assert_eq!(current, AppSettings::default());
    }

    #[test]
    fn accepts_partial_and_bom_and_bounds_reads() {
        let current = AppSettings::default();
        let text = "\u{feff}{\"format\":\"voxely-settings\",\"version\":1,\"settings\":{\"theme\":\"light\"}}";
        let next = import(text, &current).unwrap();
        assert_eq!(next.theme, "light");
        assert_eq!(next.hotkey, current.hotkey);
        assert!(read(std::io::Cursor::new(vec![b'a'; MAX_BYTES + 1])).is_err());
        assert!(read(std::io::Cursor::new(vec![255])).is_err());
        assert_eq!(read(text.as_bytes()).unwrap(), text);
    }

    #[test]
    fn settings_transfer_commands_are_registered_and_restricted_to_main() {
        let manifest = include_str!("../build.rs");
        let handlers = include_str!("lib.rs");
        let main: Value =
            serde_json::from_str(include_str!("../capabilities/default.json")).unwrap();
        let overlay: Value =
            serde_json::from_str(include_str!("../capabilities/overlay.json")).unwrap();
        for command in [
            "export_settings_file",
            "read_settings_file",
            "import_settings",
        ] {
            assert!(manifest.contains(&format!("\"{command}\"")));
            assert!(handlers.contains(&format!("            {command},")));
            let permission = Value::String(format!("allow-{}", command.replace('_', "-")));
            assert!(main["permissions"]
                .as_array()
                .unwrap()
                .contains(&permission));
            assert!(!overlay["permissions"]
                .as_array()
                .unwrap()
                .contains(&permission));
        }
    }
}
