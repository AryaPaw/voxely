use std::sync::Arc;

use tauri::{AppHandle, Manager};
use tauri_plugin_notification::NotificationExt;

use crate::app::lifecycle::app_display_name;
use crate::app::locale::resolved_ui_locale;
use crate::app::session::AppContext;
use crate::error::AppError;

pub fn should_toast_error(err: &AppError) -> bool {
    !matches!(err, AppError::Cancelled)
}

pub fn error_toast(
    app_name: &'static str,
    enabled: bool,
    err: &AppError,
) -> Option<(&'static str, String)> {
    if !enabled || !should_toast_error(err) {
        return None;
    }
    Some((app_name, err.user_message()))
}

pub fn show_error(app: &AppHandle, err: &AppError) {
    let ctx = app.try_state::<Arc<AppContext>>();
    let (enabled, locale) = match ctx {
        Some(state) => {
            let settings = state.settings.lock();
            (settings.notifications, resolved_ui_locale(&settings))
        }
        None => (true, "en".into()),
    };
    let Some((title, body)) = error_toast(app_display_name(&locale), enabled, err) else {
        return;
    };
    if let Err(show_err) = app.notification().builder().title(title).body(body).show() {
        tracing::warn!(error = %show_err, "system error toast failed");
    }
}

pub fn preview_body(locale: &str) -> &'static str {
    if locale == "en" {
        "Test error notification"
    } else {
        "Тестовое уведомление об ошибке"
    }
}

pub fn show_preview(app: &AppHandle) -> Result<(), AppError> {
    let locale = app
        .try_state::<Arc<AppContext>>()
        .map(|state| resolved_ui_locale(&state.settings.lock()))
        .unwrap_or_else(|| "en".into());
    app.notification()
        .builder()
        .title(app_display_name(&locale))
        .body(preview_body(&locale))
        .show()
        .map_err(|err| AppError::StorageFailed(err.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cancelled_is_not_a_system_toast() {
        assert!(!should_toast_error(&AppError::Cancelled));
        assert!(error_toast("Voxely", true, &AppError::Cancelled).is_none());
    }

    #[test]
    fn dictation_errors_toast_when_enabled() {
        let toast = error_toast("Voxely", true, &AppError::ProviderUnavailable);
        assert_eq!(
            toast,
            Some(("Voxely", AppError::ProviderUnavailable.user_message()))
        );
        assert!(error_toast("Voxely", false, &AppError::InvalidApiKey).is_none());
        assert!(should_toast_error(&AppError::TextInsertionFailed(
            "focus".into()
        )));
        assert!(should_toast_error(&AppError::Interrupted));
    }

    #[test]
    fn debug_preview_is_a_real_error_toast() {
        assert_eq!(preview_body("en"), "Test error notification");
        assert_eq!(preview_body("ru"), "Тестовое уведомление об ошибке");
    }
}
