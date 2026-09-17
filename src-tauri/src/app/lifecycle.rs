use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde::Serialize;
use tauri::menu::{MenuBuilder, MenuItemBuilder};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Emitter, Manager, PhysicalPosition, Position};

use tauri_plugin_autostart::ManagerExt;

use crate::app::overlay::{center_physical_position, WorkArea};
use crate::app::session::AppContext;
use crate::app::shortcuts::sync_shortcuts;
use crate::error::AppError;
use crate::windows_int::overlay::{work_area_for_cursor, work_area_for_foreground};

pub const GITHUB_REPO_URL: &str = "https://github.com/AryaPaw/voxely";
pub const GITHUB_ISSUES_URL: &str = "https://github.com/AryaPaw/voxely/issues";

pub fn github_page_url(page: Option<&str>) -> &'static str {
    match page {
        Some("issues") => GITHUB_ISSUES_URL,
        _ => GITHUB_REPO_URL,
    }
}

pub fn is_local_build() -> bool {
    cfg!(debug_assertions)
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeInfo {
    pub local_build: bool,
    pub build_date: String,
    pub settings_recovered: bool,
}

pub fn runtime_info() -> RuntimeInfo {
    RuntimeInfo {
        local_build: is_local_build(),
        build_date: env!("VOXELY_BUILD_DATE").to_string(),
        settings_recovered: false,
    }
}

pub fn runtime_info_with_recovery(recovered: bool) -> RuntimeInfo {
    RuntimeInfo {
        settings_recovered: recovered,
        ..runtime_info()
    }
}

pub fn app_display_name(_locale: &str) -> &'static str {
    "Voxely"
}

pub fn window_title(locale: &str) -> &'static str {
    if !is_local_build() {
        return "Voxely";
    }
    if locale.to_ascii_lowercase().starts_with("ru") {
        "Voxely (локальная версия)"
    } else {
        "Voxely (local)"
    }
}

fn apply_app_identity(app: &AppHandle, locale: &str) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.set_title(window_title(locale));
    }
    if let Some(tray) = app.tray_by_id("main") {
        let _ = tray.set_tooltip(Some(app_display_name("en")));
    }
}

pub fn configure_tray(app: &AppHandle) -> Result<(), AppError> {
    let locale = {
        let ctx = app.try_state::<Arc<AppContext>>();
        ctx.map(|state| crate::app::locale::resolved_ui_locale(&state.settings.lock()))
            .unwrap_or_else(|| "en".into())
    };
    let (open_label, quit_label) = tray_labels(&locale);
    let open = MenuItemBuilder::with_id("open", open_label)
        .build(app)
        .map_err(|e| AppError::StorageFailed(e.to_string()))?;
    let quit = MenuItemBuilder::with_id("quit", quit_label)
        .build(app)
        .map_err(|e| AppError::StorageFailed(e.to_string()))?;
    let menu = MenuBuilder::new(app)
        .item(&open)
        .separator()
        .item(&quit)
        .build()
        .map_err(|e| AppError::StorageFailed(e.to_string()))?;
    if let Some(tray) = app.tray_by_id("main") {
        tray.set_menu(Some(menu))
            .map_err(|e| AppError::StorageFailed(e.to_string()))?;
        apply_app_identity(app, &locale);
        return Ok(());
    }
    let icon = app
        .default_window_icon()
        .cloned()
        .ok_or_else(|| AppError::StorageFailed("missing tray icon".into()))?;
    TrayIconBuilder::with_id("main")
        .icon(icon)
        .tooltip(app_display_name(&locale))
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id().as_ref() {
            "open" => show_main(app, "history"),
            "quit" => {
                crate::app::session::shutdown_session(app);
                app.exit(0);
            }
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                show_main(tray.app_handle(), "history");
            }
        })
        .build(app)
        .map_err(|e| AppError::StorageFailed(e.to_string()))?;
    apply_app_identity(app, &locale);
    Ok(())
}

pub fn tray_labels(locale: &str) -> (&'static str, &'static str) {
    if locale == "en" {
        ("Open", "Quit")
    } else {
        ("Открыть", "Выход")
    }
}

pub fn should_hide_on_launch(args: impl IntoIterator<Item = impl AsRef<str>>) -> bool {
    args.into_iter().any(|arg| {
        arg.as_ref()
            .split(char::is_whitespace)
            .any(|part| part == "--autostart")
    })
}

pub fn hide_main_to_tray(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.hide();
        let _ = window.set_skip_taskbar(true);
    }
}

pub fn apply_launch_visibility(app: &AppHandle, args: impl IntoIterator<Item = impl AsRef<str>>) {
    if should_hide_on_launch(args) {
        hide_main_to_tray(app);
        return;
    }
    center_main_window(app);
    show_main(app, "history");
}

pub fn main_section_from_route(route: &str) -> &str {
    let trimmed = route.trim().trim_start_matches('/');
    if trimmed.is_empty() || trimmed == "history" {
        return "history";
    }
    trimmed
}

pub fn show_main(app: &AppHandle, route: &str) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.set_skip_taskbar(false);
        let _ = window.unminimize();
        let _ = window.show();
        let _ = window.set_focus();
    }
    let section = main_section_from_route(route);
    let _ = app.emit("app://navigate", section);
}

pub fn sync_autostart(app: &AppHandle, start_with_windows: bool) -> Result<(), AppError> {
    let autostart = app.autolaunch();
    if start_with_windows {
        autostart
            .enable()
            .map_err(|e| AppError::StorageFailed(e.to_string()))?;
        if !autostart.is_enabled().unwrap_or(false) {
            return Err(AppError::StorageFailed("autostart was not enabled".into()));
        }
    } else {
        autostart
            .disable()
            .map_err(|e| AppError::StorageFailed(e.to_string()))?;
    }
    Ok(())
}

pub fn center_main_window(app: &AppHandle) {
    let Some(window) = app.get_webview_window("main") else {
        return;
    };
    let skip: Vec<usize> = ["main", "overlay"]
        .into_iter()
        .filter_map(|label| {
            app.get_webview_window(label)
                .and_then(|w| w.hwnd().ok())
                .map(|h| h.0 as usize)
        })
        .collect();
    let work = work_area_for_foreground(&skip)
        .or_else(work_area_for_cursor)
        .unwrap_or(WorkArea {
            left: 0,
            top: 0,
            right: 1920,
            bottom: 1080,
        });
    if let Ok(size) = window.outer_size() {
        let (x, y) = center_physical_position(work, size.width, size.height);
        let _ = window.set_position(Position::Physical(PhysicalPosition::new(x, y)));
        return;
    }
    let _ = window.center();
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

pub fn debug_data_dir_override(raw: Option<&str>, roaming: &Path) -> Option<PathBuf> {
    let raw = raw?;
    let path = PathBuf::from(raw);
    if !path.is_absolute() {
        return None;
    }
    let name = path.file_name()?.to_str()?;
    if name != "VoxelyPerf" && !name.starts_with("VoxelyPerf-") {
        return None;
    }
    let parent = path.parent()?;
    if parent != roaming {
        return None;
    }
    Some(path)
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
    #[cfg(debug_assertions)]
    if let Some(path) =
        debug_data_dir_override(std::env::var("VOXELY_DATA_DIR").ok().as_deref(), &roaming)
    {
        return Ok(path);
    }
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
    fn debug_override_rejects_live_voxely_folder() {
        let dir = tempdir().unwrap();
        let roaming = dir.path();
        assert!(debug_data_dir_override(Some("relative"), roaming).is_none());
        assert!(
            debug_data_dir_override(Some(roaming.join("Voxely").to_str().unwrap()), roaming)
                .is_none()
        );
        let ok = roaming.join("VoxelyPerf");
        assert_eq!(
            debug_data_dir_override(ok.to_str(), roaming),
            Some(ok.clone())
        );
        let tagged = roaming.join("VoxelyPerf-history-500");
        assert_eq!(
            debug_data_dir_override(tagged.to_str(), roaming),
            Some(tagged)
        );
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

    #[test]
    fn github_urls_cover_repo_and_issues() {
        assert_eq!(GITHUB_REPO_URL, "https://github.com/AryaPaw/voxely");
        assert_eq!(github_page_url(Some("issues")), GITHUB_ISSUES_URL);
        assert_eq!(github_page_url(None), GITHUB_REPO_URL);
    }

    #[test]
    fn tray_labels_follow_locale() {
        assert_eq!(tray_labels("en"), ("Open", "Quit"));
        assert_eq!(tray_labels("ru"), ("Открыть", "Выход"));
    }

    #[test]
    fn window_title_marks_local_build_not_sandbox() {
        if is_local_build() {
            assert_eq!(window_title("ru"), "Voxely (локальная версия)");
            assert_eq!(window_title("en"), "Voxely (local)");
        } else {
            assert_eq!(window_title("ru"), "Voxely");
            assert_eq!(window_title("en"), "Voxely");
        }
        assert_eq!(app_display_name("en"), "Voxely");
        assert_eq!(app_display_name("ru"), "Voxely");
        assert_eq!(runtime_info().local_build, is_local_build());
        let day = &runtime_info().build_date;
        assert_eq!(day.len(), 10);
        assert!(day.as_bytes()[4] == b'-' && day.as_bytes()[7] == b'-');
    }

    #[test]
    fn autostart_hides_main_window() {
        assert!(should_hide_on_launch(["voxely.exe", "--autostart"]));
        assert!(should_hide_on_launch([
            r#"C:\Users\AryaPaw\AppData\Local\Voxely\voxely.exe --autostart"#
        ]));
        assert!(!should_hide_on_launch(["voxely.exe"]));
        assert!(!should_hide_on_launch(["voxely.exe", "--open"]));
        assert_eq!(main_section_from_route("/"), "history");
        assert_eq!(main_section_from_route("history"), "history");
        assert_eq!(main_section_from_route("/history"), "history");
    }

    #[test]
    fn second_autostart_instance_does_not_request_focus() {
        assert!(should_hide_on_launch([
            "C:\\Program Files\\Voxely\\voxely.exe",
            "--autostart"
        ]));
    }
}
