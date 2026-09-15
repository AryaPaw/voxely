pub use crate::windows_int::insert_engine::*;

pub fn insert_transcript_now(
    mode: &str,
    captured: Option<CapturedTarget>,
    overlay_root: Option<usize>,
    main_root: Option<usize>,
    text: &str,
    abort: impl Fn() -> bool,
    hotkey: &str,
) -> InsertOutcome {
    let abort_ref: &dyn Fn() -> bool = &abort;
    let mut world = native::LiveWorld {
        overlay_root,
        main_root,
    };
    run_insert(
        &mut world,
        InsertRequest {
            mode,
            text,
            captured,
            overlay_root,
            main_root,
            hotkey,
            abort: abort_ref,
        },
    )
}

#[cfg(windows)]
pub mod native {
    use super::{
        classify_chunk_send, CapturedTarget, ChunkSend, InsertKey, InsertWorld, NativeHwnd,
        WorldSnapshot,
    };
    use crate::error::AppError;
    use std::time::Duration;
    use windows::Win32::Foundation::{HANDLE, HWND};
    use windows::Win32::System::DataExchange::{
        CloseClipboard, EmptyClipboard, OpenClipboard, SetClipboardData,
    };
    use windows::Win32::System::Memory::{GlobalAlloc, GlobalLock, GlobalUnlock, GMEM_MOVEABLE};
    use windows::Win32::UI::Input::KeyboardAndMouse::{
        GetAsyncKeyState, SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYEVENTF_KEYUP,
        KEYEVENTF_UNICODE, VIRTUAL_KEY, VK_LCONTROL, VK_LMENU, VK_LSHIFT, VK_LWIN, VK_RCONTROL,
        VK_RMENU, VK_RSHIFT, VK_RWIN,
    };
    use windows::Win32::UI::WindowsAndMessaging::{
        GetForegroundWindow, GetWindowThreadProcessId, IsIconic, IsWindow, SetForegroundWindow,
    };

    pub struct LiveWorld {
        pub overlay_root: Option<usize>,
        pub main_root: Option<usize>,
    }

    impl InsertWorld for LiveWorld {
        fn snapshot(&self, captured: Option<CapturedTarget>) -> WorldSnapshot {
            unsafe {
                let fg = GetForegroundWindow();
                let foreground_root = hwnd_root_value(fg);
                let focus_root = thread_focus_hwnd(fg).and_then(|hwnd| hwnd_root_value(hwnd));
                let (target_alive, target_iconic, window_class, integrity_blocked) =
                    if let Some(target) = captured {
                        let hwnd = HWND(target.hwnd as *mut _);
                        let alive = IsWindow(hwnd).as_bool();
                        (
                            alive,
                            alive && IsIconic(hwnd).as_bool(),
                            if alive { window_class_name(hwnd) } else { None },
                            if alive {
                                integrity_blocked(target.pid)
                            } else {
                                None
                            },
                        )
                    } else {
                        (false, false, None, None)
                    };
                WorldSnapshot {
                    captured,
                    foreground_root,
                    focus_root,
                    overlay_root: self.overlay_root,
                    main_root: self.main_root,
                    target_alive,
                    target_iconic,
                    integrity_blocked,
                    window_class,
                }
            }
        }

        fn restore_foreground(&mut self, target: &CapturedTarget) -> bool {
            unsafe {
                let hwnd = HWND(target.root as *mut _);
                if !IsWindow(hwnd).as_bool() {
                    return false;
                }
                let _guard = attach_input(hwnd);
                let ok = SetForegroundWindow(hwnd).as_bool();
                drop(_guard);
                hwnd_root_value(GetForegroundWindow()) == Some(target.root) || ok
            }
        }

        fn wait_keys_up(&mut self, keys: &[u16], timeout: Duration) {
            wait_for_keys_up(keys, timeout);
        }

        fn send_keys(&mut self, keys: &[InsertKey]) -> ChunkSend {
            let mut inputs = Vec::with_capacity(keys.len() * 2);
            for key_plan in keys {
                match key_plan {
                    InsertKey::Unicode(unit) => {
                        inputs.push(unicode_key(*unit, false));
                        inputs.push(unicode_key(*unit, true));
                    }
                    InsertKey::VirtualKey(vk) => {
                        inputs.push(key(*vk, false));
                        inputs.push(key(*vk, true));
                    }
                }
            }
            if inputs.is_empty() {
                return ChunkSend::Complete;
            }
            let sent = unsafe { SendInput(&inputs, std::mem::size_of::<INPUT>() as i32) };
            classify_chunk_send(sent, inputs.len() as u32)
        }

        fn clipboard_copy(&mut self, text: &str) -> Result<(), AppError> {
            clipboard_copy(text)
        }
    }

    pub fn foreground_hwnd() -> Option<NativeHwnd> {
        unsafe { hwnd_to_native(GetForegroundWindow()) }
    }

    pub fn capture_target() -> Option<NativeHwnd> {
        capture_target_excluding(&[])
    }

    pub fn capture_target_excluding(skip_roots: &[usize]) -> Option<NativeHwnd> {
        inspect_foreground(skip_roots).map(|target| NativeHwnd { value: target.hwnd })
    }

    pub fn capture_session_target(skip_roots: &[usize], generation: u64) -> Option<CapturedTarget> {
        inspect_foreground(skip_roots).map(|mut target| {
            target.generation = generation;
            target
        })
    }

    fn inspect_foreground(skip_roots: &[usize]) -> Option<CapturedTarget> {
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
            let hwnd = HWND(captured.value as *mut _);
            let root = hwnd_root_value(hwnd)?;
            if skip_roots.contains(&root) {
                return None;
            }
            let mut pid = 0u32;
            let tid = GetWindowThreadProcessId(hwnd, Some(&mut pid));
            Some(CapturedTarget {
                hwnd: captured.value,
                root,
                pid,
                tid,
                generation: 0,
            })
        }
    }

    pub fn wait_for_keys_up(keys: &[u16], timeout: Duration) {
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
            std::thread::sleep(Duration::from_millis(8));
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

    fn key(vk: u16, up: bool) -> INPUT {
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

        let our_tid = GetCurrentThreadId();
        let foreground = GetForegroundWindow();
        let target_tid = window_thread_id(target);
        let fg_tid = if foreground.is_invalid() {
            0
        } else {
            window_thread_id(foreground)
        };
        let mut pairs = Vec::new();
        if fg_tid != 0 && fg_tid != our_tid && attach_thread_input(our_tid, fg_tid, true) {
            pairs.push((our_tid, fg_tid));
        }
        if target_tid != 0
            && target_tid != our_tid
            && target_tid != fg_tid
            && attach_thread_input(our_tid, target_tid, true)
        {
            pairs.push((our_tid, target_tid));
        }
        ThreadAttachGuard { pairs }
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

    fn attach_thread_input(from: u32, to: u32, attach: bool) -> bool {
        #[link(name = "user32")]
        extern "system" {
            fn AttachThreadInput(id_attach: u32, id_attach_to: u32, f_attach: i32) -> i32;
        }
        unsafe { AttachThreadInput(from, to, i32::from(attach)) != 0 }
    }

    fn integrity_blocked(target_pid: u32) -> Option<bool> {
        let ours = process_integrity(std::process::id())?;
        let theirs = process_integrity(target_pid)?;
        Some(theirs > ours)
    }

    fn process_integrity(pid: u32) -> Option<u32> {
        use windows::Win32::Foundation::CloseHandle;
        use windows::Win32::Security::{
            GetTokenInformation, TokenIntegrityLevel, TOKEN_MANDATORY_LABEL, TOKEN_QUERY,
        };
        use windows::Win32::System::Threading::{
            OpenProcess, OpenProcessToken, PROCESS_QUERY_LIMITED_INFORMATION,
        };
        unsafe {
            let process = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?;
            let mut token = HANDLE::default();
            if OpenProcessToken(process, TOKEN_QUERY, &mut token).is_err() {
                let _ = CloseHandle(process);
                return None;
            }
            let mut needed = 0u32;
            let _ = GetTokenInformation(token, TokenIntegrityLevel, None, 0, &mut needed);
            if needed == 0 {
                let _ = CloseHandle(token);
                let _ = CloseHandle(process);
                return None;
            }
            let mut buf = vec![0u8; needed as usize];
            let ok = GetTokenInformation(
                token,
                TokenIntegrityLevel,
                Some(buf.as_mut_ptr() as *mut _),
                needed,
                &mut needed,
            )
            .is_ok();
            let _ = CloseHandle(token);
            let _ = CloseHandle(process);
            if !ok {
                return None;
            }
            let label = &*(buf.as_ptr() as *const TOKEN_MANDATORY_LABEL);
            let sid = label.Label.Sid.0 as *const u8;
            if sid.is_null() {
                return None;
            }
            let sub_count = *sid.add(1) as usize;
            let rid_offset = 8 + (sub_count.saturating_sub(1)) * 4;
            let rid = u32::from_le_bytes([
                *sid.add(rid_offset),
                *sid.add(rid_offset + 1),
                *sid.add(rid_offset + 2),
                *sid.add(rid_offset + 3),
            ]);
            Some(rid)
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
            std::thread::sleep(Duration::from_millis(10));
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
}

#[cfg(not(windows))]
pub mod native {
    use super::{CapturedTarget, NativeHwnd};
    use crate::error::AppError;
    use crate::windows_int::insert_engine::{ChunkSend, InsertKey, InsertWorld, WorldSnapshot};
    use std::time::Duration;

    pub struct LiveWorld {
        pub overlay_root: Option<usize>,
        pub main_root: Option<usize>,
    }

    impl InsertWorld for LiveWorld {
        fn snapshot(&self, captured: Option<CapturedTarget>) -> WorldSnapshot {
            WorldSnapshot {
                captured,
                foreground_root: None,
                focus_root: None,
                overlay_root: self.overlay_root,
                main_root: self.main_root,
                target_alive: false,
                target_iconic: false,
                integrity_blocked: None,
                window_class: None,
            }
        }

        fn restore_foreground(&mut self, _target: &CapturedTarget) -> bool {
            false
        }

        fn wait_keys_up(&mut self, _keys: &[u16], _timeout: Duration) {}

        fn send_keys(&mut self, _keys: &[InsertKey]) -> ChunkSend {
            ChunkSend::Zero
        }

        fn clipboard_copy(&mut self, _text: &str) -> Result<(), AppError> {
            Err(AppError::TextInsertionFailed("not windows".into()))
        }
    }

    pub fn foreground_hwnd() -> Option<NativeHwnd> {
        None
    }

    pub fn capture_target() -> Option<NativeHwnd> {
        None
    }

    pub fn capture_target_excluding(_skip_roots: &[usize]) -> Option<NativeHwnd> {
        None
    }

    pub fn capture_session_target(
        _skip_roots: &[usize],
        _generation: u64,
    ) -> Option<CapturedTarget> {
        None
    }

    pub fn wait_for_keys_up(_keys: &[u16], _timeout: Duration) {}

    pub fn clipboard_copy(_text: &str) -> Result<(), AppError> {
        Err(AppError::TextInsertionFailed("not windows".into()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn insert_stays_on_start_capture() {
        let desktop = NativeHwnd { value: 1 };
        let field = NativeHwnd { value: 2 };
        let overlay = NativeHwnd { value: 3 };
        let main = NativeHwnd { value: 4 };
        assert_eq!(
            resolve_insert_target(Some(desktop), Some(field), Some(overlay), Some(main)),
            Some(desktop)
        );
        assert_eq!(
            resolve_insert_target(Some(overlay), Some(field), Some(overlay), Some(main)),
            Some(field)
        );
        assert!(capture_skips_voxely_roots(1, Some(1), Some(2)));
    }

    #[test]
    fn hotkey_wait_completes_on_release_or_timeout() {
        assert!(hotkey_wait_complete(0, false, 300));
        assert!(!hotkey_wait_complete(16, true, 300));
        assert!(hotkey_wait_complete(300, true, 300));
        assert!(hotkey_keys_to_release("Ctrl+Shift+Space").contains(&0x20));
    }

    #[test]
    fn unicode_chunks_keep_emoji_intact() {
        let thumbs = plan_logical_units("👍", None);
        assert_eq!(thumbs.len(), 1);
        assert_eq!(thumbs[0].len(), 2);
        assert_eq!(next_logical_chunk_end(&thumbs, 0, 256), 1);
        assert_eq!(adapt_unicode_chunk_size(512, 80), UNICODE_CHUNK_UNITS_MIN);
        assert_eq!(
            plan_insert_units("a\r\nb"),
            vec![
                InsertKey::Unicode(b'a' as u16),
                InsertKey::Unicode(0x0A),
                InsertKey::Unicode(b'b' as u16),
            ]
        );
    }
}

#[cfg(all(test, windows))]
mod hwnd_tests {
    use super::*;
    use windows::core::w;
    use windows::Win32::Foundation::HWND;
    use windows::Win32::UI::WindowsAndMessaging::{
        CreateWindowExW, DestroyWindow, GetWindowTextW, ShowWindow, SW_SHOW, WS_OVERLAPPEDWINDOW,
        WS_VISIBLE,
    };

    #[test]
    fn edit_window_accepts_cyrillic_and_emoji_without_replay() {
        unsafe {
            let Ok(edit) = CreateWindowExW(
                Default::default(),
                w!("EDIT"),
                w!(""),
                WS_OVERLAPPEDWINDOW | WS_VISIBLE,
                40,
                40,
                400,
                200,
                HWND::default(),
                None,
                None,
                None,
            ) else {
                return;
            };
            let _ = ShowWindow(edit, SW_SHOW);
            let target = CapturedTarget {
                hwnd: edit.0 as usize,
                root: edit.0 as usize,
                pid: std::process::id(),
                tid: 0,
                generation: 1,
            };
            let mut world = native::LiveWorld {
                overlay_root: None,
                main_root: None,
            };
            let restored = world.restore_foreground(&target);
            let never = || false;
            let text = "Привет 👍";
            let outcome = run_insert(
                &mut world,
                InsertRequest {
                    mode: "unicode",
                    text,
                    captured: Some(target),
                    overlay_root: None,
                    main_root: None,
                    hotkey: "",
                    abort: &never,
                },
            );
            let mut buf = [0u16; 64];
            let n = GetWindowTextW(edit, &mut buf);
            let got = String::from_utf16_lossy(&buf[..n as usize]);
            let _ = DestroyWindow(edit);
            assert_ne!(outcome, InsertOutcome::Failed);
            if restored && outcome == InsertOutcome::Inserted && !got.is_empty() {
                assert_eq!(got, text);
            }
        }
    }
}
