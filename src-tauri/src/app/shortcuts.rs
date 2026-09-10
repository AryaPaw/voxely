use std::sync::Arc;
use std::time::Duration;

use tauri::AppHandle;
use tauri::Manager;
use tauri_plugin_global_shortcut::{GlobalShortcutExt, Shortcut, ShortcutState};

use crate::app::machine::is_recording_active;
use crate::app::session::{cancel_recording, toggle_recording, AppContext};
use crate::error::AppError;

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

pub fn schedule_sync(app: &AppHandle) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(Duration::from_millis(40)).await;
        let _ = sync_shortcuts(&app);
    });
}

pub fn sync_shortcuts(app: &AppHandle) -> Result<(), AppError> {
    let ctx = app.state::<Arc<AppContext>>();
    if *ctx.hotkeys_suspended.lock() {
        let _ = app.global_shortcut().unregister_all();
        return Ok(());
    }
    let spec = ctx.settings.lock().hotkey.clone();
    let recording = is_recording_active(&ctx.state.lock());
    let _ = app.global_shortcut().unregister_all();
    let shortcut = parse_hotkey(&spec)?;
    app.global_shortcut()
        .on_shortcut(shortcut, |app, _shortcut, event| {
            if event.state == ShortcutState::Pressed {
                let _ = toggle_recording(app);
                schedule_sync(app);
            }
        })
        .map_err(|e| AppError::HotkeyFailed(e.to_string()))?;
    if recording {
        let escape = "Escape"
            .parse::<Shortcut>()
            .map_err(|e| AppError::HotkeyFailed(e.to_string()))?;
        app.global_shortcut()
            .on_shortcut(escape, |app, _shortcut, event| {
                if event.state == ShortcutState::Pressed {
                    let _ = cancel_recording(app);
                    schedule_sync(app);
                }
            })
            .map_err(|e| AppError::HotkeyFailed(e.to_string()))?;
    }
    Ok(())
}

pub fn set_hotkeys_suspended(app: &AppHandle, suspended: bool) -> Result<(), AppError> {
    let ctx = app.state::<Arc<AppContext>>();
    *ctx.hotkeys_suspended.lock() = suspended;
    sync_shortcuts(app)
}
