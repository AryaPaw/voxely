use std::sync::Arc;
use std::thread;
use std::time::Duration;

use tauri::AppHandle;
use tauri::Manager;
use tauri_plugin_global_shortcut::{GlobalShortcutExt, Shortcut, ShortcutState};

use crate::app::session::{cancel_recording, toggle_recording, AppContext};
use crate::error::AppError;
use crate::windows_int::escape_hook;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionCommand {
    Toggle,
    Cancel,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShortcutLayout {
    pub hotkey: String,
    pub suspended: bool,
}

pub fn shortcut_layout_changed(applied: Option<&ShortcutLayout>, desired: &ShortcutLayout) -> bool {
    applied != Some(desired)
}

pub fn coalesce_session_command(
    pending: Option<SessionCommand>,
    next: SessionCommand,
) -> SessionCommand {
    match (pending, next) {
        (_, SessionCommand::Cancel) => SessionCommand::Cancel,
        (Some(SessionCommand::Cancel), SessionCommand::Toggle) => SessionCommand::Cancel,
        (_, command) => command,
    }
}

pub fn parse_hotkey(spec: &str) -> Result<Shortcut, AppError> {
    spec.parse::<Shortcut>()
        .map_err(|e| AppError::HotkeyFailed(e.to_string()))
}

pub fn toggle_dictation(app: &AppHandle) -> Result<(), AppError> {
    toggle_recording(app)
}

pub fn cancel_dictation(app: &AppHandle) -> Result<(), AppError> {
    cancel_recording(app)
}

pub fn wants_escape_hotkey(cancellable: bool, suspended: bool) -> bool {
    cancellable && !suspended
}

pub fn should_apply_scheduled_sync(ticket: u64, latest: u64) -> bool {
    ticket == latest
}

pub fn escape_register_fallback_to_ll_hook(register_ok: bool) -> bool {
    !register_ok
}

pub fn post_session_command(app: &AppHandle, command: SessionCommand) {
    let Some(ctx) = app.try_state::<Arc<AppContext>>() else {
        return;
    };
    if command == SessionCommand::Cancel && !crate::app::operations::escape_cancels(&ctx) {
        return;
    }
    {
        let mut slot = ctx.pending_session_command.lock();
        *slot = Some(coalesce_session_command(*slot, command));
    }
    let app = app.clone();
    thread::spawn(move || {
        thread::sleep(Duration::from_millis(1));
        let handle = app.clone();
        let _ = app.run_on_main_thread(move || {
            let Some(ctx) = handle.try_state::<Arc<AppContext>>() else {
                return;
            };
            let Some(command) = ctx.pending_session_command.lock().take() else {
                return;
            };
            match command {
                SessionCommand::Toggle => {
                    let _ = toggle_recording(&handle);
                }
                SessionCommand::Cancel => {
                    let _ = cancel_recording(&handle);
                }
            }
        });
    });
}

fn log_unregister(app: &AppHandle) {
    if let Err(error) = app.global_shortcut().unregister_all() {
        tracing::error!(error = %error, "unregister_all failed");
    }
}

pub fn sync_shortcuts(app: &AppHandle) -> Result<(), AppError> {
    let Some(ctx) = app.try_state::<Arc<AppContext>>() else {
        tracing::warn!("shortcut sync skipped: AppContext not managed");
        return Ok(());
    };
    let suspended = *ctx.hotkeys_suspended.lock();
    let spec = ctx.settings.lock().hotkey.clone();
    let desired = ShortcutLayout {
        hotkey: spec.clone(),
        suspended,
    };
    if !shortcut_layout_changed(ctx.applied_shortcut_layout.lock().as_ref(), &desired) {
        return Ok(());
    }
    if suspended {
        log_unregister(app);
        escape_hook::uninstall();
        *ctx.applied_shortcut_layout.lock() = Some(desired);
        return Ok(());
    }
    log_unregister(app);
    let shortcut = parse_hotkey(&spec)?;
    app.global_shortcut()
        .on_shortcut(shortcut, |app, _shortcut, event| {
            if event.state == ShortcutState::Pressed {
                post_session_command(app, SessionCommand::Toggle);
            }
        })
        .map_err(|e| AppError::HotkeyFailed(e.to_string()))?;
    escape_hook::install(app);
    *ctx.applied_shortcut_layout.lock() = Some(desired);
    Ok(())
}

pub fn set_hotkeys_suspended(app: &AppHandle, suspended: bool) -> Result<(), AppError> {
    let ctx = app.state::<Arc<AppContext>>();
    *ctx.hotkeys_suspended.lock() = suspended;
    *ctx.applied_shortcut_layout.lock() = None;
    sync_shortcuts(app)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn idle_does_not_want_escape() {
        assert!(!wants_escape_hotkey(false, false));
        assert!(wants_escape_hotkey(true, false));
        assert!(!wants_escape_hotkey(true, true));
    }

    #[test]
    fn coalesced_sync_keeps_latest_ticket() {
        assert!(should_apply_scheduled_sync(3, 3));
        assert!(!should_apply_scheduled_sync(2, 3));
    }

    #[test]
    fn hotkey_layout_ignores_cancellable() {
        let registered = ShortcutLayout {
            hotkey: "Control+Space".into(),
            suspended: false,
        };
        assert!(!shortcut_layout_changed(Some(&registered), &registered));
        let suspended = ShortcutLayout {
            hotkey: registered.hotkey.clone(),
            suspended: true,
        };
        assert!(shortcut_layout_changed(Some(&registered), &suspended));
        let other_hotkey = ShortcutLayout {
            hotkey: "Alt+X".into(),
            suspended: false,
        };
        assert!(shortcut_layout_changed(Some(&registered), &other_hotkey));
        assert!(shortcut_layout_changed(None, &registered));
        assert!(!wants_escape_hotkey(false, false));
    }

    #[test]
    fn cancel_wins_over_pending_toggle() {
        assert_eq!(
            coalesce_session_command(Some(SessionCommand::Toggle), SessionCommand::Cancel),
            SessionCommand::Cancel
        );
        assert_eq!(
            coalesce_session_command(Some(SessionCommand::Cancel), SessionCommand::Toggle),
            SessionCommand::Cancel
        );
        assert_eq!(
            coalesce_session_command(Some(SessionCommand::Toggle), SessionCommand::Toggle),
            SessionCommand::Toggle
        );
    }

    #[test]
    fn register_failure_uses_ll_hook() {
        assert!(escape_register_fallback_to_ll_hook(false));
        assert!(!escape_register_fallback_to_ll_hook(true));
    }
}
