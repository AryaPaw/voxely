use std::path::PathBuf;
use std::sync::Arc;

use tauri::menu::{MenuBuilder, MenuItemBuilder};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Manager};

use crate::app::session::AppContext;
use crate::app::shortcuts::sync_shortcuts;
use crate::error::AppError;

pub fn configure_tray(app: &AppHandle) -> Result<(), AppError> {
    let open = MenuItemBuilder::with_id("open", "Открыть")
        .build(app)
        .map_err(|e| AppError::StorageFailed(e.to_string()))?;
    let quit = MenuItemBuilder::with_id("quit", "Выход")
        .build(app)
        .map_err(|e| AppError::StorageFailed(e.to_string()))?;
    let menu = MenuBuilder::new(app)
        .item(&open)
        .separator()
        .item(&quit)
        .build()
        .map_err(|e| AppError::StorageFailed(e.to_string()))?;
    let icon = app
        .default_window_icon()
        .cloned()
        .ok_or_else(|| AppError::StorageFailed("missing tray icon".into()))?;
    TrayIconBuilder::new()
        .icon(icon)
        .tooltip("Voxely")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id().as_ref() {
            "open" => show_main(app, "/"),
            "quit" => app.exit(0),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                show_main(tray.app_handle(), "/");
            }
        })
        .build(app)
        .map_err(|e| AppError::StorageFailed(e.to_string()))?;
    Ok(())
}

pub fn show_main(app: &AppHandle, _route: &str) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.unminimize();
        let _ = window.show();
        let _ = window.set_focus();
    }
}

pub fn reregister_hotkey(app: &AppHandle, _spec: &str) -> Result<(), AppError> {
    sync_shortcuts(app)
}

pub fn preferred_data_dir(roaming: PathBuf) -> PathBuf {
    let preferred = roaming.join("Voxely");
    let legacy = roaming.join("com.voxely.desktop");
    if !preferred.exists() && legacy.exists() {
        if std::fs::rename(&legacy, &preferred).is_ok() {
            return preferred;
        }
        return legacy;
    }
    preferred
}

pub fn data_dir(app: &AppHandle) -> Result<PathBuf, AppError> {
    let roaming = if let Ok(appdata) = std::env::var("APPDATA") {
        PathBuf::from(appdata)
    } else {
        app.path()
            .app_data_dir()
            .map_err(|e| AppError::StorageFailed(e.to_string()))?
            .parent()
            .map(PathBuf::from)
            .ok_or_else(|| AppError::StorageFailed("app data parent".into()))?
    };
    Ok(preferred_data_dir(roaming))
}

pub fn attach_context(app: &AppHandle) -> Result<Arc<AppContext>, AppError> {
    let dir = data_dir(app)?;
    AppContext::initialize(dir).map(Arc::new)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn prefers_voxely_folder_name() {
        let dir = tempdir().unwrap();
        let path = preferred_data_dir(dir.path().to_path_buf());
        assert_eq!(path.file_name().unwrap(), "Voxely");
    }

    #[test]
    fn migrates_legacy_identifier_folder() {
        let dir = tempdir().unwrap();
        let legacy = dir.path().join("com.voxely.desktop");
        std::fs::create_dir(&legacy).unwrap();
        std::fs::write(legacy.join("settings.json"), b"{}").unwrap();
        let path = preferred_data_dir(dir.path().to_path_buf());
        assert_eq!(path.file_name().unwrap(), "Voxely");
        assert!(path.join("settings.json").exists());
    }
}
