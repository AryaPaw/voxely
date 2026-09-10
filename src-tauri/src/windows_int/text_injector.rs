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

pub fn should_post_paste(captured_root: usize, focus_root: Option<usize>) -> bool {
    matches!(focus_root, Some(root) if root == captured_root)
}

#[cfg(windows)]
pub mod native {
    use super::{should_post_paste, should_send_key_paste, NativeHwnd};
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
            if hwnd.is_invalid() {
                None
            } else {
                Some(NativeHwnd {
                    value: hwnd.0 as usize,
                })
            }
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
            clipboard_copy(text)?;
            let captured_root_hwnd = root_hwnd(target);
            let captured_root = captured_root_hwnd.0 as usize;
            let in_tree = in_tree_paste_hwnd(target).unwrap_or(target);
            focus_window(target);
            std::thread::sleep(std::time::Duration::from_millis(50));
            let mut foreground_root = hwnd_root_value(GetForegroundWindow());
            let mut focus_root = hwnd_root_value(thread_focus_hwnd(target).unwrap_or(in_tree));
            if !should_send_key_paste(captured_root, foreground_root, focus_root) {
                focus_window(target);
                std::thread::sleep(std::time::Duration::from_millis(30));
                foreground_root = hwnd_root_value(GetForegroundWindow());
                focus_root = hwnd_root_value(thread_focus_hwnd(target).unwrap_or(in_tree));
            }
            if should_send_key_paste(captured_root, foreground_root, focus_root) {
                let inputs = [
                    key(VK_CONTROL.0, false),
                    key(VK_V.0, false),
                    key(VK_V.0, true),
                    key(VK_CONTROL.0, true),
                ];
                let _ = SendInput(&inputs, std::mem::size_of::<INPUT>() as i32);
            } else if should_post_paste(captured_root, hwnd_root_value(in_tree)) {
                post_paste(in_tree);
            }
            Ok(())
        }
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

    unsafe fn in_tree_paste_hwnd(captured: HWND) -> Option<HWND> {
        if let Some(focus) = thread_focus_hwnd(captured) {
            if hwnd_root_value(focus) == hwnd_root_value(captured) {
                return Some(focus);
            }
        }
        Some(root_hwnd(captured))
    }

    unsafe fn post_paste(target: HWND) {
        use windows::Win32::Foundation::{LPARAM, WPARAM};
        use windows::Win32::UI::WindowsAndMessaging::PostMessageW;
        const WM_PASTE: u32 = 0x0302;
        let _ = PostMessageW(target, WM_PASTE, WPARAM(0), LPARAM(0));
    }

    unsafe fn focus_window(target: HWND) {
        use windows::Win32::System::Threading::GetCurrentThreadId;
        use windows::Win32::UI::WindowsAndMessaging::AllowSetForegroundWindow;

        let _ = AllowSetForegroundWindow(u32::MAX);
        let foreground = GetForegroundWindow();
        let target_tid = window_thread_id(target);
        let our_tid = GetCurrentThreadId();
        let fg_tid = if foreground.is_invalid() {
            0
        } else {
            window_thread_id(foreground)
        };
        if fg_tid != 0 && fg_tid != our_tid {
            attach_thread_input(our_tid, fg_tid, true);
        }
        if target_tid != 0 && target_tid != our_tid {
            attach_thread_input(our_tid, target_tid, true);
        }
        let _ = SetForegroundWindow(target);
        if fg_tid != 0 && fg_tid != our_tid {
            attach_thread_input(our_tid, fg_tid, false);
        }
        if target_tid != 0 && target_tid != our_tid {
            attach_thread_input(our_tid, target_tid, false);
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
            open_clipboard()?;
            let previous = read_unicode_clipboard();
            if let Err(err) = write_clipboard(text) {
                CloseClipboard().ok();
                return Err(err);
            }
            CloseClipboard().map_err(|e| AppError::TextInsertionFailed(e.to_string()))?;
            let inputs = [
                key(VK_CONTROL.0, false),
                key(VK_V.0, false),
                key(VK_V.0, true),
                key(VK_CONTROL.0, true),
            ];
            SendInput(&inputs, std::mem::size_of::<INPUT>() as i32);
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

    pub fn restore_clipboard(previous: Option<&str>) {
        if previous.is_none() {
            return;
        }
        let _ = previous;
    }
}

#[cfg(not(windows))]
pub mod native {
    use super::NativeHwnd;
    use crate::error::AppError;

    pub fn foreground_hwnd() -> Option<NativeHwnd> {
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
}
