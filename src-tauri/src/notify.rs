use std::sync::Arc;

use tauri::{AppHandle, Manager};
#[cfg(not(windows))]
use tauri_plugin_notification::NotificationExt;

use crate::app::lifecycle::app_display_name;
use crate::app::locale::{error_message, message, resolved_ui_locale, system_ui_locale};
use crate::app::session::AppContext;
use crate::error::AppError;
use crate::windows_int::insert_engine::{CopyReason, InsertOutcome};

pub const WINDOWS_APP_USER_MODEL_ID: &str = "com.voxely.desktop";

fn notification_preferences(app: &AppHandle) -> (bool, &'static str) {
    app.try_state::<Arc<AppContext>>()
        .map(|state| {
            let settings = state.settings.lock();
            (settings.notifications, resolved_ui_locale(&settings))
        })
        .unwrap_or_else(|| (true, system_ui_locale()))
}

pub fn insert_outcome_toast(
    locale: &str,
    outcome: InsertOutcome,
) -> Option<(&'static str, &'static str)> {
    let key = match outcome {
        InsertOutcome::Inserted | InsertOutcome::CancelledBeforeDelivery => return None,
        InsertOutcome::Copied { reason } => match reason {
            CopyReason::UserSwitched => "insertCopyUserSwitched",
            CopyReason::NoTarget => "insertCopyNoTarget",
            CopyReason::TargetGone => "insertCopyTargetGone",
            CopyReason::Minimized => "insertCopyMinimized",
            CopyReason::IntegrityBlocked => "insertCopyIntegrityBlocked",
            CopyReason::SendBlocked => "insertCopySendBlocked",
            CopyReason::RestoreFailed => "insertCopyRestoreFailed",
            CopyReason::FocusNotReady => "insertCopyFocusNotReady",
            CopyReason::ClipboardMode => "copiedInsert",
            CopyReason::ClipboardBusy => return None,
        },
        InsertOutcome::PartialCopied => "copiedPartial",
        InsertOutcome::Failed => "insertDeliveryFailed",
    };
    Some((app_display_name(locale), message(key, locale)))
}

pub fn notify_insert_outcome(app: &AppHandle, outcome: InsertOutcome) {
    let (enabled, locale) = notification_preferences(app);
    if !enabled {
        return;
    }
    let Some((title, body)) = insert_outcome_toast(locale, outcome) else {
        return;
    };
    match show_named(app, title, body) {
        Ok(()) => tracing::info!(outcome = ?outcome, "insert toast dispatched"),
        Err(err) => tracing::warn!(error = %err, outcome = ?outcome, "insert toast show failed"),
    }
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
    locale: &str,
    enabled: bool,
    err: &AppError,
) -> Option<(&'static str, &'static str)> {
    if !enabled || !should_toast_error(err) {
        return None;
    }
    Some((app_display_name(locale), error_message(err.code(), locale)))
}

pub fn show_named(app: &AppHandle, title: &str, body: &str) -> Result<(), AppError> {
    show_desktop_toast(app, title, body)
}

pub fn show_error(app: &AppHandle, err: &AppError) {
    let (enabled, locale) = notification_preferences(app);
    let Some((title, body)) = error_toast(locale, enabled, err) else {
        return;
    };
    if let Err(show_err) = show_named(app, title, body) {
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
        .unwrap_or_else(system_ui_locale);
    show_named(app, app_display_name(locale), preview_body(locale))
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
        assert!(error_toast("en", true, &AppError::Cancelled).is_none());
    }

    #[test]
    fn dictation_errors_toast_when_enabled() {
        assert_eq!(
            error_toast("en", true, &AppError::ProviderUnavailable),
            Some(("Voxely", "Provider unavailable"))
        );
        assert!(error_toast("ru", false, &AppError::InvalidApiKey).is_none());
        assert!(should_toast_error(&AppError::Interrupted));
    }

    #[test]
    fn insertion_failure_uses_the_selected_language_for_both_toast_paths() {
        let error = AppError::TextInsertionFailed("private details".into());
        for (locale, generic, delivery) in [
            (
                "en",
                "Could not insert text",
                "Couldn't deliver the full transcript. Open History to copy it.",
            ),
            (
                "ru",
                "Не удалось вставить текст",
                "Не удалось доставить весь текст. Его можно скопировать из истории.",
            ),
        ] {
            assert_eq!(error_toast(locale, true, &error), Some(("Voxely", generic)));
            assert!(error_toast(locale, false, &error).is_none());
            assert_eq!(
                insert_outcome_toast(locale, InsertOutcome::Failed),
                Some(("Voxely", delivery))
            );
        }
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
        assert!(insert_outcome_toast(
            "en",
            InsertOutcome::Copied {
                reason: CopyReason::ClipboardBusy,
            }
        )
        .is_none());
    }

    #[test]
    fn fallback_notifications_identify_the_actual_cause_in_both_languages() {
        let cases = [
            (CopyReason::UserSwitched, "другое окно", "different window"),
            (CopyReason::RestoreFailed, "вернуть фокус", "restore focus"),
            (
                CopyReason::FocusNotReady,
                "не получило фокус",
                "doesn't have focus",
            ),
            (
                CopyReason::TargetGone,
                "окно недоступно",
                "window is no longer available",
            ),
            (CopyReason::Minimized, "свёрнуто", "minimized"),
            (
                CopyReason::IntegrityBlocked,
                "Недостаточно прав",
                "higher privileges",
            ),
            (CopyReason::SendBlocked, "отклонила", "blocked keyboard"),
            (CopyReason::NoTarget, "не найдено", "was found"),
        ];
        for (reason, russian, english) in cases {
            assert!(insert_outcome_toast("ru", InsertOutcome::Copied { reason })
                .unwrap()
                .1
                .contains(russian));
            assert!(insert_outcome_toast("en", InsertOutcome::Copied { reason })
                .unwrap()
                .1
                .contains(english));
        }
    }
}
