use crate::settings::AppSettings;
use std::{collections::BTreeMap, sync::OnceLock};

type Dictionary = BTreeMap<String, String>;
static LOCALES: OnceLock<Result<[Dictionary; 2], serde_json::Error>> = OnceLock::new();

pub(crate) fn raw(key: &str, locale: &str) -> Option<&'static str> {
    let dictionaries = LOCALES
        .get_or_init(|| {
            Ok([
                serde_json::from_str(include_str!("../../../src/lib/locales/en.json"))?,
                serde_json::from_str(include_str!("../../../src/lib/locales/ru.json"))?,
            ])
        })
        .as_ref()
        .ok()?;
    dictionaries[usize::from(locale == "ru")]
        .get(key)
        .map(String::as_str)
}

pub fn resolved_ui_locale(settings: &AppSettings) -> String {
    match settings.ui_language.as_str() {
        "en" => "en".into(),
        "ru" => "ru".into(),
        _ => {
            #[cfg(windows)]
            if let Ok(locale) = winreg::RegKey::predef(winreg::enums::HKEY_CURRENT_USER)
                .open_subkey("Control Panel\\International")
                .and_then(|key| key.get_value::<String, _>("LocaleName"))
            {
                return if locale.to_ascii_lowercase().starts_with("ru") {
                    "ru"
                } else {
                    "en"
                }
                .into();
            }
            let lang = std::env::var("LANG")
                .or_else(|_| std::env::var("LANG_SYSTEM"))
                .unwrap_or_default()
                .to_ascii_lowercase();
            if lang.starts_with("ru") {
                "ru".into()
            } else {
                "en".into()
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn explicit_english_locale() {
        let mut settings = AppSettings::default();
        settings.ui_language = "en".into();
        assert_eq!(resolved_ui_locale(&settings), "en");
    }

    #[test]
    fn native_hud_keeps_shared_web_translations_without_the_removed_renderer() {
        assert_eq!(raw("overlayRecording", "ru"), Some("Запись"));
        assert_eq!(raw("overlayRecording", "en"), Some("Recording"));
        assert_eq!(
            raw("overlayRecording", "unknown"),
            raw("overlayRecording", "en")
        );
        assert_eq!(raw("missing_translation", "ru"), None);
    }
}
