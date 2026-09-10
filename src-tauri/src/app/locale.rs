use crate::settings::AppSettings;

pub fn resolved_ui_locale(settings: &AppSettings) -> String {
    match settings.ui_language.as_str() {
        "en" => "en".into(),
        "ru" => "ru".into(),
        _ => {
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
}
