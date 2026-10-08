use crate::app::overlay::WorkArea;

#[cfg(windows)]
fn work_area_from_monitor(monitor: windows::Win32::Graphics::Gdi::HMONITOR) -> Option<WorkArea> {
    unsafe {
        use windows::Win32::Foundation::{BOOL, RECT};
        use windows::Win32::Graphics::Gdi::{GetMonitorInfoW, MONITORINFO};

        let mut info = MONITORINFO {
            cbSize: std::mem::size_of::<MONITORINFO>() as u32,
            rcMonitor: RECT::default(),
            rcWork: RECT::default(),
            dwFlags: 0,
        };
        let ok: BOOL = GetMonitorInfoW(monitor, &mut info);
        if !ok.as_bool() {
            return None;
        }
        Some(WorkArea {
            left: info.rcWork.left,
            top: info.rcWork.top,
            right: info.rcWork.right,
            bottom: info.rcWork.bottom,
        })
    }
}

#[cfg(windows)]
pub fn work_area_for_cursor() -> Option<WorkArea> {
    unsafe {
        use windows::Win32::Foundation::POINT;
        use windows::Win32::Graphics::Gdi::{MonitorFromPoint, MONITOR_DEFAULTTONEAREST};
        use windows::Win32::UI::WindowsAndMessaging::GetCursorPos;

        let mut point = POINT::default();
        GetCursorPos(&mut point).ok()?;
        work_area_from_monitor(MonitorFromPoint(point, MONITOR_DEFAULTTONEAREST))
    }
}

#[cfg(windows)]
pub fn pointer_event_matches_overlay(
    raw: isize,
    event_client_x: f64,
    event_client_y: f64,
    scale_factor: f64,
) -> bool {
    if raw == 0 || !scale_factor.is_finite() || scale_factor <= 0.0 {
        return false;
    }

    unsafe {
        use windows::Win32::Foundation::{HWND, POINT, RECT};
        use windows::Win32::Graphics::Gdi::ScreenToClient;
        use windows::Win32::UI::WindowsAndMessaging::{
            GetClientRect, GetCursorPos, IsWindowVisible,
        };

        let hwnd = HWND(raw as *mut core::ffi::c_void);
        if !IsWindowVisible(hwnd).as_bool() {
            return false;
        }

        let mut cursor = POINT::default();
        let mut rect = RECT::default();
        if GetCursorPos(&mut cursor).is_err()
            || !ScreenToClient(hwnd, &mut cursor).as_bool()
            || GetClientRect(hwnd, &mut rect).is_err()
        {
            return false;
        }

        pointer_matches_rect(
            cursor.x,
            cursor.y,
            event_client_x,
            event_client_y,
            scale_factor,
            0,
            0,
            rect.right,
            rect.bottom,
        )
    }
}

fn pointer_matches_rect(
    cursor_x: i32,
    cursor_y: i32,
    event_client_x: f64,
    event_client_y: f64,
    scale_factor: f64,
    left: i32,
    top: i32,
    right: i32,
    bottom: i32,
) -> bool {
    if !scale_factor.is_finite()
        || scale_factor <= 0.0
        || !event_client_x.is_finite()
        || !event_client_y.is_finite()
    {
        return false;
    }

    let event_x = event_client_x * scale_factor;
    let event_y = event_client_y * scale_factor;
    let cursor_x_f = f64::from(cursor_x);
    let cursor_y_f = f64::from(cursor_y);
    let in_window = cursor_x >= left && cursor_x < right && cursor_y >= top && cursor_y < bottom;
    let same_pointer = (cursor_x_f - event_x).abs() <= 8.0 && (cursor_y_f - event_y).abs() <= 8.0;

    in_window && same_pointer
}

#[cfg(windows)]
pub fn work_area_for_hwnd(raw: isize) -> Option<WorkArea> {
    if raw == 0 {
        return None;
    }
    unsafe {
        use windows::Win32::Foundation::HWND;
        use windows::Win32::Graphics::Gdi::{MonitorFromWindow, MONITOR_DEFAULTTONEAREST};

        let hwnd = HWND(raw as *mut core::ffi::c_void);
        work_area_from_monitor(MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST))
    }
}

#[cfg(windows)]
pub fn dpi_scale_for_hwnd(raw: isize) -> Option<f64> {
    if raw == 0 {
        return None;
    }
    unsafe {
        use windows::Win32::Foundation::HWND;
        use windows::Win32::Graphics::Gdi::{MonitorFromWindow, MONITOR_DEFAULTTONEAREST};
        use windows::Win32::UI::HiDpi::{GetDpiForMonitor, MDT_EFFECTIVE_DPI};

        let hwnd = HWND(raw as *mut core::ffi::c_void);
        let monitor = MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST);
        let mut dpi_x = 0u32;
        let mut dpi_y = 0u32;
        GetDpiForMonitor(monitor, MDT_EFFECTIVE_DPI, &mut dpi_x, &mut dpi_y).ok()?;
        if dpi_x == 0 {
            return None;
        }
        Some(f64::from(dpi_x) / 96.0)
    }
}

#[cfg(windows)]
pub fn work_area_for_foreground(skip_roots: &[usize]) -> Option<WorkArea> {
    unsafe {
        use windows::Win32::UI::WindowsAndMessaging::{GetAncestor, GetForegroundWindow, GA_ROOT};

        let hwnd = GetForegroundWindow();
        if hwnd.0.is_null() {
            return work_area_for_cursor();
        }
        let root = GetAncestor(hwnd, GA_ROOT);
        let root_val = root.0 as usize;
        let raw = hwnd.0 as usize;
        if skip_roots.contains(&root_val) || skip_roots.contains(&raw) {
            return work_area_for_cursor();
        }
        work_area_for_hwnd(hwnd.0 as isize)
    }
}

#[cfg(windows)]
pub fn apply_overlay_exstyle(raw: isize) {
    unsafe {
        use windows::Win32::Foundation::HWND;
        use windows::Win32::UI::WindowsAndMessaging::{
            GetWindowLongW, SetWindowLongW, SetWindowPos, GWL_EXSTYLE, HWND_TOPMOST,
            SWP_FRAMECHANGED, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, WS_EX_APPWINDOW,
            WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW,
        };
        let hwnd = HWND(raw as *mut core::ffi::c_void);
        let mut ex = GetWindowLongW(hwnd, GWL_EXSTYLE);
        ex |= WS_EX_NOACTIVATE.0 as i32 | WS_EX_TOOLWINDOW.0 as i32;
        ex &= !(WS_EX_APPWINDOW.0 as i32);
        SetWindowLongW(hwnd, GWL_EXSTYLE, ex);
        let _ = SetWindowPos(
            hwnd,
            HWND_TOPMOST,
            0,
            0,
            0,
            0,
            SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | SWP_FRAMECHANGED,
        );
    }
}

#[cfg(windows)]
pub fn show_noactivate(raw: isize) {
    set_click_through(raw, false);
    unsafe {
        use windows::Win32::Foundation::HWND;
        use windows::Win32::UI::WindowsAndMessaging::{ShowWindow, SW_SHOWNOACTIVATE};
        let hwnd = HWND(raw as *mut core::ffi::c_void);
        let _ = ShowWindow(hwnd, SW_SHOWNOACTIVATE);
    }
    apply_overlay_exstyle(raw);
}

#[cfg(windows)]
pub fn hide(raw: isize) {
    set_click_through(raw, true);
    unsafe {
        use windows::Win32::Foundation::HWND;
        use windows::Win32::UI::WindowsAndMessaging::{ShowWindow, SW_HIDE};
        let hwnd = HWND(raw as *mut core::ffi::c_void);
        let _ = ShowWindow(hwnd, SW_HIDE);
    }
}

#[cfg(windows)]
fn set_click_through(raw: isize, through: bool) {
    unsafe {
        use windows::Win32::Foundation::HWND;
        use windows::Win32::UI::WindowsAndMessaging::{
            GetWindowLongW, SetWindowLongW, GWL_EXSTYLE, WS_EX_TRANSPARENT,
        };
        let hwnd = HWND(raw as *mut core::ffi::c_void);
        let mut ex = GetWindowLongW(hwnd, GWL_EXSTYLE);
        if through {
            ex |= WS_EX_TRANSPARENT.0 as i32;
        } else {
            ex &= !(WS_EX_TRANSPARENT.0 as i32);
        }
        SetWindowLongW(hwnd, GWL_EXSTYLE, ex);
    }
}

#[cfg(not(windows))]
pub fn apply_overlay_exstyle(_raw: isize) {}

#[cfg(not(windows))]
pub fn show_noactivate(_raw: isize) {}

#[cfg(not(windows))]
pub fn hide(_raw: isize) {}

#[cfg(not(windows))]
pub fn work_area_for_cursor() -> Option<WorkArea> {
    None
}

#[cfg(not(windows))]
pub fn pointer_event_matches_overlay(
    _raw: isize,
    _event_client_x: f64,
    _event_client_y: f64,
    _scale_factor: f64,
) -> bool {
    false
}

#[cfg(not(windows))]
pub fn work_area_for_hwnd(_raw: isize) -> Option<WorkArea> {
    None
}

#[cfg(not(windows))]
pub fn dpi_scale_for_hwnd(_raw: isize) -> Option<f64> {
    None
}

#[cfg(not(windows))]
pub fn work_area_for_foreground(_skip_roots: &[usize]) -> Option<WorkArea> {
    None
}

#[cfg(test)]
mod pointer_tests {
    use super::pointer_matches_rect;

    #[test]
    fn accepts_current_pointer_inside_window_at_scaled_coordinates() {
        assert!(pointer_matches_rect(
            100, 100, 50.0, 50.0, 2.0, 0, 0, 200, 200
        ));
    }

    #[test]
    fn rejects_stale_webview_coordinates_after_window_show() {
        assert!(!pointer_matches_rect(
            100, 100, 450.0, 250.0, 2.0, 0, 0, 200, 200
        ));
    }

    #[test]
    fn rejects_cursor_outside_overlay_window() {
        assert!(!pointer_matches_rect(
            201, 100, 100.5, 50.0, 2.0, 0, 0, 200, 200
        ));
    }

    #[test]
    fn rejects_invalid_scale_or_non_finite_coordinates() {
        assert!(!pointer_matches_rect(
            100, 100, 50.0, 50.0, 0.0, 0, 0, 200, 200
        ));
        assert!(!pointer_matches_rect(
            100,
            100,
            f64::NAN,
            50.0,
            2.0,
            0,
            0,
            200,
            200
        ));
    }
}
