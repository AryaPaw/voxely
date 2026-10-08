//! Observe only lifecycle messages for the main HWND. Never consume or alter them.

#[cfg(windows)]
mod native {
    use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
    use windows::Win32::System::Threading::GetCurrentThreadId;
    use windows::Win32::UI::Shell::{DefSubclassProc, RemoveWindowSubclass, SetWindowSubclass};
    use windows::Win32::UI::WindowsAndMessaging::{
        GetForegroundWindow, GetWindowThreadProcessId, InSendMessageEx, SC_CLOSE, WM_CLOSE,
        WM_DESTROY, WM_ENDSESSION, WM_NCDESTROY, WM_QUERYENDSESSION, WM_SYSCOMMAND,
    };

    use crate::runtime_diagnostics::{self, Event, NativeWindowMessage};

    const SUBCLASS_ID: usize = 0x56584C44;

    pub(super) fn classify(message: u32, wparam: usize) -> Option<NativeWindowMessage> {
        match message {
            WM_SYSCOMMAND if wparam & 0xFFF0 == SC_CLOSE as usize => {
                Some(NativeWindowMessage::SystemClose)
            }
            WM_CLOSE => Some(NativeWindowMessage::Close),
            WM_QUERYENDSESSION => Some(NativeWindowMessage::QueryEndSession),
            WM_ENDSESSION => Some(NativeWindowMessage::EndSession),
            WM_DESTROY => Some(NativeWindowMessage::Destroy),
            WM_NCDESTROY => Some(NativeWindowMessage::NonClientDestroy),
            _ => None,
        }
    }

    pub(super) unsafe fn install(hwnd: HWND) -> bool {
        // SetWindowSubclass cannot subclass a window owned by another thread.
        GetWindowThreadProcessId(hwnd, None) == GetCurrentThreadId()
            && SetWindowSubclass(hwnd, Some(observe), SUBCLASS_ID, 0).as_bool()
    }

    unsafe extern "system" fn observe(
        hwnd: HWND,
        message: u32,
        wparam: WPARAM,
        lparam: LPARAM,
        subclass_id: usize,
        _reference: usize,
    ) -> LRESULT {
        // Diagnostics are best effort. Even a logging panic must not escape the ABI or
        // prevent the original window procedure from receiving the message.
        let _ = std::panic::catch_unwind(|| {
            if let Some(kind) = classify(message, wparam.0) {
                runtime_diagnostics::try_record(Event::NativeWindowMessage {
                    message: kind,
                    // These flags indicate cross-thread delivery, not a sender PID.
                    send_flags: InSendMessageEx(None),
                    foreground: GetForegroundWindow() == hwnd,
                    session_ending: message == WM_ENDSESSION && wparam.0 != 0,
                });
            }
        });
        if message == WM_NCDESTROY {
            let _ = RemoveWindowSubclass(hwnd, Some(observe), subclass_id);
        }
        DefSubclassProc(hwnd, message, wparam, lparam)
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use windows::core::w;
        use windows::Win32::UI::Shell::GetWindowSubclass;
        use windows::Win32::UI::WindowsAndMessaging::{
            CreateWindowExW, DefWindowProcW, DestroyWindow, RegisterClassW, SendMessageW,
            UnregisterClassW, WINDOW_EX_STYLE, WINDOW_STYLE, WM_APP, WNDCLASSW,
        };

        #[test]
        fn classification_is_limited_to_lifecycle_messages() {
            assert_eq!(
                classify(WM_SYSCOMMAND, SC_CLOSE as usize | 3),
                Some(NativeWindowMessage::SystemClose)
            );
            assert_eq!(classify(WM_CLOSE, 0), Some(NativeWindowMessage::Close));
            assert_eq!(
                classify(WM_ENDSESSION, 1),
                Some(NativeWindowMessage::EndSession)
            );
            assert_eq!(classify(WM_APP, 0), None);
            assert_eq!(classify(WM_SYSCOMMAND, 0xF020), None);
        }

        unsafe extern "system" fn test_window(
            hwnd: HWND,
            msg: u32,
            wp: WPARAM,
            lp: LPARAM,
        ) -> LRESULT {
            if msg == WM_CLOSE || msg == WM_APP {
                // A distinctive return value proves forwarding through the subclass.
                return LRESULT(73);
            }
            DefWindowProcW(hwnd, msg, wp, lp)
        }

        #[test]
        fn real_hwnd_forwards_messages_and_survives_window_destruction() {
            unsafe {
                let class = w!("VoxelyDiagnosticsIsolatedTest");
                let wc = WNDCLASSW {
                    lpfnWndProc: Some(test_window),
                    lpszClassName: class,
                    ..Default::default()
                };
                assert_ne!(RegisterClassW(&wc), 0);
                let hwnd = CreateWindowExW(
                    WINDOW_EX_STYLE::default(),
                    class,
                    w!(""),
                    WINDOW_STYLE::default(),
                    0,
                    0,
                    0,
                    0,
                    None,
                    None,
                    None,
                    None,
                )
                .unwrap();
                assert!(install(hwnd));
                assert_eq!(
                    SendMessageW(hwnd, WM_CLOSE, WPARAM(0), LPARAM(0)),
                    LRESULT(73)
                );
                assert_eq!(
                    SendMessageW(hwnd, WM_APP, WPARAM(0), LPARAM(0)),
                    LRESULT(73)
                );
                let mut reference = 1;
                assert!(
                    GetWindowSubclass(hwnd, Some(observe), SUBCLASS_ID, Some(&mut reference))
                        .as_bool()
                );
                assert_eq!(reference, 0);
                DestroyWindow(hwnd).unwrap();
                assert!(
                    !GetWindowSubclass(hwnd, Some(observe), SUBCLASS_ID, Some(&mut reference))
                        .as_bool()
                );
                UnregisterClassW(class, None).unwrap();
            }
        }
    }
}

pub(crate) fn install(window: &tauri::WebviewWindow) {
    #[cfg(windows)]
    let succeeded = window
        .hwnd()
        .is_ok_and(|hwnd| unsafe { native::install(windows::Win32::Foundation::HWND(hwnd.0)) });
    #[cfg(not(windows))]
    let succeeded = {
        let _ = window;
        false
    };
    crate::runtime_diagnostics::record(
        crate::runtime_diagnostics::Event::NativeWindowHookInstalled { succeeded },
    );
}
