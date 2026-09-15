use std::sync::Arc;

use tauri::{AppHandle, Manager};
#[cfg(not(windows))]
use tauri_plugin_notification::NotificationExt;

use crate::app::lifecycle::app_display_name;
use crate::app::locale::resolved_ui_locale;
use crate::app::session::AppContext;
use crate::error::AppError;
use crate::windows_int::insert_engine::{CopyReason, InsertOutcome};

pub const WINDOWS_APP_USER_MODEL_ID: &str = "com.voxely.desktop";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToastDispatch {
    Dispatched,
    Disabled,
}

pub fn insert_outcome_toast(
    locale: &str,
    outcome: InsertOutcome,
) -> Option<(&'static str, String)> {
    let title = crate::app::lifecycle::app_display_name(locale);
    let english = locale == "en";
    match outcome {
        InsertOutcome::Inserted | InsertOutcome::CancelledBeforeDelivery => None,
        InsertOutcome::Copied {
            reason: CopyReason::ClipboardBusy,
        } => None,
        InsertOutcome::Copied { .. } => Some((
            title,
            if english {
                "Text copied. Paste it yourself.".into()
            } else {
                "Текст скопирован, вставьте вручную".into()
            },
        )),
        InsertOutcome::PartialCopied => Some((
            title,
            if english {
                "Insert interrupted. Some text may already be in the field. Full transcript copied."
                    .into()
            } else {
                "Вставка прервана, часть текста могла вставиться; полный текст скопирован".into()
            },
        )),
        InsertOutcome::Failed => Some((
            title,
            if english {
                "Could not insert or copy the transcript. Open History.".into()
            } else {
                "Не удалось вставить или скопировать текст. Откройте История.".into()
            },
        )),
    }
}

pub fn notify_insert_outcome(app: &AppHandle, outcome: InsertOutcome) {
    let ctx = app.try_state::<Arc<AppContext>>();
    let (enabled, locale) = match ctx {
        Some(state) => {
            let settings = state.settings.lock();
            (settings.notifications, resolved_ui_locale(&settings))
        }
        None => (true, "en".into()),
    };
    let Some((title, body)) = insert_outcome_toast(&locale, outcome) else {
        if matches!(outcome, InsertOutcome::Failed) {
            show_error(app, &AppError::TextInsertionFailed("insert failed".into()));
        }
        return;
    };
    match show_if_enabled(app, title, &body) {
        Ok(()) => tracing::info!(outcome = ?outcome, "insert toast dispatched"),
        Err(err) => tracing::warn!(error = %err, outcome = ?outcome, "insert toast show failed"),
    }
    let _ = enabled;
}

pub fn apply_windows_app_identity() {
    #[cfg(windows)]
    unsafe {
        let _ = windows::Win32::UI::Shell::SetCurrentProcessExplicitAppUserModelID(
            windows::core::w!("com.voxely.desktop"),
        );
    }
}

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

pub fn show_named(app: &AppHandle, title: &str, body: &str) -> Result<(), AppError> {
    show_desktop_toast(app, title, body)
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
    if let Err(show_err) = show_named(app, title, &body) {
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
    show_named(app, app_display_name(&locale), preview_body(&locale))
}

pub fn show_if_enabled(app: &AppHandle, title: &str, body: &str) -> Result<(), AppError> {
    let enabled = app
        .try_state::<Arc<AppContext>>()
        .map(|state| state.settings.lock().notifications)
        .unwrap_or(true);
    if !enabled {
        return Ok(());
    }
    show_named(app, title, body)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToastUserAction {
    Activate,
    Dismiss,
}

pub fn toast_opens_history(action: ToastUserAction) -> bool {
    matches!(action, ToastUserAction::Activate)
}

fn show_desktop_toast(app: &AppHandle, title: &str, body: &str) -> Result<(), AppError> {
    #[cfg(windows)]
    {
        let app = app.clone();
        let title = title.to_string();
        let body = body.to_string();
        std::thread::spawn(move || {
            let mut notification = notify_rust::Notification::new();
            notification.summary(&title);
            notification.body(&body);
            notification.app_id(WINDOWS_APP_USER_MODEL_ID);
            match notification.show() {
                Ok(handle) => {
                    let _ = handle.wait_for_response(OpenHistoryOnActivate(app));
                }
                Err(err) => tracing::warn!(error = %err, "system toast failed"),
            }
        });
        Ok(())
    }
    #[cfg(not(windows))]
    {
        app.notification()
            .builder()
            .title(title)
            .body(body)
            .show()
            .map_err(|err| AppError::StorageFailed(err.to_string()))
    }
}

#[cfg(windows)]
fn windows_toast_action(response: &notify_rust::NotificationResponse) -> ToastUserAction {
    match response {
        notify_rust::NotificationResponse::Closed(_) => ToastUserAction::Dismiss,
        notify_rust::NotificationResponse::Default
        | notify_rust::NotificationResponse::Action(_)
        | notify_rust::NotificationResponse::Reply(_) => ToastUserAction::Activate,
    }
}

#[cfg(windows)]
struct OpenHistoryOnActivate(AppHandle);

#[cfg(windows)]
impl notify_rust::ResponseHandler for OpenHistoryOnActivate {
    fn call(self, response: &notify_rust::NotificationResponse) {
        if !toast_opens_history(windows_toast_action(response)) {
            return;
        }
        let app = self.0;
        let shown = app.clone();
        let _ = shown.run_on_main_thread(move || {
            crate::app::lifecycle::show_main(&app, "history");
        });
    }
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

    #[test]
    fn windows_toast_uses_voxely_app_id() {
        assert_eq!(WINDOWS_APP_USER_MODEL_ID, "com.voxely.desktop");
        assert!(!WINDOWS_APP_USER_MODEL_ID
            .to_ascii_lowercase()
            .contains("powershell"));
        assert!(toast_opens_history(ToastUserAction::Activate));
        assert!(!toast_opens_history(ToastUserAction::Dismiss));
    }

    #[test]
    fn insert_outcomes_have_honest_copy() {
        assert!(insert_outcome_toast("en", InsertOutcome::Inserted).is_none());
        assert!(insert_outcome_toast("ru", InsertOutcome::CancelledBeforeDelivery).is_none());
        let copied = insert_outcome_toast(
            "ru",
            InsertOutcome::Copied {
                reason: CopyReason::UserSwitched,
            },
        )
        .unwrap();
        assert!(copied.1.contains("скопирован"));
        let partial = insert_outcome_toast("ru", InsertOutcome::PartialCopied).unwrap();
        assert!(partial.1.contains("прервана"));
        assert_eq!(ToastDispatch::Dispatched, ToastDispatch::Dispatched);
    }
}
