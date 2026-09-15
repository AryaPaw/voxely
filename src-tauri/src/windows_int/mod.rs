pub mod credentials;
#[cfg(windows)]
pub mod escape_hook;
#[cfg(not(windows))]
pub mod escape_hook {
    use tauri::AppHandle;

    pub fn install(_app: &AppHandle) {}

    pub fn uninstall() {}
}
pub mod insert_engine;
pub mod overlay;
pub mod text_injector;
