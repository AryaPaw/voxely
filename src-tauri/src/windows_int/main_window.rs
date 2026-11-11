//! Windows owns the outer border, shadow and corner clipping of the main window.
pub fn round_corners(window: &tauri::WebviewWindow) {
    #[cfg(not(windows))]
    let _ = window;
    #[cfg(windows)]
    if let Ok(hwnd) = window.hwnd() {
        use windows::Win32::Graphics::Dwm::{
            DwmSetWindowAttribute, DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_ROUND,
        };
        let preference = DWMWCP_ROUND;
        // This is a hint on Windows 11. Unsupported systems keep their own policy;
        // never replace window regions or fail startup for a cosmetic preference.
        if let Err(error) = unsafe {
            DwmSetWindowAttribute(
                windows::Win32::Foundation::HWND(hwnd.0),
                DWMWA_WINDOW_CORNER_PREFERENCE,
                &preference as *const _ as _,
                std::mem::size_of_val(&preference) as u32,
            )
        } {
            tracing::debug!(%error, "Main window corner preference isn't available");
        }
    }
}
