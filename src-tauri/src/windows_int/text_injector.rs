pub use crate::windows_int::insert_engine::*;

pub fn adopt_stop_target(current: &mut Option<CapturedTarget>, at_stop: Option<CapturedTarget>) {
    let Some(at_stop) = at_stop else { return };
    let previous = current.replace(at_stop);
    if let Some(previous) = previous.filter(|old| !old.shares_lifetime_marker(at_stop)) {
        native::release_session_target(previous);
    }
}

pub fn insert_transcript_now(
    mode: &str,
    captured: Option<CapturedTarget>,
    overlay_root: Option<usize>,
    main_root: Option<usize>,
    text: &str,
    abort: impl Fn() -> bool,
    hotkey: &str,
) -> InsertOutcome {
    struct ReleaseTarget(Option<CapturedTarget>);
    impl Drop for ReleaseTarget {
        fn drop(&mut self) {
            if let Some(target) = self.0 {
                native::release_session_target(target);
            }
        }
    }
    let _target_lifetime = ReleaseTarget(captured);
    let abort_ref: &dyn Fn() -> bool = &abort;
    let captured = resolve_captured_insert_target(captured, None, overlay_root, main_root);
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
        GetForegroundWindow, GetPropW, GetWindowThreadProcessId, IsIconic, IsWindow, RemovePropW,
        SetForegroundWindow, SetPropW,
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
                let mut foreground_pid = 0u32;
                if !fg.is_invalid() {
                    GetWindowThreadProcessId(fg, Some(&mut foreground_pid));
                }
                let focus = thread_focus_hwnd(fg);
                let focus_root = focus.and_then(|hwnd| hwnd_root_value(hwnd));
                let (target_alive, target_iconic, window_class, integrity_blocked) =
                    if let Some(target) = captured {
                        let hwnd = HWND(target.hwnd as *mut _);
                        let window_ok = IsWindow(hwnd).as_bool();
                        let root = if window_ok { root_hwnd(hwnd) } else { hwnd };
                        let pid_ok = window_ok && pid_matches(hwnd, target.pid);
                        let root_ok = window_ok && hwnd_root_value(hwnd) == Some(target.root);
                        let thread_ok = window_ok && window_thread_id(hwnd) == target.tid;
                        let marker_ok = window_ok && session_target_marker_matches(&target);
                        let alive = window_ok && pid_ok && root_ok && thread_ok && marker_ok;
                        if !alive {
                            tracing::warn!(
                                generation = target.generation,
                                captured_hwnd = target.hwnd,
                                captured_root = target.root,
                                captured_pid = target.pid,
                                captured_tid = target.tid,
                                window_exists = window_ok,
                                pid_matches = pid_ok,
                                root_matches = root_ok,
                                thread_matches = thread_ok,
                                lifetime_marker_matches = marker_ok,
                                "insertion target identity rejected"
                            );
                        }
                        (
                            alive,
                            alive && IsIconic(root).as_bool(),
                            if alive && focus_root == Some(target.root) {
                                focus.and_then(|hwnd| window_class_name(hwnd))
                            } else {
                                None
                            },
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
                    foreground_pid: if foreground_pid == 0 {
                        None
                    } else {
                        Some(foreground_pid)
                    },
                    focus_root,
                    focus_hwnd: focus.map(|hwnd| hwnd.0 as usize),
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
                let _ = SetForegroundWindow(hwnd);
                drop(_guard);
                hwnd_root_value(GetForegroundWindow()) == Some(target.root)
            }
        }

        fn wait_keys_up(&mut self, keys: &[u16], timeout: Duration) {
            wait_for_keys_up(keys, timeout);
        }

        fn send_keys(&mut self, keys: &[InsertKey]) -> ChunkSend {
            if keys.is_empty() {
                return ChunkSend::Complete;
            }
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
        let captured = inspect_foreground(skip_roots).and_then(|mut target| {
            target.generation = generation;
            bind_session_target(target)
        });
        tracing::info!(
            generation,
            captured_hwnd = captured.map(|target| target.hwnd),
            captured_root = captured.map(|target| target.root),
            captured_pid = captured.map(|target| target.pid),
            captured_tid = captured.map(|target| target.tid),
            "insertion target captured"
        );
        captured
    }

    fn session_target_property() -> windows::core::HSTRING {
        format!("Voxely.InsertTarget.{}", std::process::id()).into()
    }

    pub(super) fn bind_session_target(target: CapturedTarget) -> Option<CapturedTarget> {
        let token = usize::try_from(target.generation).ok()?.checked_add(1)?;
        // Window properties are removed by Windows when the HWND is destroyed.
        // PID/TID/root equality alone cannot detect reuse on the same UI thread.
        unsafe {
            if let Err(error) = SetPropW(
                HWND(target.hwnd as *mut _),
                &session_target_property(),
                HANDLE(token as *mut _),
            ) {
                tracing::warn!(
                    generation = target.generation,
                    captured_hwnd = target.hwnd,
                    error_code = error.code().0,
                    "insertion lifetime marker could not be bound"
                );
                return None;
            }
        }
        Some(target)
    }

    fn session_target_marker_matches(target: &CapturedTarget) -> bool {
        let Some(token) = usize::try_from(target.generation)
            .ok()
            .and_then(|g| g.checked_add(1))
        else {
            return false;
        };
        unsafe {
            GetPropW(HWND(target.hwnd as *mut _), &session_target_property()).0 as usize == token
        }
    }

    pub fn release_session_target(target: CapturedTarget) {
        if session_target_marker_matches(&target) {
            unsafe {
                let _ = RemovePropW(HWND(target.hwnd as *mut _), &session_target_property());
            }
        }
    }

    fn inspect_foreground(skip_roots: &[usize]) -> Option<CapturedTarget> {
        unsafe {
            let foreground = GetForegroundWindow();
            hwnd_to_native(foreground)?;
            // The stable recipient is the original top-level window. Its child
            // controls can be recreated while dictation is in progress.
            let hwnd = root_hwnd(foreground);
            let root = hwnd_root_value(hwnd)?;
            if skip_roots.contains(&root) {
                return None;
            }
            let mut pid = 0u32;
            let tid = GetWindowThreadProcessId(hwnd, Some(&mut pid));
            Some(CapturedTarget {
                hwnd: root,
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

    unsafe fn pid_matches(hwnd: HWND, expected: u32) -> bool {
        let mut pid = 0u32;
        let _ = GetWindowThreadProcessId(hwnd, Some(&mut pid));
        pid != 0 && pid == expected
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
            free_hglobal(handle);
            return Err(AppError::TextInsertionFailed(
                "clipboard lock failed".into(),
            ));
        }
        std::ptr::copy_nonoverlapping(encoded.as_ptr() as *const u8, ptr as *mut u8, bytes);
        GlobalUnlock(handle).ok();
        if let Err(err) = SetClipboardData(13, HANDLE(handle.0)) {
            free_hglobal(handle);
            return Err(AppError::TextInsertionFailed(err.to_string()));
        }
        Ok(())
    }

    fn free_hglobal(handle: windows::Win32::Foundation::HGLOBAL) {
        #[link(name = "kernel32")]
        extern "system" {
            fn GlobalFree(hmem: *mut core::ffi::c_void) -> *mut core::ffi::c_void;
        }
        unsafe {
            let _ = GlobalFree(handle.0);
        }
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
                foreground_pid: None,
                focus_root: None,
                focus_hwnd: None,
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

    pub fn release_session_target(_target: CapturedTarget) {}

    pub fn wait_for_keys_up(_keys: &[u16], _timeout: Duration) {}

    pub fn clipboard_copy(_text: &str) -> Result<(), AppError> {
        Err(AppError::TextInsertionFailed("not windows".into()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn insert_preserves_start_target_without_live_fallback() {
        let desktop = NativeHwnd { value: 1 };
        let field = NativeHwnd { value: 2 };
        let overlay = NativeHwnd { value: 3 };
        let main = NativeHwnd { value: 4 };
        assert_eq!(
            resolve_insert_target(Some(desktop), Some(field), Some(overlay), Some(main)),
            Some(desktop)
        );
        assert_eq!(
            resolve_insert_target(Some(desktop), Some(overlay), Some(overlay), Some(main)),
            Some(desktop)
        );
        assert_eq!(
            resolve_insert_target(None, Some(field), Some(overlay), Some(main)),
            None
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
mod native_tests;

#[cfg(all(test, windows))]
mod hwnd_tests {
    use super::*;
    use windows::core::w;
    use windows::Win32::Foundation::HWND;
    use windows::Win32::UI::WindowsAndMessaging::{
        CreateWindowExW, DestroyWindow, DispatchMessageW, GetForegroundWindow, GetWindowTextW,
        GetWindowThreadProcessId, PeekMessageW, SetForegroundWindow, TranslateMessage, MSG,
        PM_REMOVE, WS_OVERLAPPEDWINDOW, WS_VISIBLE,
    };

    // Opt-in native focus check: uses only owned edit windows and intercepts
    // clipboard delivery so the user's clipboard and applications stay intact.
    #[test]
    #[ignore = "requires an interactive Windows desktop and temporarily changes foreground"]
    fn native_start_target_survives_foreground_switch() {
        unsafe fn settle_foreground(expected: HWND) {
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
            loop {
                let mut message = MSG::default();
                while PeekMessageW(&mut message, None, 0, 0, PM_REMOVE).as_bool() {
                    let _ = TranslateMessage(&message);
                    DispatchMessageW(&message);
                }
                if GetForegroundWindow() == expected || std::time::Instant::now() >= deadline {
                    return;
                }
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
        }
        struct OwnedWindow(HWND);
        impl Drop for OwnedWindow {
            fn drop(&mut self) {
                unsafe {
                    let _ = DestroyWindow(self.0);
                }
            }
        }
        struct FixtureWorld {
            native: native::LiveWorld,
            copied: Option<String>,
        }
        impl InsertWorld for FixtureWorld {
            fn snapshot(&self, target: Option<CapturedTarget>) -> WorldSnapshot {
                self.native.snapshot(target)
            }
            fn restore_foreground(&mut self, _: &CapturedTarget) -> bool {
                panic!("must not steal focus from the other window")
            }
            fn wait_keys_up(&mut self, _: &[u16], _: std::time::Duration) {
                panic!("must not prepare keyboard delivery after a user switch")
            }
            fn send_keys(&mut self, _: &[InsertKey]) -> ChunkSend {
                panic!("must not send any keys to the other window")
            }
            fn clipboard_copy(&mut self, text: &str) -> Result<(), crate::error::AppError> {
                self.copied = Some(text.into());
                Ok(())
            }
        }
        unsafe {
            let original_foreground = GetForegroundWindow();
            let create = |title| {
                OwnedWindow(
                    CreateWindowExW(
                        Default::default(),
                        w!("EDIT"),
                        title,
                        WS_OVERLAPPEDWINDOW | WS_VISIBLE,
                        40,
                        40,
                        400,
                        200,
                        HWND::default(),
                        None,
                        None,
                        None,
                    )
                    .expect("create owned edit window"),
                )
            };
            let first = create(w!("Voxely insertion fixture A"));
            let second = create(w!("Voxely insertion fixture B"));
            struct ForegroundRestore(HWND);
            impl Drop for ForegroundRestore {
                fn drop(&mut self) {
                    unsafe {
                        let _ = SetForegroundWindow(self.0);
                    }
                }
            }
            let _restore = ForegroundRestore(original_foreground);
            let _ = SetForegroundWindow(first.0);
            settle_foreground(first.0);
            if GetForegroundWindow() != first.0
                && std::env::var("VOXELY_NATIVE_FOCUS_WAIT").as_deref() == Ok("1")
            {
                // Automation may not own foreground permission. Give the
                // desktop controller time to activate this exact owned window.
                let deadline = std::time::Instant::now() + std::time::Duration::from_secs(45);
                while GetForegroundWindow() != first.0 && std::time::Instant::now() < deadline {
                    settle_foreground(first.0);
                }
            }
            assert_eq!(
                GetForegroundWindow(),
                first.0,
                "foreground activation denied"
            );
            let start = native::capture_session_target(&[], 42).expect("capture start window");
            assert_eq!(start.root, first.0 .0 as usize);
            assert!(
                SetForegroundWindow(second.0).as_bool(),
                "foreground switch denied"
            );
            settle_foreground(second.0);
            assert_eq!(
                GetForegroundWindow(),
                second.0,
                "switched window must really own foreground"
            );
            let live = CapturedTarget {
                hwnd: second.0 .0 as usize,
                root: second.0 .0 as usize,
                pid: std::process::id(),
                tid: GetWindowThreadProcessId(second.0, None),
                generation: 42,
            };
            let captured = resolve_captured_insert_target(Some(start), Some(live), None, None);
            assert_eq!(captured, Some(start));
            let mut world = FixtureWorld {
                native: native::LiveWorld {
                    overlay_root: None,
                    main_root: None,
                },
                copied: None,
            };
            let never = || false;
            let text = "Private transcript";
            assert_eq!(
                run_insert(
                    &mut world,
                    InsertRequest {
                        mode: "unicode",
                        text,
                        captured,
                        overlay_root: None,
                        main_root: None,
                        hotkey: "",
                        abort: &never,
                    }
                ),
                InsertOutcome::Copied {
                    reason: CopyReason::UserSwitched
                }
            );
            assert_eq!(world.copied.as_deref(), Some(text));
            assert_eq!(GetForegroundWindow(), second.0);
            let mut buffer = [0u16; 64];
            let len = GetWindowTextW(second.0, &mut buffer);
            assert_eq!(
                String::from_utf16_lossy(&buffer[..len as usize]),
                "Voxely insertion fixture B"
            );
            for invalid in [
                CapturedTarget {
                    pid: start.pid.wrapping_add(1),
                    ..start
                },
                CapturedTarget {
                    tid: start.tid.wrapping_add(1),
                    ..start
                },
                CapturedTarget {
                    root: second.0 .0 as usize,
                    ..start
                },
            ] {
                assert!(!world.snapshot(Some(invalid)).target_alive);
            }
            assert!(world.snapshot(Some(start)).target_alive);
            native::release_session_target(start);
            assert!(
                !world.snapshot(Some(start)).target_alive,
                "lost lifetime marker must reject a reused HWND"
            );
            let rebound = native::bind_session_target(CapturedTarget {
                generation: start.generation + 1,
                ..start
            })
            .unwrap();
            assert!(
                !world.snapshot(Some(start)).target_alive,
                "a new lifetime must not validate the old target"
            );
            assert!(world.snapshot(Some(rebound)).target_alive);
            native::release_session_target(rebound);
            native::release_session_target(live);
            drop(first);
            assert!(!world.snapshot(Some(start)).target_alive);
        }
    }
}
