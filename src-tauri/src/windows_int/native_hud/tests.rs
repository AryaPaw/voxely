use super::*;

#[test]
fn native_hud_uses_existing_theme_and_translation_sources() {
    for dark in [false, true] {
        for name in ["bg", "fg", "border", "armed", "armed-fg"] {
            assert!(render::token(name, dark).is_some(), "{name}");
        }
    }
    assert_ne!(
        render::token("bg", false).unwrap().0,
        render::token("bg", true).unwrap().0
    );
    for locale in ["ru", "en"] {
        for key in [
            "overlayRecording",
            "overlaySaving",
            "overlayProcessing",
            "overlayTranscribing",
            "overlayRetry",
            "overlayError",
            "overlayCancel",
            "overlayRecordingLimit",
        ] {
            assert!(render::copy(key, locale).is_some(), "{key}/{locale}");
        }
    }
    assert_eq!(render::copy("overlayRecording", "ru"), Some("Запись"));
    assert!(!render::pill_contains(0.0, 0.0));
    assert!(!render::pill_contains(12.0, 8.0));
    assert!(render::pill_contains(160.0, 34.0));
}

fn frame(visible: bool) -> Frame {
    Frame {
        snapshot: crate::app::overlay_controller::overlay_snapshot(
            1,
            visible,
            SessionState::Recording,
            false,
        ),
        epoch: 1,
        generation: 1,
        theme: "dark".into(),
        accent_color: Default::default(),
        locale: "en".into(),
        target: 0,
        levels: vec![0.2; 28],
    }
}

#[test]
fn native_hud_limit_label_does_not_hide_transcription_or_errors() {
    let mut frame = frame(true);
    frame.snapshot.limit_reached = true;
    assert_eq!(render::label(&frame, false), "Recording limit reached");
    frame.snapshot.state = SessionState::ProcessingAudio;
    assert_eq!(render::label(&frame, false), "Processing");
    frame.snapshot.state = SessionState::Transcribing { attempt: 2 };
    assert_eq!(render::label(&frame, false), "Transcribing (try 2)");
    frame.snapshot.state = SessionState::Failed {
        message: "private details".into(),
        code: "test".into(),
    };
    assert_eq!(render::label(&frame, false), "Error");
    assert_eq!(render::label(&frame, true), "Cancel recording");
}

#[test]
#[ignore = "requires a visible Windows desktop; isolated native windows only, no microphone or user data"]
fn native_rust_hud_visibility_alpha_dpi_and_lifetime() {
    use parking_lot::Mutex;
    let visible = Arc::new(AtomicBool::new(false));
    let theme = Arc::new(Mutex::new("dark".to_string()));
    let v = visible.clone();
    let t = theme.clone();
    let before = unsafe { GetForegroundWindow() };
    let calls = Arc::new(std::sync::atomic::AtomicU64::new(0));
    let cancelled = calls.clone();
    let hud = NativeHud::spawn(
        move || {
            let mut f = frame(v.load(Ordering::SeqCst));
            f.theme = t.lock().clone();
            Some(f)
        },
        move |generation, epoch| {
            assert_eq!((generation, epoch), (1, 1));
            cancelled.fetch_add(1, Ordering::SeqCst);
        },
    )
    .unwrap();
    let hwnd = HWND(hud.hwnd() as *mut _);
    assert!(!hwnd.0.is_null());
    assert!(!unsafe { IsWindowVisible(hwnd) }.as_bool());
    for cycle in 0..4 {
        *theme.lock() = if cycle % 2 == 0 { "light" } else { "dark" }.into();
        visible.store(true, Ordering::SeqCst);
        std::thread::sleep(Duration::from_millis(200));
        unsafe {
            assert!(IsWindowVisible(hwnd).as_bool());
            assert_eq!(GetForegroundWindow(), before, "HUD must not steal focus");
            let style = GetWindowLongW(hwnd, GWL_EXSTYLE) as u32;
            assert_eq!(
                style & (WS_EX_LAYERED.0 | WS_EX_NOACTIVATE.0 | WS_EX_TOOLWINDOW.0),
                WS_EX_LAYERED.0 | WS_EX_NOACTIVATE.0 | WS_EX_TOOLWINDOW.0
            );
            assert_eq!(style & WS_EX_APPWINDOW.0, 0);
            let mut rect = RECT::default();
            GetWindowRect(hwnd, &mut rect).unwrap();
            let corner = POINT {
                x: rect.left + 1,
                y: rect.top + 1,
            };
            assert_ne!(
                WindowFromPoint(corner),
                hwnd,
                "transparent corners must pass through"
            );
            assert_eq!(
                WindowFromPoint(POINT {
                    x: (rect.left + rect.right) / 2,
                    y: (rect.top + rect.bottom) / 2
                }),
                hwnd
            );
            if cycle == 0 {
                let mut previous = POINT::default();
                GetCursorPos(&mut previous).unwrap();
                let center = POINT {
                    x: (rect.left + rect.right) / 2,
                    y: (rect.top + rect.bottom) / 2,
                };
                struct RestorePointer(POINT, POINT);
                impl Drop for RestorePointer {
                    fn drop(&mut self) {
                        unsafe {
                            let mut now = POINT::default();
                            if GetCursorPos(&mut now).is_ok() && now == self.1 {
                                let _ = SetCursorPos(self.0.x, self.0.y);
                            }
                        }
                    }
                }
                let _restore = RestorePointer(previous, center);
                SetCursorPos(center.x, center.y).unwrap();
                std::thread::sleep(Duration::from_millis(100));
                SendMessageW(hwnd, WM_LBUTTONDOWN, WPARAM(0), LPARAM(0));
                SendMessageW(hwnd, WM_LBUTTONUP, WPARAM(0), LPARAM(0));
                assert_eq!(
                    calls.load(Ordering::SeqCst),
                    1,
                    "hovered native pill must dispatch cancellation once"
                );
                assert_eq!(
                    GetForegroundWindow(),
                    before,
                    "click must not activate the HUD"
                );
            }
        }
        // Display changes must not falsify the physical visibility bookkeeping.
        unsafe {
            SendMessageW(hwnd, WM_DISPLAYCHANGE, WPARAM(0), LPARAM(0));
        }
        visible.store(false, Ordering::SeqCst);
        std::thread::sleep(Duration::from_millis(150));
        assert!(!unsafe { IsWindowVisible(hwnd) }.as_bool());
    }
    // Exercise actual per-pixel painting at multiple DPI scales independently of monitor setup.
    for scale in [1.0, 1.25, 1.5, 2.0] {
        unsafe {
            let mut painter = render::Painter::new(scale).unwrap();
            painter
                .paint(
                    hwnd,
                    &frame(true),
                    false,
                    Duration::from_secs(65),
                    POINT { x: 80, y: 80 },
                )
                .unwrap();
            let mut rect = RECT::default();
            GetWindowRect(hwnd, &mut rect).unwrap();
            assert_eq!(rect.right - rect.left, (320.0 * scale).round() as i32);
            assert_eq!(rect.bottom - rect.top, (72.0 * scale).round() as i32);
        }
    }
    drop(hud);
    assert!(
        !unsafe { IsWindow(hwnd) }.as_bool(),
        "worker/window must end together"
    );
    let other = NativeHud::spawn(|| Some(frame(false)), |_, _| {}).unwrap();
    unsafe {
        SendMessageW(HWND(other.hwnd() as *mut _), WM_CLOSE, WPARAM(0), LPARAM(0));
    }
    assert_eq!(
        other.hwnd(),
        0,
        "a dead renderer must expose failure for fallback"
    );
    drop(other);
}
