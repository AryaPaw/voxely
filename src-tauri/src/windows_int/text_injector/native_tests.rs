use super::*;
use std::time::{Duration, Instant};
use windows::core::w;
use windows::Win32::Foundation::HWND;
use windows::Win32::System::Threading::GetCurrentThreadId;
use windows::Win32::UI::Input::KeyboardAndMouse::{GetFocus, SetFocus};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DestroyWindow, DispatchMessageW, GetAncestor, GetForegroundWindow,
    GetWindowTextLengthW, GetWindowTextW, GetWindowThreadProcessId, IsWindow, PeekMessageW,
    SetForegroundWindow, SetWindowTextW, ShowWindow, TranslateMessage, ES_AUTOVSCROLL,
    ES_MULTILINE, GA_ROOT, MSG, PM_REMOVE, SW_SHOW, WINDOW_STYLE, WS_CHILD, WS_OVERLAPPEDWINDOW,
    WS_TABSTOP, WS_VISIBLE,
};

struct OwnedEdit(HWND);

impl OwnedEdit {
    fn hidden() -> Self {
        Self(unsafe {
            CreateWindowExW(
                Default::default(),
                w!("EDIT"),
                w!(""),
                WS_OVERLAPPEDWINDOW,
                80,
                80,
                480,
                200,
                HWND::default(),
                None,
                None,
                None,
            )
            .expect("create owned EDIT fixture")
        })
    }

    fn bind(&self, generation: u64) -> CapturedTarget {
        bind_owned_root(self.0, generation)
    }

    fn child(root: HWND) -> Self {
        Self(unsafe {
            CreateWindowExW(
                Default::default(),
                w!("EDIT"),
                w!(""),
                WS_CHILD
                    | WS_VISIBLE
                    | WS_TABSTOP
                    | WINDOW_STYLE(ES_MULTILINE as u32 | ES_AUTOVSCROLL as u32),
                8,
                8,
                440,
                140,
                root,
                None,
                None,
                None,
            )
            .expect("create owned multiline EDIT child")
        })
    }

    fn text_units(&self) -> Vec<u16> {
        let len = unsafe { GetWindowTextLengthW(self.0) };
        let mut buffer = vec![0u16; len as usize + 1];
        let copied = unsafe { GetWindowTextW(self.0, &mut buffer) };
        buffer.truncate(copied as usize);
        buffer
    }

    fn text(&self) -> String {
        String::from_utf16(&self.text_units()).expect("valid EDIT UTF-16")
    }
}

struct OwnedRoot(HWND);

impl OwnedRoot {
    fn hidden() -> Self {
        Self(unsafe {
            CreateWindowExW(
                Default::default(),
                w!("STATIC"),
                w!("Voxely native Unicode fixture - click the input"),
                WS_OVERLAPPEDWINDOW,
                80,
                80,
                480,
                240,
                HWND::default(),
                None,
                None,
                None,
            )
            .expect("create owned root fixture")
        })
    }

    fn bind(&self, generation: u64) -> CapturedTarget {
        bind_owned_root(self.0, generation)
    }
}

impl Drop for OwnedRoot {
    fn drop(&mut self) {
        unsafe {
            let _ = DestroyWindow(self.0);
        }
    }
}

fn bind_owned_root(hwnd: HWND, generation: u64) -> CapturedTarget {
    let mut pid = 0;
    let tid = unsafe { GetWindowThreadProcessId(hwnd, Some(&mut pid)) };
    assert_eq!(pid, std::process::id());
    assert_eq!(tid, unsafe { GetCurrentThreadId() });
    let root = unsafe { GetAncestor(hwnd, GA_ROOT) };
    assert_eq!(hwnd, root, "session identity belongs to the root window");
    native::bind_session_target(CapturedTarget {
        hwnd: root.0 as usize,
        root: root.0 as usize,
        pid,
        tid,
        generation,
    })
    .expect("bind owned root lifetime marker")
}

impl Drop for OwnedEdit {
    fn drop(&mut self) {
        unsafe {
            let _ = DestroyWindow(self.0);
        }
    }
}

struct FixtureWorld {
    native: native::LiveWorld,
    target: CapturedTarget,
    keyboard_allowed: bool,
    expected_focus: usize,
    copied: Option<String>,
}

impl FixtureWorld {
    fn new(target: CapturedTarget, keyboard_allowed: bool) -> Self {
        Self {
            native: native::LiveWorld {
                overlay_root: None,
                main_root: None,
            },
            target,
            keyboard_allowed,
            expected_focus: target.hwnd,
            copied: None,
        }
    }

    fn assert_owned_target(&self, target: &CapturedTarget) {
        assert!(
            self.keyboard_allowed,
            "this fixture prohibits keyboard delivery"
        );
        assert_eq!(
            *target, self.target,
            "only the owned fixture may be targeted"
        );
        assert_eq!(target.pid, std::process::id());
        assert_eq!(target.tid, unsafe { GetCurrentThreadId() });
        assert!(self.native.snapshot(Some(*target)).target_alive);
    }
}

impl InsertWorld for FixtureWorld {
    fn snapshot(&self, captured: Option<CapturedTarget>) -> WorldSnapshot {
        self.native.snapshot(captured)
    }

    fn restore_foreground(&mut self, target: &CapturedTarget) -> bool {
        self.assert_owned_target(target);
        self.native.restore_foreground(target)
    }

    fn wait_keys_up(&mut self, keys: &[u16], timeout: Duration) {
        self.native.wait_keys_up(keys, timeout);
    }

    fn send_keys(&mut self, keys: &[InsertKey]) -> ChunkSend {
        self.assert_owned_target(&self.target);
        let focused = HWND(self.expected_focus as *mut _);
        assert!(unsafe { IsWindow(focused) }.as_bool());
        assert_eq!(
            unsafe { GetAncestor(focused, GA_ROOT) }.0 as usize,
            self.target.root
        );
        let mut focus_pid = 0;
        let focus_tid = unsafe { GetWindowThreadProcessId(focused, Some(&mut focus_pid)) };
        assert_eq!(focus_pid, self.target.pid);
        assert_eq!(focus_tid, self.target.tid);
        let snapshot = self.native.snapshot(Some(self.target));
        assert_eq!(snapshot.foreground_root, Some(self.target.root));
        assert_eq!(snapshot.foreground_pid, Some(std::process::id()));
        assert_eq!(
            unsafe { GetForegroundWindow() }.0 as usize,
            self.target.root,
            "refuse SendInput after foreground leaves the owned fixture"
        );
        assert_eq!(
            unsafe { GetFocus() }.0 as usize,
            self.expected_focus,
            "refuse SendInput unless the owned EDIT has keyboard focus"
        );
        self.native.send_keys(keys)
    }

    fn clipboard_copy(&mut self, text: &str) -> Result<(), crate::error::AppError> {
        // Never open, inspect, or replace the user's clipboard in a native test.
        self.copied = Some(text.to_owned());
        Ok(())
    }
}

fn insert(world: &mut FixtureWorld, text: &str) -> InsertOutcome {
    let captured = Some(world.target);
    run_insert(
        world,
        InsertRequest {
            mode: "unicode",
            text,
            captured,
            overlay_root: None,
            main_root: None,
            hotkey: "",
            abort: &|| false,
        },
    )
}

#[test]
fn hidden_owned_edit_validates_identity_and_lifetime_without_delivery() {
    // No ShowWindow, foreground activation, SendInput, or real clipboard access.
    let edit = OwnedEdit::hidden();
    let other = OwnedEdit::hidden();
    let target = edit.bind(101);
    assert_eq!(target.root, edit.0 .0 as usize);
    let world = FixtureWorld::new(target, false);
    let snapshot = world.snapshot(Some(target));
    assert!(snapshot.target_alive);
    assert!(!snapshot.target_iconic);
    for invalid in [
        CapturedTarget {
            pid: target.pid.wrapping_add(1),
            ..target
        },
        CapturedTarget {
            tid: target.tid.wrapping_add(1),
            ..target
        },
        CapturedTarget {
            root: other.0 .0 as usize,
            ..target
        },
        CapturedTarget {
            generation: target.generation + 1,
            ..target
        },
    ] {
        assert!(!world.snapshot(Some(invalid)).target_alive, "{invalid:?}");
    }
    native::release_session_target(target);
    assert!(!world.snapshot(Some(target)).target_alive);
    let rebound = edit.bind(target.generation + 1);
    assert!(!world.snapshot(Some(target)).target_alive);
    assert!(world.snapshot(Some(rebound)).target_alive);
    native::release_session_target(target);
    assert!(
        world.snapshot(Some(rebound)).target_alive,
        "releasing an old marker must preserve the new lifetime"
    );
    native::release_session_target(rebound);
    drop(edit);
    assert!(!world.snapshot(Some(rebound)).target_alive);
}

#[test]
fn root_lifetime_survives_owned_edit_child_recreation() {
    // The root stays hidden: no foreground/focus changes or keyboard delivery.
    let root = OwnedRoot::hidden();
    let first = OwnedEdit::child(root.0);
    let target = root.bind(105);
    assert_eq!(target.hwnd, root.0 .0 as usize);
    assert_eq!(target.root, root.0 .0 as usize);
    assert_ne!(target.hwnd, first.0 .0 as usize);
    let world = FixtureWorld::new(target, false);
    assert!(world.snapshot(Some(target)).target_alive);
    drop(first);
    assert!(
        world.snapshot(Some(target)).target_alive,
        "destroying the old focused child must not invalidate the captured root"
    );
    let replacement = OwnedEdit::child(root.0);
    assert_eq!(unsafe { GetAncestor(replacement.0, GA_ROOT) }, root.0);
    assert!(
        world.snapshot(Some(target)).target_alive,
        "replacement EDIT must preserve the root identity and lifetime marker"
    );
    native::release_session_target(target);
    assert!(!world.snapshot(Some(target)).target_alive);
    drop(replacement);
    drop(root);
}

#[test]
fn stop_target_adoption_preserves_same_window_marker_and_releases_previous_window() {
    let first = OwnedEdit::hidden();
    let second = OwnedEdit::hidden();
    let start = first.bind(201);
    let mut selected = Some(start);
    let same = first.bind(201);
    adopt_stop_target(&mut selected, Some(same));
    let world = FixtureWorld::new(same, false);
    assert!(
        world.snapshot(selected).target_alive,
        "same-window stop must retain the rebound marker"
    );
    // A reused HWND can have new identity fields while aliasing the same property.
    selected = Some(CapturedTarget {
        pid: same.pid.wrapping_add(1),
        ..same
    });
    adopt_stop_target(&mut selected, Some(first.bind(201)));
    assert!(world.snapshot(selected).target_alive);
    let stopped = second.bind(201);
    adopt_stop_target(&mut selected, Some(stopped));
    assert!(!world.snapshot(Some(start)).target_alive);
    assert!(world.snapshot(selected).target_alive);
    adopt_stop_target(&mut selected, None);
    assert_eq!(selected, Some(stopped));
    assert!(world.snapshot(selected).target_alive);
    drop(second);
    let mut delivery = FixtureWorld::new(stopped, false);
    assert_eq!(
        insert(&mut delivery, "stopped window gone"),
        InsertOutcome::Copied {
            reason: CopyReason::TargetGone
        }
    );
    assert_eq!(delivery.copied.as_deref(), Some("stopped window gone"));
}

#[test]
fn destroyed_owned_edit_copies_target_gone_without_keyboard_or_real_clipboard() {
    let edit = OwnedEdit::hidden();
    let target = edit.bind(103);
    let mut world = FixtureWorld::new(target, false);
    drop(edit);
    let text = "Owned fixture transcript";
    assert_eq!(
        insert(&mut world, text),
        InsertOutcome::Copied {
            reason: CopyReason::TargetGone
        }
    );
    assert_eq!(world.copied.as_deref(), Some(text));
}

fn pump_messages() {
    // Bound each drain even if another producer continuously posts messages.
    for _ in 0..128 {
        let mut message = MSG::default();
        unsafe {
            if !PeekMessageW(&mut message, None, 0, 0, PM_REMOVE).as_bool() {
                break;
            }
            let _ = TranslateMessage(&message);
            DispatchMessageW(&message);
        }
    }
}

struct RestoreForeground(HWND);

impl Drop for RestoreForeground {
    fn drop(&mut self) {
        unsafe {
            if IsWindow(self.0).as_bool() {
                let _ = SetForegroundWindow(self.0);
            }
        }
    }
}

#[test]
#[ignore = "interactive Windows desktop required; temporarily activates an owned EDIT window"]
fn native_unicode_delivery_to_owned_edit_is_exact_for_cyrillic_and_emoji() {
    let original_foreground = unsafe { GetForegroundWindow() };
    let start_root = OwnedRoot::hidden();
    let start_window = OwnedEdit::child(start_root.0);
    let root = OwnedRoot::hidden();
    let first = OwnedEdit::child(root.0);
    let _restore = RestoreForeground(original_foreground);
    let timeout = if std::env::var("VOXELY_NATIVE_FOCUS_WAIT").as_deref() == Ok("1") {
        Duration::from_secs(45)
    } else {
        Duration::from_secs(2)
    };
    unsafe {
        SetWindowTextW(start_root.0, w!("Voxely native recording start fixture")).unwrap();
        let _ = ShowWindow(start_root.0, SW_SHOW);
        let _ = SetForegroundWindow(start_root.0);
    }
    let deadline = Instant::now() + timeout;
    while unsafe { GetForegroundWindow() } != start_root.0 && Instant::now() < deadline {
        pump_messages();
        std::thread::sleep(Duration::from_millis(10));
    }
    assert_eq!(unsafe { GetForegroundWindow() }, start_root.0);
    let _ = unsafe { SetFocus(start_window.0) };
    let start_target =
        native::capture_session_target(&[], 104).expect("capture recording start window");
    assert_eq!(start_target.root, start_root.0 .0 as usize);
    unsafe {
        let _ = ShowWindow(root.0, SW_SHOW);
        let _ = SetForegroundWindow(root.0);
    }
    let deadline = Instant::now() + timeout;
    while unsafe { GetForegroundWindow() } != root.0 && Instant::now() < deadline {
        pump_messages();
        std::thread::sleep(Duration::from_millis(10));
    }
    assert_eq!(
        unsafe { GetForegroundWindow() },
        root.0,
        "owned EDIT activation denied; native Unicode delivery was not verified"
    );
    // SetFocus returns the previous focus, which may legitimately be NULL.
    let _ = unsafe { SetFocus(first.0) };
    assert_eq!(
        unsafe { GetFocus() },
        first.0,
        "first EDIT must really have focus"
    );
    let stopped = native::capture_session_target(&[], 104).expect("capture explicit stop window");
    let mut selected = Some(start_target);
    adopt_stop_target(&mut selected, Some(stopped));
    let target = selected.expect("explicit stop must select the destination");
    assert!(
        !FixtureWorld::new(start_target, false)
            .snapshot(Some(start_target))
            .target_alive
    );
    assert_eq!(target.hwnd, root.0 .0 as usize);
    assert_eq!(target.root, root.0 .0 as usize);
    assert_eq!(target.pid, std::process::id());
    assert_eq!(target.tid, unsafe { GetCurrentThreadId() });
    drop(first);
    let edit = OwnedEdit::child(root.0);
    let _ = unsafe { SetFocus(edit.0) };
    assert_eq!(
        unsafe { GetFocus() },
        edit.0,
        "replacement EDIT must really have focus"
    );
    let mut world = FixtureWorld::new(target, true);
    world.expected_focus = edit.0 .0 as usize;
    assert!(world.snapshot(Some(target)).target_alive);
    let snapshot = world.snapshot(Some(target));
    assert_eq!(snapshot.focus_root, Some(target.root));
    assert_eq!(snapshot.focus_hwnd, Some(edit.0 .0 as usize));
    assert!(snapshot
        .window_class
        .as_deref()
        .is_some_and(|class| class.eq_ignore_ascii_case("EDIT")));
    let expected = "Unicode: Привет, ёжик! 👍\r\nВторая строка 🚀";
    let expected_units = expected.encode_utf16().collect::<Vec<_>>();
    let outcome = insert(&mut world, expected);
    assert_eq!(outcome, InsertOutcome::Inserted);
    assert_eq!(
        world.copied, None,
        "native delivery must never fall back to copy"
    );
    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        pump_messages();
        // WM_CHAR delivers surrogate halves separately. Intermediate text may
        // end with a high surrogate while the rest is still queued.
        if edit.text_units() == expected_units || Instant::now() >= deadline {
            break;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    assert_eq!(
        edit.text_units(),
        expected_units,
        "actual EDIT UTF-16 must match exactly"
    );
    assert_eq!(
        start_window.text(),
        "",
        "recording start window must not receive the text"
    );
    native::release_session_target(target);
}
