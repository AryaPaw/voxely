use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct NativeHwnd {
    pub value: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InsertDecision {
    InsertIntoCaptured,
    HoldForUser { reason: &'static str },
}

pub fn decide_insert(captured: Option<NativeHwnd>, _current: Option<NativeHwnd>) -> InsertDecision {
    match captured {
        Some(_) => InsertDecision::InsertIntoCaptured,
        None => InsertDecision::HoldForUser {
            reason: "no captured window",
        },
    }
}

pub fn should_send_key_paste(
    captured_root: usize,
    foreground_root: Option<usize>,
    focus_root: Option<usize>,
) -> bool {
    should_post_paste(captured_root, foreground_root)
        && should_post_paste(captured_root, focus_root)
}

pub fn overlay_blocks_insert(overlay_root: Option<usize>, foreground_root: Option<usize>) -> bool {
    matches!(
        (overlay_root, foreground_root),
        (Some(overlay), Some(foreground)) if overlay == foreground
    )
}

pub fn insert_delivered(events_sent: u32, used_key_paste: bool) -> bool {
    used_key_paste && events_sent > 0
}

pub fn should_restore_clipboard(ours: &str, current: Option<&str>) -> bool {
    current == Some(ours)
}

pub fn should_post_paste(captured_root: usize, focus_root: Option<usize>) -> bool {
    matches!(focus_root, Some(root) if root == captured_root)
}

#[cfg(windows)]
pub mod native {
    use super::{
        insert_delivered, overlay_blocks_insert, should_restore_clipboard, should_send_key_paste,
        NativeHwnd,
    };
    use crate::error::AppError;
    use windows::Win32::Foundation::{HANDLE, HWND};
    use windows::Win32::System::DataExchange::{
        CloseClipboard, EmptyClipboard, GetClipboardData, OpenClipboard, SetClipboardData,
    };
    use windows::Win32::System::Memory::{GlobalAlloc, GlobalLock, GlobalUnlock, GMEM_MOVEABLE};
    use windows::Win32::UI::Input::KeyboardAndMouse::{
        SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYEVENTF_KEYUP, VK_CONTROL, VK_V,
    };
    use windows::Win32::UI::WindowsAndMessaging::{
        GetForegroundWindow, GetWindowThreadProcessId, IsWindow, SetForegroundWindow,
    };

    pub fn foreground_hwnd() -> Option<NativeHwnd> {
        unsafe {
            let hwnd = GetForegroundWindow();
            hwnd_to_native(hwnd)
        }
    }

    pub fn capture_target() -> Option<NativeHwnd> {
        unsafe {
            let foreground = GetForegroundWindow();
            hwnd_to_native(foreground)?;
            if let Some(focus) = thread_focus_hwnd(foreground) {
                if hwnd_root_value(focus) == hwnd_root_value(foreground) {
                    return hwnd_to_native(focus);
                }
            }
            hwnd_to_native(foreground)
        }
    }

    unsafe fn hwnd_to_native(hwnd: HWND) -> Option<NativeHwnd> {
        if hwnd.is_invalid() {
            None
        } else {
            Some(NativeHwnd {
                value: hwnd.0 as usize,
            })
        }
    }

    pub fn insert_unicode(hwnd: NativeHwnd, text: &str) -> Result<(), AppError> {
        insert_into_window(hwnd, text)
    }

    pub fn insert_into_window(hwnd: NativeHwnd, text: &str) -> Result<(), AppError> {
        unsafe {
            let target = HWND(hwnd.value as *mut _);
            if !IsWindow(target).as_bool() {
                return Err(AppError::TextInsertionFailed("window gone".into()));
            }
            let previous = snapshot_clipboard();
            clipboard_copy(text)?;
            let captured_root_hwnd = root_hwnd(target);
            let captured_root = captured_root_hwnd.0 as usize;
            let mut last_reason = "focus left captured window";
            for _ in 0..3 {
                let _guard = attach_input(target);
                let restored = SetForegroundWindow(target).as_bool();
                std::thread::sleep(std::time::Duration::from_millis(20));
                let foreground_root = hwnd_root_value(GetForegroundWindow());
                if overlay_blocks_insert(None, foreground_root) {
                    last_reason = "overlay still foreground";
                    continue;
                }
                let focus_root = hwnd_root_value(thread_focus_hwnd(target).unwrap_or(target));
                if should_send_key_paste(captured_root, foreground_root, focus_root) {
                    let inputs = [
                        key(VK_CONTROL.0, false),
                        key(VK_V.0, false),
                        key(VK_V.0, true),
                        key(VK_CONTROL.0, true),
                    ];
                    let sent = SendInput(&inputs, std::mem::size_of::<INPUT>() as i32);
                    if insert_delivered(sent, true) {
                        std::thread::sleep(std::time::Duration::from_millis(40));
                        restore_clipboard_if_unchanged(text, previous.as_deref());
                        return Ok(());
                    }
                    last_reason = "SendInput delivered no events";
                } else if !restored {
                    last_reason = "could not restore target window";
                } else {
                    last_reason = "focus left captured window";
                }
            }
            restore_clipboard_if_unchanged(text, previous.as_deref());
            Err(AppError::TextInsertionFailed(last_reason.into()))
        }
    }

    struct ThreadAttachGuard {
        pairs: Vec<(u32, u32)>,
    }

    impl Drop for ThreadAttachGuard {
        fn drop(&mut self) {
            for (from, to) in &self.pairs {
                attach_thread_input(*from, *to, false);
            }
        }
    }

    unsafe fn attach_input(target: HWND) -> ThreadAttachGuard {
        use windows::Win32::System::Threading::GetCurrentThreadId;
        use windows::Win32::UI::WindowsAndMessaging::AllowSetForegroundWindow;

        let _ = AllowSetForegroundWindow(u32::MAX);
        let our_tid = GetCurrentThreadId();
        let foreground = GetForegroundWindow();
        let target_tid = window_thread_id(target);
        let fg_tid = if foreground.is_invalid() {
            0
        } else {
            window_thread_id(foreground)
        };
        let mut pairs = Vec::new();
        if fg_tid != 0 && fg_tid != our_tid {
            attach_thread_input(our_tid, fg_tid, true);
            pairs.push((our_tid, fg_tid));
        }
        if target_tid != 0 && target_tid != our_tid && target_tid != fg_tid {
            attach_thread_input(our_tid, target_tid, true);
            pairs.push((our_tid, target_tid));
        }
        ThreadAttachGuard { pairs }
    }

    unsafe fn root_hwnd(hwnd: HWND) -> HWND {
        use windows::Win32::UI::WindowsAndMessaging::{GetAncestor, GA_ROOT};
        if hwnd.is_invalid() {
            return hwnd;
        }
        let root = GetAncestor(hwnd, GA_ROOT);
        if root.is_invalid() {
            hwnd
        } else {
            root
        }
    }

    unsafe fn hwnd_root_value(hwnd: HWND) -> Option<usize> {
        if hwnd.is_invalid() {
            None
        } else {
            Some(root_hwnd(hwnd).0 as usize)
        }
    }

    unsafe fn window_thread_id(hwnd: HWND) -> u32 {
        GetWindowThreadProcessId(hwnd, None)
    }

    unsafe fn thread_focus_hwnd(hwnd: HWND) -> Option<HWND> {
        use windows::Win32::UI::WindowsAndMessaging::{GetGUIThreadInfo, GUITHREADINFO};
        let tid = window_thread_id(hwnd);
        if tid == 0 {
            return None;
        }
        let mut info = GUITHREADINFO {
            cbSize: std::mem::size_of::<GUITHREADINFO>() as u32,
            ..Default::default()
        };
        if GetGUIThreadInfo(tid, &mut info).is_ok() && !info.hwndFocus.is_invalid() {
            Some(info.hwndFocus)
        } else {
            None
        }
    }

    fn attach_thread_input(from: u32, to: u32, attach: bool) {
        #[link(name = "user32")]
        extern "system" {
            fn AttachThreadInput(id_attach: u32, id_attach_to: u32, f_attach: i32) -> i32;
        }
        unsafe {
            AttachThreadInput(from, to, i32::from(attach));
        }
    }

    pub fn clipboard_paste(text: &str) -> Result<Option<String>, AppError> {
        unsafe {
            let previous = snapshot_clipboard();
            clipboard_copy(text)?;
            let inputs = [
                key(VK_CONTROL.0, false),
                key(VK_V.0, false),
                key(VK_V.0, true),
                key(VK_CONTROL.0, true),
            ];
            let sent = SendInput(&inputs, std::mem::size_of::<INPUT>() as i32);
            if !insert_delivered(sent, true) {
                restore_clipboard_if_unchanged(text, previous.as_deref());
                return Err(AppError::TextInsertionFailed(
                    "SendInput delivered no events".into(),
                ));
            }
            std::thread::sleep(std::time::Duration::from_millis(40));
            restore_clipboard_if_unchanged(text, previous.as_deref());
            Ok(previous)
        }
    }

    pub fn clipboard_copy(text: &str) -> Result<(), AppError> {
        unsafe {
            open_clipboard()?;
            if let Err(err) = write_clipboard(text) {
                CloseClipboard().ok();
                return Err(err);
            }
            CloseClipboard().map_err(|e| AppError::TextInsertionFailed(e.to_string()))?;
            Ok(())
        }
    }

    unsafe fn snapshot_clipboard() -> Option<String> {
        if open_clipboard().is_err() {
            return None;
        }
        let previous = read_unicode_clipboard();
        CloseClipboard().ok();
        previous
    }

    fn restore_clipboard_if_unchanged(ours: &str, previous: Option<&str>) {
        unsafe {
            if open_clipboard().is_err() {
                return;
            }
            let current = read_unicode_clipboard();
            CloseClipboard().ok();
            if !should_restore_clipboard(ours, current.as_deref()) {
                return;
            }
            restore_clipboard(previous);
        }
    }

    pub fn restore_clipboard(previous: Option<&str>) {
        match previous {
            Some(text) => {
                let _ = clipboard_copy(text);
            }
            None => unsafe {
                if open_clipboard().is_ok() {
                    EmptyClipboard().ok();
                    CloseClipboard().ok();
                }
            },
        }
    }

    unsafe fn open_clipboard() -> Result<(), AppError> {
        for _ in 0..20 {
            if OpenClipboard(HWND::default()).is_ok() {
                return Ok(());
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        Err(AppError::TextInsertionFailed("clipboard busy".into()))
    }

    unsafe fn write_clipboard(text: &str) -> Result<(), AppError> {
        EmptyClipboard().map_err(|e| AppError::TextInsertionFailed(e.to_string()))?;
        let encoded: Vec<u16> = text.encode_utf16().chain(std::iter::once(0)).collect();
        let bytes = encoded.len() * 2;
        let handle = GlobalAlloc(GMEM_MOVEABLE, bytes)
            .map_err(|e| AppError::TextInsertionFailed(e.to_string()))?;
        let ptr = GlobalLock(handle);
        if ptr.is_null() {
            return Err(AppError::TextInsertionFailed(
                "clipboard lock failed".into(),
            ));
        }
        std::ptr::copy_nonoverlapping(encoded.as_ptr() as *const u8, ptr as *mut u8, bytes);
        GlobalUnlock(handle).ok();
        SetClipboardData(13, HANDLE(handle.0))
            .map_err(|e| AppError::TextInsertionFailed(e.to_string()))?;
        Ok(())
    }

    fn key(vk: u16, up: bool) -> INPUT {
        use windows::Win32::UI::Input::KeyboardAndMouse::VIRTUAL_KEY;
        INPUT {
            r#type: INPUT_KEYBOARD,
            Anonymous: INPUT_0 {
                ki: KEYBDINPUT {
                    wVk: VIRTUAL_KEY(vk),
                    wScan: 0,
                    dwFlags: if up {
                        KEYEVENTF_KEYUP
                    } else {
                        Default::default()
                    },
                    time: 0,
                    dwExtraInfo: 0,
                },
            },
        }
    }

    unsafe fn read_unicode_clipboard() -> Option<String> {
        let handle = GetClipboardData(13).ok()?;
        if handle.0.is_null() {
            return None;
        }
        let ptr = GlobalLock(windows::Win32::Foundation::HGLOBAL(handle.0)) as *const u16;
        if ptr.is_null() {
            return None;
        }
        let mut len = 0usize;
        while *ptr.add(len) != 0 {
            len += 1;
        }
        let slice = std::slice::from_raw_parts(ptr, len);
        let text = String::from_utf16_lossy(slice);
        GlobalUnlock(windows::Win32::Foundation::HGLOBAL(handle.0)).ok();
        Some(text)
    }
}

#[cfg(not(windows))]
pub mod native {
    use super::NativeHwnd;
    use crate::error::AppError;

    pub fn foreground_hwnd() -> Option<NativeHwnd> {
        None
    }

    pub fn capture_target() -> Option<NativeHwnd> {
        None
    }

    pub fn insert_unicode(_hwnd: NativeHwnd, _text: &str) -> Result<(), AppError> {
        Err(AppError::TextInsertionFailed("not windows".into()))
    }

    pub fn insert_into_window(hwnd: NativeHwnd, text: &str) -> Result<(), AppError> {
        insert_unicode(hwnd, text)
    }

    pub fn clipboard_paste(_text: &str) -> Result<Option<String>, AppError> {
        Err(AppError::TextInsertionFailed("not windows".into()))
    }

    pub fn clipboard_copy(_text: &str) -> Result<(), AppError> {
        Err(AppError::TextInsertionFailed("not windows".into()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn same_window_inserts() {
        let hwnd = NativeHwnd { value: 42 };
        assert_eq!(
            decide_insert(Some(hwnd), Some(hwnd)),
            InsertDecision::InsertIntoCaptured
        );
    }

    #[test]
    fn overlay_does_not_block_insert() {
        assert_eq!(
            decide_insert(Some(NativeHwnd { value: 1 }), Some(NativeHwnd { value: 2 })),
            InsertDecision::InsertIntoCaptured
        );
    }

    #[test]
    fn key_paste_stays_in_captured_window_tree() {
        assert!(should_send_key_paste(10, Some(10), Some(10)));
        assert!(!should_send_key_paste(10, Some(10), Some(11)));
        assert!(!should_send_key_paste(10, Some(11), Some(10)));
        assert!(!should_send_key_paste(10, Some(10), None));
        assert!(should_post_paste(10, Some(10)));
        assert!(!should_post_paste(10, Some(11)));
        assert!(!should_post_paste(10, None));
    }

    #[test]
    fn two_cursor_roots_do_not_cross_paste() {
        let agents = 100;
        let ide = 200;
        let overlay = 300;
        assert!(!should_send_key_paste(agents, Some(overlay), Some(ide)));
        assert!(!should_send_key_paste(agents, Some(ide), Some(ide)));
        assert!(should_send_key_paste(agents, Some(agents), Some(agents)));
        assert!(overlay_blocks_insert(Some(overlay), Some(overlay)));
        assert!(!overlay_blocks_insert(Some(overlay), Some(agents)));
    }

    #[test]
    fn no_success_without_delivery() {
        assert!(!insert_delivered(0, true));
        assert!(!insert_delivered(4, false));
        assert!(insert_delivered(4, true));
    }

    #[test]
    fn clipboard_user_change_skips_restore() {
        assert!(should_restore_clipboard("voxely", Some("voxely")));
        assert!(!should_restore_clipboard("voxely", Some("user copied")));
        assert!(!should_restore_clipboard("voxely", None));
    }
}
