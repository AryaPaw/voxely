use crate::settings::AppSettings;
use std::{collections::BTreeMap, sync::OnceLock};

type Dictionary = BTreeMap<String, String>;
static LOCALES: OnceLock<Result<[Dictionary; 2], serde_json::Error>> = OnceLock::new();
static ERROR_KEYS: OnceLock<Dictionary> = OnceLock::new();
static SYSTEM_UI_LOCALE: OnceLock<&'static str> = OnceLock::new();

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

pub(crate) fn message(key: &str, locale: &str) -> &'static str {
    raw(key, locale)
        .or_else(|| raw("errUnknown", locale))
        .unwrap_or("Unknown error")
}

pub(crate) fn error_message(code: &str, locale: &str) -> &'static str {
    let keys = ERROR_KEYS.get_or_init(|| {
        serde_json::from_str(include_str!("../../../src/lib/locales/errors.json"))
            .expect("bundled error translation keys must be valid")
    });
    message(
        keys.get(code).map(String::as_str).unwrap_or("errUnknown"),
        locale,
    )
}

pub(crate) fn system_ui_locale() -> &'static str {
    SYSTEM_UI_LOCALE.get_or_init(detect_system_ui_locale)
}

#[cfg(windows)]
fn detect_system_ui_locale() -> &'static str {
    use windows::Win32::Globalization::GetUserDefaultUILanguage;
    use windows::Win32::System::SystemServices::LANG_RUSSIAN;

    // The lower ten bits of a LANGID identify the primary language.
    let language_id = unsafe { GetUserDefaultUILanguage() };
    if u32::from(language_id & 0x03ff) == LANG_RUSSIAN {
        "ru"
    } else {
        "en"
    }
}

#[cfg(not(windows))]
fn detect_system_ui_locale() -> &'static str {
    let language = std::env::var("LANG")
        .or_else(|_| std::env::var("LANG_SYSTEM"))
        .unwrap_or_default();
    if language.to_ascii_lowercase().starts_with("ru") {
        "ru"
    } else {
        "en"
    }
}

pub fn resolved_ui_locale(settings: &AppSettings) -> &'static str {
    match settings.ui_language.as_str() {
        "en" => "en",
        "ru" => "ru",
        _ => system_ui_locale(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn explicit_locale_overrides_the_system_language() {
        let mut settings = AppSettings::default();
        assert_eq!(resolved_ui_locale(&settings), system_ui_locale());
        settings.ui_language = "en".into();
        assert_eq!(resolved_ui_locale(&settings), "en");
        settings.ui_language = "ru".into();
        assert_eq!(resolved_ui_locale(&settings), "ru");
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
