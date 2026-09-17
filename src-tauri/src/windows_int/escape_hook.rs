use std::sync::{Arc, OnceLock};

use parking_lot::Mutex;
use tauri::AppHandle;
use tauri::Manager;
use windows::Win32::Foundation::{LPARAM, LRESULT, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Input::KeyboardAndMouse::VK_ESCAPE;
use windows::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, SetWindowsHookExW, UnhookWindowsHookEx, HHOOK, KBDLLHOOKSTRUCT, WH_KEYBOARD_LL,
    WM_KEYDOWN, WM_SYSKEYDOWN,
};

use crate::app::operations::escape_cancels;
use crate::app::session::AppContext;

static APP: OnceLock<AppHandle> = OnceLock::new();
static HOOK: Mutex<Option<isize>> = Mutex::new(None);

pub fn install(app: &AppHandle) {
    let _ = APP.set(app.clone());
    let mut guard = HOOK.lock();
    if guard.is_some() {
        return;
    }
    unsafe {
        let module = match GetModuleHandleW(None) {
            Ok(handle) => handle,
            Err(error) => {
                tracing::error!(error = %error, "GetModuleHandleW failed for Escape LL hook");
                return;
            }
        };
        match SetWindowsHookExW(WH_KEYBOARD_LL, Some(ll_proc), module, 0) {
            Ok(hook) => {
                *guard = Some(hook.0 as isize);
                tracing::info!("Escape busy-only WH_KEYBOARD_LL installed");
            }
            Err(error) => {
                tracing::error!(error = %error, "WH_KEYBOARD_LL install failed");
            }
        }
    }
}

pub fn uninstall() {
    let mut guard = HOOK.lock();
    let Some(raw) = guard.take() else {
        return;
    };
    unsafe {
        let _ = UnhookWindowsHookEx(HHOOK(raw as *mut _));
    }
    tracing::info!("Escape LL hook uninstalled");
}

fn session_is_cancellable(app: &AppHandle) -> bool {
    app.try_state::<Arc<AppContext>>()
        .map(|ctx| escape_cancels(&ctx))
        .unwrap_or(false)
}

unsafe extern "system" fn ll_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code >= 0 {
        let msg = wparam.0 as u32;
        if msg == WM_KEYDOWN || msg == WM_SYSKEYDOWN {
            let info = unsafe { &*(lparam.0 as *const KBDLLHOOKSTRUCT) };
            if info.vkCode == u32::from(VK_ESCAPE.0) {
                if let Some(app) = APP.get() {
                    if session_is_cancellable(app) {
                        crate::app::shortcuts::post_session_command(
                            app,
                            crate::app::shortcuts::SessionCommand::Cancel,
                        );
                        return LRESULT(1);
                    }
                }
            }
        }
    }
    unsafe { CallNextHookEx(None, code, wparam, lparam) }
}
