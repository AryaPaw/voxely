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
pub fn work_area_for_hwnd(_raw: isize) -> Option<WorkArea> {
    None
}

#[cfg(not(windows))]
pub fn work_area_for_foreground(_skip_roots: &[usize]) -> Option<WorkArea> {
    None
}
