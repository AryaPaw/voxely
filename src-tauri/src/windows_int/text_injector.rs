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

pub fn insert_should_abort(started_generation: u64, current_generation: u64) -> bool {
    started_generation != current_generation
}

pub fn should_send_key_paste(
    captured_root: usize,
    foreground_root: Option<usize>,
    focus_root: Option<usize>,
) -> bool {
    should_post_paste(captured_root, foreground_root)
        && (focus_root.is_none() || should_post_paste(captured_root, focus_root))
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

pub fn utf16_code_units(text: &str) -> Vec<u16> {
    text.encode_utf16().collect()
}

pub const HOTKEY_MODIFIER_UP_COUNT: u32 = 4;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InsertKey {
    Unicode(u16),
    VirtualKey(u16),
}

pub fn plan_insert_units(text: &str) -> Vec<InsertKey> {
    let units = utf16_code_units(text);
    let mut out = Vec::with_capacity(units.len());
    let mut i = 0;
    while i < units.len() {
        let unit = units[i];
        if unit == 0x0D && units.get(i + 1) == Some(&0x0A) {
            out.push(InsertKey::VirtualKey(0x0D));
            i += 2;
            continue;
        }
        out.push(match unit {
            0x09 => InsertKey::VirtualKey(0x09),
            0x0A | 0x0D => InsertKey::VirtualKey(0x0D),
            0x20 => InsertKey::VirtualKey(0x20),
            _ => InsertKey::Unicode(unit),
        });
        i += 1;
    }
    out
}

pub fn unicode_send_count(text: &str) -> u32 {
    let keys = plan_insert_units(text).len() as u32;
    keys.saturating_mul(2)
        .saturating_add(HOTKEY_MODIFIER_UP_COUNT)
}

pub const GITHUB_REPO_URL: &str = "https://github.com/AryaPaw/voxely";
pub const GITHUB_ISSUES_URL: &str = "https://github.com/AryaPaw/voxely/issues";

pub fn github_page_url(page: Option<&str>) -> &'static str {
    match page {
        Some("issues") => GITHUB_ISSUES_URL,
        _ => GITHUB_REPO_URL,
    }
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
        GetAsyncKeyState, SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYEVENTF_KEYUP,
        KEYEVENTF_UNICODE, VIRTUAL_KEY, VK_LCONTROL, VK_LMENU, VK_LSHIFT, VK_LWIN, VK_RCONTROL,
        VK_RMENU, VK_RSHIFT, VK_RWIN,
    };
    use windows::Win32::UI::WindowsAndMessaging::{
        AllowSetForegroundWindow, BringWindowToTop, GetForegroundWindow, GetWindowThreadProcessId,
        IsIconic, IsWindow, SetForegroundWindow, ShowWindow, SW_RESTORE,
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
        insert_unicode_while(hwnd, text, || false)
    }

    pub fn insert_unicode_while(
        hwnd: NativeHwnd,
        text: &str,
        abort: impl Fn() -> bool,
    ) -> Result<(), AppError> {
        insert_into_window(hwnd, text, abort)
    }

    pub fn insert_into_window(
        hwnd: NativeHwnd,
        text: &str,
        abort: impl Fn() -> bool,
    ) -> Result<(), AppError> {
        unsafe {
            let target = HWND(hwnd.value as *mut _);
            if !IsWindow(target).as_bool() {
                return Err(AppError::TextInsertionFailed("window gone".into()));
            }
            if text.is_empty() {
                return Ok(());
            }
            let captured_root_hwnd = root_hwnd(target);
            let captured_root = captured_root_hwnd.0 as usize;
            let mut last_reason = "focus left captured window";
            for _ in 0..4 {
                if abort() {
                    return Err(AppError::Cancelled);
                }
                let _guard = attach_input(target);
                let restored = restore_foreground(target);
                std::thread::sleep(std::time::Duration::from_millis(40));
                if abort() {
                    return Err(AppError::Cancelled);
                }
                let foreground_root = hwnd_root_value(GetForegroundWindow());
                if overlay_blocks_insert(None, foreground_root) {
                    last_reason = "overlay still foreground";
                    continue;
                }
                let focus_root = hwnd_root_value(thread_focus_hwnd(target).unwrap_or(target));
                if should_send_key_paste(captured_root, foreground_root, focus_root) {
                    match send_insert_keys(text, &abort) {
                        Ok(()) => return Ok(()),
                        Err("aborted") => return Err(AppError::Cancelled),
                        Err(reason) => last_reason = reason,
                    }
                } else if !restored {
                    last_reason = "could not restore target window";
                } else {
                    last_reason = "focus left captured window";
                }
            }
            Err(AppError::TextInsertionFailed(last_reason.into()))
        }
    }

    fn send_insert_keys(text: &str, abort: impl Fn() -> bool) -> Result<(), &'static str> {
        if abort() {
            return Err("aborted");
        }
        let mut prefix = Vec::new();
        for vk in [
            VK_LCONTROL.0,
            VK_RCONTROL.0,
            VK_LSHIFT.0,
            VK_RSHIFT.0,
            VK_LMENU.0,
            VK_RMENU.0,
            VK_LWIN.0,
            VK_RWIN.0,
        ] {
            if vk_down(vk) {
                prefix.push(key(vk, true));
            }
        }
        send_all(&prefix)?;
        if !prefix.is_empty() {
            std::thread::sleep(std::time::Duration::from_millis(40));
        }
        let planned = super::plan_insert_units(text);
        for chunk in planned.chunks(8) {
            if abort() {
                return Err("aborted");
            }
            let mut inputs = Vec::with_capacity(chunk.len() * 2);
            for key_plan in chunk {
                match *key_plan {
                    super::InsertKey::Unicode(unit) => {
                        inputs.push(unicode_key(unit, false));
                        inputs.push(unicode_key(unit, true));
                    }
                    super::InsertKey::VirtualKey(vk) => {
                        inputs.push(key(vk, false));
                        inputs.push(key(vk, true));
                    }
                }
            }
            send_all(&inputs)?;
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
        Ok(())
    }

    fn send_all(inputs: &[INPUT]) -> Result<(), &'static str> {
        if inputs.is_empty() {
            return Ok(());
        }
        let sent = unsafe { SendInput(inputs, std::mem::size_of::<INPUT>() as i32) };
        if insert_delivered(sent, true) && sent == inputs.len() as u32 {
            Ok(())
        } else {
            Err("SendInput delivered no events")
        }
    }

    fn vk_down(vk: u16) -> bool {
        unsafe { GetAsyncKeyState(i32::from(vk)) as u16 & 0x8000 != 0 }
    }

    fn unicode_key(unit: u16, up: bool) -> INPUT {
        let mut flags = KEYEVENTF_UNICODE;
        if up {
            flags |= KEYEVENTF_KEYUP;
        }
        INPUT {
            r#type: INPUT_KEYBOARD,
            Anonymous: INPUT_0 {
                ki: KEYBDINPUT {
                    wVk: VIRTUAL_KEY(0),
                    wScan: unit,
                    dwFlags: flags,
                    time: 0,
                    dwExtraInfo: 0,
                },
            },
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

    unsafe fn restore_foreground(target: HWND) -> bool {
        let _ = AllowSetForegroundWindow(u32::MAX);
        if IsIconic(target).as_bool() {
            let _ = ShowWindow(target, SW_RESTORE);
        }
        let _ = BringWindowToTop(target);
        SetForegroundWindow(target).as_bool()
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
        clipboard_copy(text)?;
        Ok(None)
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

    pub fn insert_unicode_while(
        hwnd: NativeHwnd,
        text: &str,
        abort: impl Fn() -> bool,
    ) -> Result<(), AppError> {
        if abort() {
            return Err(AppError::Cancelled);
        }
        insert_unicode(hwnd, text)
    }

    pub fn insert_into_window(
        hwnd: NativeHwnd,
        text: &str,
        abort: impl Fn() -> bool,
    ) -> Result<(), AppError> {
        insert_unicode_while(hwnd, text, abort)
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
        assert!(should_send_key_paste(10, Some(10), None));
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

    #[test]
    fn new_session_aborts_in_flight_insert() {
        assert!(!insert_should_abort(3, 3));
        assert!(insert_should_abort(3, 4));
    }

    #[test]
    fn unicode_send_count_covers_surrogates_spaces_and_hotkey_modifiers() {
        assert_eq!(utf16_code_units("A").len(), 1);
        assert_eq!(utf16_code_units("😀").len(), 2);
        assert_eq!(unicode_send_count("A"), 6);
        assert_eq!(unicode_send_count("😀"), 8);
        assert_eq!(unicode_send_count("A B"), 10);
        assert_eq!(
            plan_insert_units("hi there"),
            vec![
                InsertKey::Unicode(b'h' as u16),
                InsertKey::Unicode(b'i' as u16),
                InsertKey::VirtualKey(0x20),
                InsertKey::Unicode(b't' as u16),
                InsertKey::Unicode(b'h' as u16),
                InsertKey::Unicode(b'e' as u16),
                InsertKey::Unicode(b'r' as u16),
                InsertKey::Unicode(b'e' as u16),
            ]
        );
        assert_eq!(
            plan_insert_units("a\r\nb"),
            vec![
                InsertKey::Unicode(b'a' as u16),
                InsertKey::VirtualKey(0x0D),
                InsertKey::Unicode(b'b' as u16),
            ]
        );
        assert_eq!(HOTKEY_MODIFIER_UP_COUNT, 4);
    }

    #[test]
    fn github_urls_cover_repo_and_issues() {
        assert_eq!(GITHUB_REPO_URL, "https://github.com/AryaPaw/voxely");
        assert_eq!(
            GITHUB_ISSUES_URL,
            "https://github.com/AryaPaw/voxely/issues"
        );
        assert_eq!(github_page_url(Some("issues")), GITHUB_ISSUES_URL);
        assert_eq!(github_page_url(None), GITHUB_REPO_URL);
    }
}
