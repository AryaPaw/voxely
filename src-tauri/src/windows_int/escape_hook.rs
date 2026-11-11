use std::sync::{
    atomic::{AtomicU64, Ordering},
    Arc, OnceLock,
};

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
use crate::error::AppError;

static APP: OnceLock<AppHandle> = OnceLock::new();
struct Hook {
    raw: isize,
    epoch: u64,
}
static HOOK: Mutex<Option<Hook>> = Mutex::new(None);
static HOOK_EPOCH: AtomicU64 = AtomicU64::new(u64::MAX);

fn hook_is_active(hook: Option<&Hook>, epoch: u64, active_epoch: u64) -> bool {
    active_epoch == epoch && hook.is_some_and(|hook| hook.epoch == epoch)
}

pub fn install(app: &AppHandle) -> Result<(), AppError> {
    let _ = APP.set(app.clone());
    let Some(context) = app.try_state::<Arc<AppContext>>() else {
        return Err(AppError::HotkeyFailed(
            "Escape hook context isn't available".into(),
        ));
    };
    let epoch = context.shortcut_capture_epoch.load(Ordering::SeqCst);
    let mut guard = HOOK.lock();
    if hook_is_active(guard.as_ref(), epoch, HOOK_EPOCH.load(Ordering::SeqCst)) {
        return Ok(());
    }
    unsafe {
        // Capture restore replaces the hook even when the chord stayed the same.
        // Disable callbacks between the two lifetimes on their one owning pump.
        HOOK_EPOCH.store(u64::MAX, Ordering::SeqCst);
        if let Some(old) = guard.take() {
            if let Err(error) = UnhookWindowsHookEx(HHOOK(old.raw as *mut _)) {
                *guard = Some(old);
                return Err(AppError::HotkeyFailed(error.to_string()));
            }
        }
        let module = match GetModuleHandleW(None) {
            Ok(handle) => handle,
            Err(error) => {
                tracing::error!(error = %error, "GetModuleHandleW failed for Escape LL hook");
                return Err(AppError::HotkeyFailed(error.to_string()));
            }
        };
        match SetWindowsHookExW(WH_KEYBOARD_LL, Some(ll_proc), module, 0) {
            Ok(hook) => {
                *guard = Some(Hook {
                    raw: hook.0 as isize,
                    epoch,
                });
                HOOK_EPOCH.store(epoch, Ordering::SeqCst);
                tracing::info!("Escape busy-only WH_KEYBOARD_LL installed");
                Ok(())
            }
            Err(error) => {
                tracing::error!(error = %error, "WH_KEYBOARD_LL install failed");
                Err(AppError::HotkeyFailed(error.to_string()))
            }
        }
    }
}

pub fn uninstall() -> Result<(), AppError> {
    let mut guard = HOOK.lock();
    HOOK_EPOCH.store(u64::MAX, Ordering::SeqCst);
    let Some(hook) = guard.take() else {
        return Ok(());
    };
    unsafe {
        if let Err(error) = UnhookWindowsHookEx(HHOOK(hook.raw as *mut _)) {
            *guard = Some(hook);
            return Err(AppError::HotkeyFailed(error.to_string()));
        }
    }
    tracing::info!("Escape LL hook uninstalled");
    Ok(())
}

fn session_command_epoch(app: &AppHandle) -> Option<u64> {
    let source = HOOK_EPOCH.load(Ordering::SeqCst);
    app.try_state::<Arc<AppContext>>()
        .and_then(|ctx| hook_command_epoch(&ctx, source))
}

fn hook_should_consume_escape(ctx: &AppContext) -> bool {
    hook_command_epoch(ctx, ctx.shortcut_capture_epoch.load(Ordering::SeqCst)).is_some()
}

fn hook_command_epoch(ctx: &AppContext, source: u64) -> Option<u64> {
    let suspended = ctx.hotkeys_suspended.lock();
    (!*suspended
        && source == ctx.shortcut_capture_epoch.load(Ordering::SeqCst)
        && escape_cancels(ctx))
    .then(|| {
        ctx.shortcut_capture_epoch
            .load(std::sync::atomic::Ordering::SeqCst)
    })
}

unsafe extern "system" fn ll_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code >= 0 {
        let msg = wparam.0 as u32;
        if msg == WM_KEYDOWN || msg == WM_SYSKEYDOWN {
            let info = unsafe { &*(lparam.0 as *const KBDLLHOOKSTRUCT) };
            if info.vkCode == u32::from(VK_ESCAPE.0) {
                if let Some(app) = APP.get() {
                    if let Some(epoch) = session_command_epoch(app) {
                        crate::app::shortcuts::post_session_command_for_epoch(
                            app,
                            crate::app::shortcuts::SessionCommand::Cancel,
                            epoch,
                        );
                        return LRESULT(1);
                    }
                }
            }
        }
    }
    unsafe { CallNextHookEx(None, code, wparam, lparam) }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn failed_hook_cleanup_cannot_make_same_epoch_retry_report_an_inactive_hook_as_ready() {
        let retained = Hook { raw: 7, epoch: 3 };
        assert!(hook_is_active(Some(&retained), 3, 3));
        // Failed UnhookWindowsHookEx retains the handle, but deactivates callbacks.
        assert!(!hook_is_active(Some(&retained), 3, u64::MAX));
        assert!(!hook_is_active(Some(&retained), 4, 3));
        assert!(!hook_is_active(None, 3, 3));
    }

    #[test]
    fn escape_callbacks_from_a_previous_hook_epoch_stay_inert_after_capture_resume() {
        let directory = tempfile::tempdir().unwrap();
        let ctx = AppContext::initialize(directory.path().to_path_buf()).unwrap();
        *ctx.state.lock() = crate::app::machine::SessionState::Recording;
        let previous = ctx.shortcut_capture_epoch.load(Ordering::SeqCst);
        assert_eq!(hook_command_epoch(&ctx, previous), Some(previous));
        crate::app::shortcuts::set_capture_intent(&ctx, true);
        crate::app::shortcuts::set_capture_intent(&ctx, false);
        assert_eq!(hook_command_epoch(&ctx, previous), None);
        let current = ctx.shortcut_capture_epoch.load(Ordering::SeqCst);
        assert_eq!(hook_command_epoch(&ctx, current), Some(current));
        assert_eq!(
            *ctx.state.lock(),
            crate::app::machine::SessionState::Recording
        );
    }
    #[test]
    fn hotkey_capture_passes_escape_to_the_editor_without_cancelling_live_work() {
        let directory = tempfile::tempdir().unwrap();
        let ctx = AppContext::initialize(directory.path().to_path_buf()).unwrap();
        *ctx.state.lock() = crate::app::machine::SessionState::Recording;
        assert!(hook_should_consume_escape(&ctx));
        crate::app::shortcuts::set_capture_intent(&ctx, true);
        assert!(!hook_should_consume_escape(&ctx));
        ctx.insert_in_flight
            .store(true, std::sync::atomic::Ordering::SeqCst);
        assert!(!hook_should_consume_escape(&ctx));
        assert_eq!(
            *ctx.state.lock(),
            crate::app::machine::SessionState::Recording
        );
        crate::app::shortcuts::set_capture_intent(&ctx, false);
        assert!(hook_should_consume_escape(&ctx));
    }
}
