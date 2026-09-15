use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::Duration;

use tauri::AppHandle;
use tauri::Manager;
use tauri_plugin_global_shortcut::{GlobalShortcutExt, Shortcut, ShortcutState};

use crate::app::machine::is_cancellable;
use crate::app::session::{cancel_recording, toggle_recording, AppContext};
use crate::error::AppError;
use crate::windows_int::escape_hook;

pub fn parse_hotkey(spec: &str) -> Result<Shortcut, AppError> {
    spec.parse::<Shortcut>()
        .map_err(|e| AppError::HotkeyFailed(e.to_string()))
}

pub fn toggle_dictation(app: &AppHandle) -> Result<(), AppError> {
    let result = toggle_recording(app);
    schedule_sync(app);
    result
}

pub fn cancel_dictation(app: &AppHandle) -> Result<(), AppError> {
    let result = cancel_recording(app);
    schedule_sync(app);
    result
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

pub fn schedule_sync(app: &AppHandle) {
    let Some(ctx) = app.try_state::<Arc<AppContext>>() else {
        return;
    };
    let ticket = ctx.shortcut_sync_generation.fetch_add(1, Ordering::SeqCst) + 1;
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(Duration::from_millis(40)).await;
        let Some(ctx) = app.try_state::<Arc<AppContext>>() else {
            return;
        };
        if !should_apply_scheduled_sync(ticket, ctx.shortcut_sync_generation.load(Ordering::SeqCst))
        {
            return;
        }
        if let Err(error) = sync_shortcuts(&app) {
            tracing::error!(error = %error, "shortcut sync failed");
        }
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
    let cancellable = is_cancellable(&ctx.state.lock());
    if suspended {
        log_unregister(app);
        escape_hook::uninstall();
        return Ok(());
    }
    let spec = ctx.settings.lock().hotkey.clone();
    log_unregister(app);
    let shortcut = parse_hotkey(&spec)?;
    app.global_shortcut()
        .on_shortcut(shortcut, |app, _shortcut, event| {
            if event.state == ShortcutState::Pressed {
                let _ = toggle_recording(app);
                schedule_sync(app);
            }
        })
        .map_err(|e| AppError::HotkeyFailed(e.to_string()))?;
    if wants_escape_hotkey(cancellable, suspended) {
        let escape = "Escape"
            .parse::<Shortcut>()
            .map_err(|e| AppError::HotkeyFailed(e.to_string()))?;
        match app
            .global_shortcut()
            .on_shortcut(escape, |app, _shortcut, event| {
                if event.state == ShortcutState::Pressed {
                    let _ = cancel_recording(app);
                    schedule_sync(app);
                }
            }) {
            Ok(()) => {
                tracing::info!("Escape registered via RegisterHotKey");
                escape_hook::uninstall();
            }
            Err(error) => {
                tracing::error!(
                    error = %error,
                    "RegisterHotKey failed for Escape; using busy-only LL hook"
                );
                escape_hook::install(app);
            }
        }
    } else {
        escape_hook::uninstall();
    }
    Ok(())
}

pub fn set_hotkeys_suspended(app: &AppHandle, suspended: bool) -> Result<(), AppError> {
    let ctx = app.state::<Arc<AppContext>>();
    *ctx.hotkeys_suspended.lock() = suspended;
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
    fn register_failure_uses_ll_hook() {
        assert!(escape_register_fallback_to_ll_hook(false));
        assert!(!escape_register_fallback_to_ll_hook(true));
    }
}
