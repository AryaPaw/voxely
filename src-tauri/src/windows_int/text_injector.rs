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

pub fn edit_class_uses_vk_return(class: &str) -> bool {
    let class = class.trim();
    class.eq_ignore_ascii_case("edit") || class.to_ascii_lowercase().starts_with("richedit")
}

pub fn capture_skips_voxely_roots(
    candidate_root: usize,
    overlay_root: Option<usize>,
    main_root: Option<usize>,
) -> bool {
    overlay_root == Some(candidate_root) || main_root == Some(candidate_root)
}

pub fn should_queue_insert_keys(captured_root: usize, foreground_root: Option<usize>) -> bool {
    should_post_paste(captured_root, foreground_root)
}

pub fn insertion_mode_queues_keys(mode: &str) -> bool {
    mode != "clipboard"
}

pub fn resolve_insert_target(
    start: Option<NativeHwnd>,
    live: Option<NativeHwnd>,
    overlay: Option<NativeHwnd>,
    main: Option<NativeHwnd>,
) -> Option<NativeHwnd> {
    let overlay_root = overlay.map(|hwnd| hwnd.value);
    let main_root = main.map(|hwnd| hwnd.value);
    if let Some(live) = live {
        if !capture_skips_voxely_roots(live.value, overlay_root, main_root) {
            return Some(live);
        }
    }
    start.filter(|hwnd| !capture_skips_voxely_roots(hwnd.value, overlay_root, main_root))
}

pub fn hotkey_wait_complete(elapsed_ms: u32, still_down: bool, timeout_ms: u32) -> bool {
    !still_down || elapsed_ms >= timeout_ms
}

pub fn hotkey_keys_to_release(spec: &str) -> Vec<u16> {
    let mut keys = Vec::new();
    for token in spec.split('+') {
        match token.trim() {
            t if t.eq_ignore_ascii_case("Ctrl") || t.eq_ignore_ascii_case("Control") => {
                keys.push(0x11);
            }
            t if t.eq_ignore_ascii_case("Shift") => keys.push(0x10),
            t if t.eq_ignore_ascii_case("Alt") => keys.push(0x12),
            t if t.eq_ignore_ascii_case("Win") || t.eq_ignore_ascii_case("Meta") => {
                keys.push(0x5B);
            }
            t if t.eq_ignore_ascii_case("Space") => keys.push(0x20),
            t if t.eq_ignore_ascii_case("Left") => keys.push(0x25),
            t if t.eq_ignore_ascii_case("Up") => keys.push(0x26),
            t if t.eq_ignore_ascii_case("Right") => keys.push(0x27),
            t if t.eq_ignore_ascii_case("Down") => keys.push(0x28),
            _ => {}
        }
    }
    keys
}

pub fn prefix_release_keys(hotkey: &str) -> Vec<u16> {
    let mut keys = hotkey_keys_to_release(hotkey);
    for extra in [0x20u16, 0x25, 0x26, 0x27, 0x28] {
        if !keys.contains(&extra) {
            keys.push(extra);
        }
    }
    keys
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InsertKey {
    Unicode(u16),
    VirtualKey(u16),
}

pub fn plan_insert_units(text: &str) -> Vec<InsertKey> {
    plan_insert_units_for_class(text, None)
}

pub fn plan_insert_units_for_class(text: &str, class: Option<&str>) -> Vec<InsertKey> {
    let vk_return = class.is_some_and(edit_class_uses_vk_return);
    let units = utf16_code_units(text);
    let mut out = Vec::with_capacity(units.len());
    let mut i = 0;
    while i < units.len() {
        let unit = units[i];
        if unit == 0x0D && units.get(i + 1) == Some(&0x0A) {
            out.push(if vk_return {
                InsertKey::VirtualKey(0x0D)
            } else {
                InsertKey::Unicode(0x0A)
            });
            i += 2;
            continue;
        }
        out.push(match unit {
            0x09 if vk_return => InsertKey::VirtualKey(0x09),
            0x0A | 0x0D if vk_return => InsertKey::VirtualKey(0x0D),
            _ => InsertKey::Unicode(unit),
        });
        i += 1;
    }
    out
}

pub fn unicode_send_count(text: &str) -> u32 {
    plan_insert_units(text).len() as u32 * 2
}

pub fn insert_transcript_now(
    mode: &str,
    captured: Option<NativeHwnd>,
    overlay: Option<NativeHwnd>,
    text: &str,
    abort: impl Fn() -> bool,
) -> Result<&'static str, crate::error::AppError> {
    use crate::error::AppError;
    if abort() {
        return Err(AppError::Cancelled);
    }
    match mode {
        "clipboard" => {
            native::clipboard_copy(text)?;
            Ok("copied")
        }
        _ => {
            let hwnd = captured
                .ok_or_else(|| AppError::TextInsertionFailed("no captured window".into()))?;
            match native::insert_unicode_while(hwnd, text, abort, overlay) {
                Ok(()) => Ok("inserted"),
                Err(AppError::Cancelled) => Err(AppError::Cancelled),
                Err(_) => {
                    native::clipboard_copy(text)?;
                    Ok("copied")
                }
            }
        }
    }
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
        IsIconic, IsWindow, SetForegroundWindow, ShowWindow, SwitchToThisWindow, SW_RESTORE,
    };

    pub fn foreground_hwnd() -> Option<NativeHwnd> {
        unsafe {
            let hwnd = GetForegroundWindow();
            hwnd_to_native(hwnd)
        }
    }

    pub fn capture_target() -> Option<NativeHwnd> {
        capture_target_excluding(&[])
    }

    pub fn capture_target_excluding(skip_roots: &[usize]) -> Option<NativeHwnd> {
        unsafe {
            let foreground = GetForegroundWindow();
            hwnd_to_native(foreground)?;
            let captured = if let Some(focus) = thread_focus_hwnd(foreground) {
                if hwnd_root_value(focus) == hwnd_root_value(foreground) {
                    hwnd_to_native(focus)
                } else {
                    hwnd_to_native(foreground)
                }
            } else {
                hwnd_to_native(foreground)
            }?;
            let root = hwnd_root_value(HWND(captured.value as *mut _))?;
            if skip_roots.contains(&root) {
                return None;
            }
            Some(captured)
        }
    }

    pub fn wait_for_keys_up(keys: &[u16], timeout: std::time::Duration) {
        let start = std::time::Instant::now();
        let poll = expand_keys_to_poll(keys);
        loop {
            let still_down = poll.iter().any(|vk| vk_down(*vk));
            if super::hotkey_wait_complete(
                start.elapsed().as_millis() as u32,
                still_down,
                timeout.as_millis() as u32,
            ) {
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(8));
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
        insert_unicode_while(hwnd, text, || false, None)
    }

    pub fn insert_unicode_while(
        hwnd: NativeHwnd,
        text: &str,
        abort: impl Fn() -> bool,
        overlay: Option<NativeHwnd>,
    ) -> Result<(), AppError> {
        insert_into_window(hwnd, text, abort, overlay)
    }

    pub fn insert_into_window(
        hwnd: NativeHwnd,
        text: &str,
        abort: impl Fn() -> bool,
        overlay: Option<NativeHwnd>,
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
            let overlay_root = overlay.and_then(|h| hwnd_root_value(HWND(h.value as *mut _)));
            let mut last_reason = "focus left captured window";
            for _ in 0..6 {
                if abort() {
                    return Err(AppError::Cancelled);
                }
                let foreground_root = hwnd_root_value(GetForegroundWindow());
                if overlay_blocks_insert(overlay_root, foreground_root) {
                    last_reason = "overlay still foreground";
                    std::thread::sleep(std::time::Duration::from_millis(16));
                    continue;
                }
                if !super::should_queue_insert_keys(captured_root, foreground_root) {
                    last_reason = "foreground is not captured window";
                    break;
                }
                let _guard = attach_input(target);
                if may_restore_foreground(target, overlay_root) {
                    let _ = restore_foreground(target);
                }
                std::thread::sleep(std::time::Duration::from_millis(16));
                if abort() {
                    return Err(AppError::Cancelled);
                }
                let foreground_root = hwnd_root_value(GetForegroundWindow());
                if overlay_blocks_insert(overlay_root, foreground_root)
                    || !super::should_queue_insert_keys(captured_root, foreground_root)
                {
                    last_reason = "foreground is not captured window";
                    break;
                }
                let focus_root = hwnd_root_value(thread_focus_hwnd(target).unwrap_or(target));
                if should_send_key_paste(captured_root, foreground_root, focus_root) {
                    let class = window_class_name(target);
                    match send_insert_keys(text, class.as_deref(), &abort) {
                        Ok(()) => return Ok(()),
                        Err("aborted") => return Err(AppError::Cancelled),
                        Err(reason) => last_reason = reason,
                    }
                } else {
                    last_reason = "focus left captured window";
                    break;
                }
            }
            Err(AppError::TextInsertionFailed(last_reason.into()))
        }
    }

    fn expand_keys_to_poll(keys: &[u16]) -> Vec<u16> {
        let mut out = Vec::new();
        for vk in keys {
            match *vk {
                0x11 => {
                    out.push(VK_LCONTROL.0);
                    out.push(VK_RCONTROL.0);
                }
                0x10 => {
                    out.push(VK_LSHIFT.0);
                    out.push(VK_RSHIFT.0);
                }
                0x12 => {
                    out.push(VK_LMENU.0);
                    out.push(VK_RMENU.0);
                }
                0x5B => {
                    out.push(VK_LWIN.0);
                    out.push(VK_RWIN.0);
                }
                other => out.push(other),
            }
        }
        out
    }

    fn send_insert_keys(
        text: &str,
        class: Option<&str>,
        abort: impl Fn() -> bool,
    ) -> Result<(), &'static str> {
        if abort() {
            return Err("aborted");
        }
        let mut prefix = Vec::new();
        for vk in expand_keys_to_poll(&super::prefix_release_keys("Ctrl+Shift+Space")) {
            if vk_down(vk) {
                prefix.push(key(vk, true));
            }
        }
        send_all(&prefix)?;
        if !prefix.is_empty() {
            std::thread::sleep(std::time::Duration::from_millis(8));
        }
        if abort() {
            return Err("aborted");
        }
        let planned = super::plan_insert_units_for_class(text, class);
        let mut inputs = Vec::with_capacity(planned.len() * 2);
        for key_plan in planned {
            match key_plan {
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
        send_all(&inputs)
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
        SwitchToThisWindow(target, true);
        let ok = SetForegroundWindow(target).as_bool();
        hwnd_root_value(GetForegroundWindow()) == hwnd_root_value(target) || ok
    }

    unsafe fn may_restore_foreground(target: HWND, overlay_root: Option<usize>) -> bool {
        let foreground_root = hwnd_root_value(GetForegroundWindow());
        let captured_root = hwnd_root_value(target);
        captured_root == foreground_root || overlay_blocks_insert(overlay_root, foreground_root)
    }

    unsafe fn window_class_name(hwnd: HWND) -> Option<String> {
        use windows::Win32::UI::WindowsAndMessaging::GetClassNameW;
        let mut buf = [0u16; 256];
        let n = GetClassNameW(hwnd, &mut buf);
        if n == 0 {
            return None;
        }
        Some(String::from_utf16_lossy(&buf[..n as usize]))
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

    pub fn capture_target_excluding(_skip_roots: &[usize]) -> Option<NativeHwnd> {
        None
    }

    pub fn wait_for_keys_up(_keys: &[u16], _timeout: std::time::Duration) {}

    pub fn insert_unicode(_hwnd: NativeHwnd, _text: &str) -> Result<(), AppError> {
        Err(AppError::TextInsertionFailed("not windows".into()))
    }

    pub fn insert_unicode_while(
        hwnd: NativeHwnd,
        text: &str,
        abort: impl Fn() -> bool,
        overlay: Option<NativeHwnd>,
    ) -> Result<(), AppError> {
        let _ = overlay;
        if abort() {
            return Err(AppError::Cancelled);
        }
        insert_unicode(hwnd, text)
    }

    pub fn insert_into_window(
        hwnd: NativeHwnd,
        text: &str,
        abort: impl Fn() -> bool,
        overlay: Option<NativeHwnd>,
    ) -> Result<(), AppError> {
        insert_unicode_while(hwnd, text, abort, overlay)
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
        assert_eq!(unicode_send_count("A"), 2);
        assert_eq!(unicode_send_count("😀"), 4);
        assert_eq!(unicode_send_count("A B"), 6);
        assert_eq!(
            plan_insert_units("hi there"),
            vec![
                InsertKey::Unicode(b'h' as u16),
                InsertKey::Unicode(b'i' as u16),
                InsertKey::Unicode(b' ' as u16),
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
                InsertKey::Unicode(0x0A),
                InsertKey::Unicode(b'b' as u16),
            ]
        );
    }

    #[test]
    fn plan_insert_units_newlines_and_tab_are_unicode() {
        assert_eq!(
            plan_insert_units("a\nb"),
            vec![
                InsertKey::Unicode(b'a' as u16),
                InsertKey::Unicode(0x0A),
                InsertKey::Unicode(b'b' as u16),
            ]
        );
        assert_eq!(
            plan_insert_units("a\tb"),
            vec![
                InsertKey::Unicode(b'a' as u16),
                InsertKey::Unicode(0x09),
                InsertKey::Unicode(b'b' as u16),
            ]
        );
        assert!(!plan_insert_units("a\nb")
            .iter()
            .any(|k| matches!(*k, InsertKey::VirtualKey(_))));
        assert_eq!(
            plan_insert_units_for_class("a\nb", Some("Edit")),
            vec![
                InsertKey::Unicode(b'a' as u16),
                InsertKey::VirtualKey(0x0D),
                InsertKey::Unicode(b'b' as u16),
            ]
        );
        assert!(edit_class_uses_vk_return("RICHEDIT50W"));
        assert!(!edit_class_uses_vk_return("Chrome_WidgetWin_1"));
    }

    #[test]
    fn plan_insert_units_space_is_unicode_not_vk_space() {
        assert_eq!(plan_insert_units(" "), vec![InsertKey::Unicode(0x20)]);
        assert!(!plan_insert_units(" ")
            .iter()
            .any(|k| matches!(*k, InsertKey::VirtualKey(0x20))));
    }

    #[test]
    fn hotkey_keys_include_space_only_when_in_spec() {
        let keys = hotkey_keys_to_release("Ctrl+Shift+Space");
        assert!(keys.contains(&0x20));
        assert!(keys.contains(&0x11));
        assert!(keys.contains(&0x10));
        assert!(!hotkey_keys_to_release("Ctrl+Shift+Q").contains(&0x20));
        assert!(prefix_release_keys("Ctrl+Shift+Q").contains(&0x20));
        assert!(prefix_release_keys("Ctrl+Shift+Q").contains(&0x25));
    }

    #[test]
    fn clipboard_mode_never_queues_keys() {
        assert!(!insertion_mode_queues_keys("clipboard"));
        assert!(insertion_mode_queues_keys("unicode"));
    }

    #[test]
    fn never_plan_sendinput_when_foreground_root_differs() {
        assert!(!should_queue_insert_keys(10, Some(11)));
        assert!(should_queue_insert_keys(10, Some(10)));
        assert!(!should_queue_insert_keys(10, None));
    }

    #[test]
    fn insert_follows_live_focus_not_start_capture() {
        let desktop = NativeHwnd { value: 1 };
        let field = NativeHwnd { value: 2 };
        let overlay = NativeHwnd { value: 3 };
        let main = NativeHwnd { value: 4 };
        assert_eq!(
            resolve_insert_target(Some(desktop), Some(field), Some(overlay), Some(main)),
            Some(field)
        );
        assert_eq!(
            resolve_insert_target(Some(desktop), Some(overlay), Some(overlay), Some(main)),
            Some(desktop)
        );
        assert_eq!(
            resolve_insert_target(Some(overlay), Some(overlay), Some(overlay), Some(main)),
            None
        );
    }

    #[test]
    fn capture_skips_overlay_and_main_roots() {
        assert!(capture_skips_voxely_roots(1, Some(1), Some(2)));
        assert!(capture_skips_voxely_roots(2, Some(1), Some(2)));
        assert!(!capture_skips_voxely_roots(3, Some(1), Some(2)));
    }

    #[test]
    fn hotkey_wait_completes_on_release_or_timeout() {
        assert!(hotkey_wait_complete(0, false, 300));
        assert!(!hotkey_wait_complete(16, true, 300));
        assert!(hotkey_wait_complete(300, true, 300));
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
