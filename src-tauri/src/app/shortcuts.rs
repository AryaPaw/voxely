use std::sync::{atomic::Ordering, Arc};
use std::thread;
use std::time::Duration;

use tauri::AppHandle;
use tauri::Manager;
use tauri_plugin_global_shortcut::{GlobalShortcutExt, Shortcut, ShortcutState};

use crate::app::session::{cancel_recording, toggle_recording, AppContext};
use crate::error::AppError;
use crate::windows_int::escape_hook;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionCommand {
    Toggle,
    Cancel,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct QueuedSessionCommand {
    pub command: SessionCommand,
    pub epoch: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShortcutLayout {
    pub hotkey: String,
    pub suspended: bool,
    pub capture_epoch: u64,
}

pub fn shortcut_layout_changed(applied: Option<&ShortcutLayout>, desired: &ShortcutLayout) -> bool {
    applied != Some(desired)
}

pub fn coalesce_session_command(
    pending: Option<SessionCommand>,
    next: SessionCommand,
) -> SessionCommand {
    match (pending, next) {
        (_, SessionCommand::Cancel) => SessionCommand::Cancel,
        (Some(SessionCommand::Cancel), SessionCommand::Toggle) => SessionCommand::Cancel,
        (_, command) => command,
    }
}

pub fn parse_hotkey(spec: &str) -> Result<Shortcut, AppError> {
    spec.parse::<Shortcut>()
        .map_err(|e| AppError::HotkeyFailed(e.to_string()))
}

pub fn toggle_dictation(app: &AppHandle) -> Result<(), AppError> {
    toggle_recording(app)
}

pub fn cancel_dictation(app: &AppHandle) -> Result<(), AppError> {
    cancel_recording(app)
}

pub fn wants_escape_hotkey(cancellable: bool, suspended: bool) -> bool {
    cancellable && !suspended
}

pub fn should_apply_scheduled_sync(ticket: u64, latest: u64) -> bool {
    ticket == latest
}

pub fn escape_register_fallback_to_ll_hook(register_ok: bool) -> bool {
    !register_ok
}

pub fn post_session_command(app: &AppHandle, command: SessionCommand) {
    let Some(ctx) = app.try_state::<Arc<AppContext>>() else {
        return;
    };
    let epoch = ctx.shortcut_capture_epoch.load(Ordering::SeqCst);
    post_session_command_for_epoch(app, command, epoch);
}

pub(crate) fn post_session_command_for_epoch(app: &AppHandle, command: SessionCommand, epoch: u64) {
    let Some(ctx) = app.try_state::<Arc<AppContext>>() else {
        return;
    };
    if *ctx.hotkeys_suspended.lock() {
        return;
    }
    if command == SessionCommand::Cancel && !crate::app::operations::escape_cancels(&ctx) {
        return;
    }
    if !enqueue_session_command_for_epoch(&ctx, command, epoch) {
        return;
    }
    let app = app.clone();
    thread::spawn(move || {
        thread::sleep(Duration::from_millis(1));
        let handle = app.clone();
        let _ = app.run_on_main_thread(move || {
            let Some(ctx) = handle.try_state::<Arc<AppContext>>() else {
                return;
            };
            let Some(command) = take_session_command(&ctx) else {
                return;
            };
            match command {
                SessionCommand::Toggle => {
                    let _ = toggle_recording(&handle);
                }
                SessionCommand::Cancel => {
                    let _ = cancel_recording(&handle);
                }
            }
        });
    });
}

/// The UI publishes intent before accepting a captured chord. Native unregister
/// may finish later, but callbacks from the old registration already become inert.
pub fn set_capture_intent(ctx: &AppContext, suspended: bool) {
    let mut intent = ctx.hotkeys_suspended.lock();
    if suspended && !*intent {
        ctx.shortcut_capture_epoch.fetch_add(1, Ordering::SeqCst);
    }
    *intent = suspended;
    if suspended {
        ctx.pending_session_command.lock().take();
    }
}

fn enqueue_session_command(ctx: &AppContext, command: SessionCommand) -> bool {
    enqueue_session_command_for_epoch(
        ctx,
        command,
        ctx.shortcut_capture_epoch.load(Ordering::SeqCst),
    )
}

fn enqueue_session_command_for_epoch(
    ctx: &AppContext,
    command: SessionCommand,
    epoch: u64,
) -> bool {
    let suspended = ctx.hotkeys_suspended.lock();
    if *suspended || ctx.shortcut_capture_epoch.load(Ordering::SeqCst) != epoch {
        return false;
    }
    let mut pending = ctx.pending_session_command.lock();
    let previous = pending
        .as_ref()
        .filter(|pending| pending.epoch == epoch)
        .map(|pending| pending.command);
    *pending = Some(QueuedSessionCommand {
        command: coalesce_session_command(previous, command),
        epoch,
    });
    true
}

fn take_session_command(ctx: &AppContext) -> Option<SessionCommand> {
    let suspended = ctx.hotkeys_suspended.lock();
    let pending = ctx.pending_session_command.lock().take();
    if *suspended {
        None
    } else {
        pending
            .filter(|pending| pending.epoch == ctx.shortcut_capture_epoch.load(Ordering::SeqCst))
            .map(|pending| pending.command)
    }
}

fn apply_shortcut_layout(
    applied: &mut Option<ShortcutLayout>,
    desired: ShortcutLayout,
    synchronize: impl FnOnce(&ShortcutLayout) -> Result<(), AppError>,
) -> Result<(), AppError> {
    if !shortcut_layout_changed(applied.as_ref(), &desired) {
        return Ok(());
    }
    // Native registration can fail after removing the old shortcut. Never let
    // a cached previous layout suppress registration during settings rollback.
    *applied = None;
    synchronize(&desired)?;
    *applied = Some(desired);
    Ok(())
}

pub fn sync_shortcuts(app: &AppHandle) -> Result<(), AppError> {
    let owner = app.clone();
    registration_on_owner(
        |work| {
            app.run_on_main_thread(work)
                .map_err(|error| AppError::HotkeyFailed(error.to_string()))
        },
        move || sync_shortcuts_on_owner(&owner),
    )
}

fn registration_on_owner(
    dispatch: impl FnOnce(Box<dyn FnOnce() + Send>) -> Result<(), AppError>,
    operation: impl FnOnce() -> Result<(), AppError> + Send + 'static,
) -> Result<(), AppError> {
    let (reply, receive) = std::sync::mpsc::sync_channel(1);
    // Acquire neither our applied-layout lock nor the plugin's shortcut store
    // before dispatch. WM_HOTKEY on the owning pump needs that same store.
    dispatch(Box::new(move || {
        let _ = reply.send(operation());
    }))?;
    receive.recv().map_err(|_| {
        AppError::HotkeyFailed("Shortcut owner stopped before acknowledging registration".into())
    })?
}

fn sync_shortcuts_on_owner(app: &AppHandle) -> Result<(), AppError> {
    let Some(ctx) = app.try_state::<Arc<AppContext>>() else {
        tracing::warn!("shortcut sync skipped: AppContext not managed");
        return Ok(());
    };
    // Read intent only after acquiring the registration lane: an older waiter
    // must not overwrite a newer registration with a pre-capture snapshot.
    let mut applied = ctx.applied_shortcut_layout.lock();
    let (suspended, capture_epoch) = {
        let suspended = ctx.hotkeys_suspended.lock();
        (
            *suspended,
            ctx.shortcut_capture_epoch.load(Ordering::SeqCst),
        )
    };
    let spec = ctx.settings.lock().hotkey.clone();
    let desired = ShortcutLayout {
        hotkey: spec.clone(),
        suspended,
        capture_epoch,
    };
    apply_shortcut_layout(&mut applied, desired, |layout| {
        let shortcut = if layout.suspended {
            None
        } else {
            Some(parse_hotkey(&layout.hotkey)?)
        };
        app.global_shortcut()
            .unregister_all()
            .map_err(|e| AppError::HotkeyFailed(e.to_string()))?;
        #[cfg(windows)]
        drain_owned_hotkey_messages()?;
        if let Some(shortcut) = shortcut {
            let epoch = layout.capture_epoch;
            app.global_shortcut()
                .on_shortcut(shortcut, move |app, _shortcut, event| {
                    if event.state == ShortcutState::Pressed {
                        post_session_command_for_epoch(app, SessionCommand::Toggle, epoch);
                    }
                })
                .map_err(|e| AppError::HotkeyFailed(e.to_string()))?;
        }
        // The entire registration transaction already runs on the Tao pump.
        // The low-level Escape hook must also stay on this message-loop thread.
        if hook_update_is_current(&ctx, layout) {
            if layout.suspended || ctx.shutdown_requested.load(Ordering::SeqCst) {
                #[cfg(windows)]
                escape_hook::uninstall()?;
                #[cfg(not(windows))]
                escape_hook::uninstall();
            } else {
                #[cfg(windows)]
                escape_hook::install(app)?;
                #[cfg(not(windows))]
                escape_hook::install(app);
            }
        }
        Ok(())
    })
}

fn hook_update_is_current(ctx: &AppContext, layout: &ShortcutLayout) -> bool {
    let suspended = ctx.hotkeys_suspended.lock();
    *suspended == layout.suspended
        && ctx.shortcut_capture_epoch.load(Ordering::SeqCst) == layout.capture_epoch
}

#[cfg(windows)]
fn drain_owned_hotkey_messages() -> Result<(), AppError> {
    use windows::Win32::{
        Foundation::{BOOL, HWND, LPARAM},
        System::Threading::GetCurrentThreadId,
        UI::WindowsAndMessaging::{EnumThreadWindows, GetClassNameW},
    };
    unsafe extern "system" fn find(hwnd: HWND, data: LPARAM) -> BOOL {
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let mut class = [0u16; 128];
            let len = GetClassNameW(hwnd, &mut class);
            // global-hotkey 0.8 owns this hidden top-level HWND on Tao's pump.
            if len > 0 && String::from_utf16_lossy(&class[..len as usize]) == "global_hotkey_app" {
                (*(data.0 as *mut Vec<HWND>)).push(hwnd);
            }
        }));
        result.is_ok().into()
    }
    let mut windows = Vec::<HWND>::new();
    if !unsafe {
        EnumThreadWindows(
            GetCurrentThreadId(),
            Some(find),
            LPARAM(&mut windows as *mut _ as isize),
        )
        .as_bool()
    } || windows.is_empty()
    {
        return Err(AppError::HotkeyFailed(
            "The shortcut owner's native queue couldn't be located".into(),
        ));
    }
    for hwnd in windows {
        drain_window_hotkey_messages(hwnd)?;
    }
    Ok(())
}

#[cfg(windows)]
fn drain_window_hotkey_messages(hwnd: windows::Win32::Foundation::HWND) -> Result<usize, AppError> {
    use windows::Win32::{
        System::Threading::{GetCurrentProcessId, GetCurrentThreadId},
        UI::WindowsAndMessaging::{
            GetWindowThreadProcessId, IsWindow, PeekMessageW, PostQuitMessage, MSG, PM_NOREMOVE,
            PM_REMOVE, WM_HOTKEY, WM_QUIT,
        },
    };
    unsafe {
        let mut process = 0;
        let owner = GetWindowThreadProcessId(hwnd, Some(&mut process));
        if !IsWindow(hwnd).as_bool()
            || owner != GetCurrentThreadId()
            || process != GetCurrentProcessId()
        {
            return Err(AppError::HotkeyFailed(
                "Shortcut messages can only be drained on their owning pump".into(),
            ));
        }
        let mut message = MSG::default();
        let mut drained = 0;
        while drained < 256
            && PeekMessageW(&mut message, hwnd, WM_HOTKEY, WM_HOTKEY, PM_REMOVE).as_bool()
        {
            // PeekMessage can surface WM_QUIT despite a message-range filter.
            // Preserve exit instead of consuming it as a stale shortcut.
            if message.message == WM_QUIT {
                PostQuitMessage(message.wParam.0 as i32);
                return Err(AppError::HotkeyFailed("Shortcut owner is exiting".into()));
            }
            drained += 1;
        }
        if PeekMessageW(&mut message, hwnd, WM_HOTKEY, WM_HOTKEY, PM_NOREMOVE).as_bool() {
            return Err(AppError::HotkeyFailed(
                "Too many pending shortcut messages; registration wasn't resumed".into(),
            ));
        }
        Ok(drained)
    }
}

pub fn set_hotkeys_suspended(app: &AppHandle, suspended: bool) -> Result<(), AppError> {
    let ctx = app.state::<Arc<AppContext>>();
    set_capture_intent(&ctx, suspended);
    sync_shortcuts(app)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registration_waiter_keeps_plugin_and_applied_locks_free_for_the_owner_pump() {
        type Work = Box<dyn FnOnce() + Send>;
        let plugin = Arc::new(parking_lot::Mutex::new(()));
        let applied = Arc::new(parking_lot::Mutex::new(()));
        let (send, receive) = std::sync::mpsc::channel::<Work>();
        let (release, wait) = std::sync::mpsc::sync_channel(1);
        let (queued, queue_ready) = std::sync::mpsc::sync_channel(1);
        let (observed, observation) = std::sync::mpsc::sync_channel(1);
        let hotkey_store = plugin.clone();
        send.send(Box::new(move || {
            let _store = hotkey_store.lock();
        }))
        .unwrap();
        let owner = thread::spawn(move || {
            wait.recv_timeout(Duration::from_secs(2)).unwrap();
            for _ in 0..2 {
                receive.recv_timeout(Duration::from_secs(2)).unwrap()();
            }
            thread::current().id()
        });
        let registration_store = plugin.clone();
        let registration_cache = applied.clone();
        let worker = thread::spawn(move || {
            registration_on_owner(
                move |work| {
                    send.send(work).unwrap();
                    queued.send(()).unwrap();
                    Ok(())
                },
                move || {
                    let _cache = registration_cache.lock();
                    let _store = registration_store.lock();
                    observed.send(thread::current().id()).unwrap();
                    Ok(())
                },
            )
        });
        queue_ready.recv_timeout(Duration::from_secs(2)).unwrap();
        let locks_free = plugin.try_lock().is_some() && applied.try_lock().is_some();
        release.send(()).unwrap();
        worker.join().unwrap().unwrap();
        let owner_id = owner.join().unwrap();
        assert!(
            locks_free,
            "Waiting for the Tao pump must not hold either registration lock"
        );
        assert_eq!(
            observation.recv().unwrap(),
            owner_id,
            "The native transaction must run on the pump, after the queued hotkey callback"
        );
    }

    #[cfg(windows)]
    #[test]
    fn native_same_id_re_registration_discards_only_old_owned_hotkey_messages() {
        use std::cell::Cell;
        use windows::{
            core::PCWSTR,
            Win32::{
                Foundation::{HWND, LPARAM, LRESULT, WPARAM},
                System::{LibraryLoader::GetModuleHandleW, Threading::GetCurrentProcessId},
                UI::WindowsAndMessaging::*,
            },
        };
        thread_local! {static DELIVERED:Cell<usize>=const {Cell::new(0)};}
        unsafe extern "system" fn procedure(
            hwnd: HWND,
            message: u32,
            wp: WPARAM,
            lp: LPARAM,
        ) -> LRESULT {
            if message == WM_HOTKEY {
                DELIVERED.with(|count| count.set(count.get() + 1));
                return LRESULT(0);
            }
            unsafe { DefWindowProcW(hwnd, message, wp, lp) }
        }
        struct Fixture {
            windows: Vec<HWND>,
            name: Vec<u16>,
            instance: windows::Win32::Foundation::HINSTANCE,
        }
        impl Drop for Fixture {
            fn drop(&mut self) {
                unsafe {
                    for hwnd in &self.windows {
                        let _ = DestroyWindow(*hwnd);
                    }
                    let _ = UnregisterClassW(PCWSTR(self.name.as_ptr()), self.instance);
                }
            }
        }
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        unsafe {
            let instance = GetModuleHandleW(None).unwrap().into();
            let name = format!(
                "VoxelyHotkeyQueueTest-{}-{}",
                GetCurrentProcessId(),
                NEXT.fetch_add(1, Ordering::SeqCst)
            )
            .encode_utf16()
            .chain(Some(0))
            .collect::<Vec<_>>();
            assert_ne!(
                RegisterClassW(&WNDCLASSW {
                    lpfnWndProc: Some(procedure),
                    hInstance: instance,
                    lpszClassName: PCWSTR(name.as_ptr()),
                    ..Default::default()
                }),
                0
            );
            let mut fixture = Fixture {
                windows: Vec::new(),
                name,
                instance,
            };
            for _ in 0..2 {
                fixture.windows.push(
                    CreateWindowExW(
                        WINDOW_EX_STYLE::default(),
                        PCWSTR(fixture.name.as_ptr()),
                        PCWSTR::null(),
                        WS_POPUP,
                        0,
                        0,
                        0,
                        0,
                        None,
                        None,
                        instance,
                        None,
                    )
                    .unwrap(),
                );
            }
            let target = fixture.windows[0];
            let unrelated = fixture.windows[1];
            PostMessageW(target, WM_HOTKEY, WPARAM(7), LPARAM(0)).unwrap();
            PostMessageW(unrelated, WM_HOTKEY, WPARAM(7), LPARAM(0)).unwrap();
            assert_eq!(drain_window_hotkey_messages(target).unwrap(), 1);
            let raw = target.0 as isize;
            assert!(thread::spawn(
                move || drain_window_hotkey_messages(HWND(raw as *mut _)).is_err()
            )
            .join()
            .unwrap());
            let mut message = MSG::default();
            assert!(
                !PeekMessageW(&mut message, target, WM_HOTKEY, WM_HOTKEY, PM_NOREMOVE).as_bool()
            );
            // The same HotKeyId now refers to a fresh handler. Only its new post is delivered.
            PostMessageW(target, WM_HOTKEY, WPARAM(7), LPARAM(0)).unwrap();
            assert!(PeekMessageW(&mut message, target, WM_HOTKEY, WM_HOTKEY, PM_REMOVE).as_bool());
            DispatchMessageW(&message);
            assert_eq!(DELIVERED.with(Cell::get), 1);
            assert!(
                PeekMessageW(&mut message, unrelated, WM_HOTKEY, WM_HOTKEY, PM_REMOVE).as_bool()
            );
            assert_eq!(
                message.wParam.0, 7,
                "A different HWND's queue must remain untouched"
            );
            for _ in 0..257 {
                PostMessageW(target, WM_HOTKEY, WPARAM(7), LPARAM(0)).unwrap();
            }
            assert!(
                drain_window_hotkey_messages(target).is_err(),
                "A saturated queue must leave registration suspended"
            );
            assert!(PeekMessageW(&mut message, target, WM_HOTKEY, WM_HOTKEY, PM_REMOVE).as_bool());
            assert!(
                !PeekMessageW(&mut message, target, WM_HOTKEY, WM_HOTKEY, PM_NOREMOVE).as_bool()
            );
        }
    }

    #[test]
    fn capture_epoch_rejects_delayed_native_callbacks_after_capture_has_finished() {
        let directory = tempfile::tempdir().unwrap();
        let ctx = Arc::new(AppContext::initialize(directory.path().to_path_buf()).unwrap());
        *ctx.state.lock() = crate::app::machine::SessionState::Recording;
        let old_epoch = ctx.shortcut_capture_epoch.load(Ordering::SeqCst);
        let (release, wait) = std::sync::mpsc::sync_channel(1);
        let worker_ctx = ctx.clone();
        let callback = thread::spawn(move || {
            wait.recv_timeout(Duration::from_secs(2)).unwrap();
            enqueue_session_command_for_epoch(&worker_ctx, SessionCommand::Toggle, old_epoch)
        });
        set_capture_intent(&ctx, true);
        set_capture_intent(&ctx, false);
        assert!(!*ctx.hotkeys_suspended.lock());
        release.send(()).unwrap();
        assert!(
            !callback.join().unwrap(),
            "The previous native registration must stay inert after capture ends"
        );
        assert_eq!(take_session_command(&ctx), None);
        assert_eq!(
            *ctx.state.lock(),
            crate::app::machine::SessionState::Recording
        );
        let current = ctx.shortcut_capture_epoch.load(Ordering::SeqCst);
        assert!(enqueue_session_command_for_epoch(
            &ctx,
            SessionCommand::Toggle,
            current
        ));
        assert_eq!(take_session_command(&ctx), Some(SessionCommand::Toggle));
    }

    #[test]
    fn queued_commands_cannot_cross_a_capture_epoch_even_after_resume() {
        let directory = tempfile::tempdir().unwrap();
        let ctx = AppContext::initialize(directory.path().to_path_buf()).unwrap();
        let old_epoch = ctx.shortcut_capture_epoch.load(Ordering::SeqCst);
        set_capture_intent(&ctx, true);
        set_capture_intent(&ctx, false);
        // Simulate a callback which was already scheduled before the editor opened.
        *ctx.pending_session_command.lock() = Some(QueuedSessionCommand {
            command: SessionCommand::Cancel,
            epoch: old_epoch,
        });
        assert_eq!(take_session_command(&ctx), None);
        assert!(ctx.pending_session_command.lock().is_none());
        assert_eq!(*ctx.state.lock(), crate::app::machine::SessionState::Idle);
    }

    #[test]
    fn capture_restore_renews_same_shortcut_and_rejects_stale_hook_updates() {
        let directory = tempfile::tempdir().unwrap();
        let ctx = AppContext::initialize(directory.path().to_path_buf()).unwrap();
        let old = ShortcutLayout {
            hotkey: ctx.settings.lock().hotkey.clone(),
            suspended: false,
            capture_epoch: 0,
        };
        let mut applied = Some(old.clone());
        set_capture_intent(&ctx, true);
        let epoch = ctx.shortcut_capture_epoch.load(Ordering::SeqCst);
        set_capture_intent(&ctx, true);
        assert_eq!(
            ctx.shortcut_capture_epoch.load(Ordering::SeqCst),
            epoch,
            "Duplicate capture intent is idempotent"
        );
        set_capture_intent(&ctx, false);
        let restored = ShortcutLayout {
            capture_epoch: epoch,
            ..old.clone()
        };
        let mut registrations = 0;
        apply_shortcut_layout(&mut applied, restored.clone(), |layout| {
            registrations += 1;
            assert_eq!(layout.hotkey, old.hotkey);
            assert_eq!(layout.capture_epoch, epoch);
            Ok(())
        })
        .unwrap();
        assert_eq!(
            registrations, 1,
            "Returning to the same key still needs a fresh callback epoch"
        );
        assert!(!hook_update_is_current(&ctx, &old));
        assert!(hook_update_is_current(&ctx, &restored));
        assert!(!hook_update_is_current(
            &ctx,
            &ShortcutLayout {
                suspended: true,
                ..restored
            }
        ));
    }

    #[test]
    fn capture_intent_suppresses_commands_before_native_registration_ack() {
        let directory = tempfile::tempdir().unwrap();
        let ctx = Arc::new(AppContext::initialize(directory.path().to_path_buf()).unwrap());
        *ctx.state.lock() = crate::app::machine::SessionState::Recording;
        assert!(enqueue_session_command(&ctx, SessionCommand::Toggle));
        let (release, wait) = std::sync::mpsc::sync_channel(1);
        let (ack, acknowledged) = std::sync::mpsc::sync_channel(1);
        let native = thread::spawn(move || {
            wait.recv().unwrap();
            ack.send(()).unwrap();
        });
        set_capture_intent(&ctx, true);
        assert!(matches!(
            acknowledged.try_recv(),
            Err(std::sync::mpsc::TryRecvError::Empty)
        ));
        assert!(!enqueue_session_command(&ctx, SessionCommand::Toggle));
        assert!(!enqueue_session_command(&ctx, SessionCommand::Cancel));
        assert_eq!(take_session_command(&ctx), None);
        assert_eq!(
            *ctx.state.lock(),
            crate::app::machine::SessionState::Recording
        );
        release.send(()).unwrap();
        acknowledged.recv().unwrap();
        native.join().unwrap();
        set_capture_intent(&ctx, false);
        assert!(enqueue_session_command(&ctx, SessionCommand::Toggle));
        assert_eq!(take_session_command(&ctx), Some(SessionCommand::Toggle));
    }

    #[test]
    fn queued_global_command_is_rechecked_before_dispatch_while_capturing_a_shortcut() {
        let directory = tempfile::tempdir().unwrap();
        let ctx = AppContext::initialize(directory.path().to_path_buf()).unwrap();
        set_capture_intent(&ctx, true);
        // A previously scheduled callback must fail closed even if an older
        // producer left a command in its slot before observing the new intent.
        *ctx.pending_session_command.lock() = Some(QueuedSessionCommand {
            command: SessionCommand::Cancel,
            epoch: ctx.shortcut_capture_epoch.load(Ordering::SeqCst),
        });
        assert_eq!(take_session_command(&ctx), None);
        assert_eq!(*ctx.state.lock(), crate::app::machine::SessionState::Idle);
        assert!(ctx.pending_session_command.lock().is_none());
    }

    #[test]
    fn failed_registration_does_not_skip_restoring_the_previous_hotkey() {
        let previous = ShortcutLayout {
            hotkey: "Ctrl+Shift+Space".into(),
            suspended: false,
            capture_epoch: 0,
        };
        let desired = ShortcutLayout {
            hotkey: "Alt+X".into(),
            suspended: false,
            capture_epoch: 0,
        };
        let mut applied = Some(previous.clone());
        let mut registered = Some(previous.hotkey.clone());
        assert!(apply_shortcut_layout(&mut applied, desired, |_| {
            registered = None; // The native unregister succeeded before registration failed.
            Err(AppError::HotkeyFailed("occupied".into()))
        })
        .is_err());
        assert!(applied.is_none());
        apply_shortcut_layout(&mut applied, previous.clone(), |layout| {
            registered = Some(layout.hotkey.clone());
            Ok(())
        })
        .unwrap();
        assert_eq!(registered, Some(previous.hotkey.clone()));
        assert_eq!(applied, Some(previous.clone()));
        apply_shortcut_layout(&mut applied, previous, |_| {
            panic!("unchanged layout must not re-register")
        })
        .unwrap();
    }

    #[test]
    fn idle_does_not_want_escape() {
        assert!(!wants_escape_hotkey(false, false));
        assert!(wants_escape_hotkey(true, false));
        assert!(!wants_escape_hotkey(true, true));
    }

    #[test]
    fn coalesced_sync_keeps_latest_ticket() {
        assert!(should_apply_scheduled_sync(3, 3));
        assert!(!should_apply_scheduled_sync(2, 3));
    }

    #[test]
    fn hotkey_layout_ignores_cancellable() {
        let registered = ShortcutLayout {
            hotkey: "Control+Space".into(),
            suspended: false,
            capture_epoch: 0,
        };
        assert!(!shortcut_layout_changed(Some(&registered), &registered));
        let suspended = ShortcutLayout {
            hotkey: registered.hotkey.clone(),
            suspended: true,
            capture_epoch: 0,
        };
        assert!(shortcut_layout_changed(Some(&registered), &suspended));
        let other_hotkey = ShortcutLayout {
            hotkey: "Alt+X".into(),
            suspended: false,
            capture_epoch: 0,
        };
        assert!(shortcut_layout_changed(Some(&registered), &other_hotkey));
        assert!(shortcut_layout_changed(None, &registered));
        assert!(!wants_escape_hotkey(false, false));
    }

    #[test]
    fn cancel_wins_over_pending_toggle() {
        assert_eq!(
            coalesce_session_command(Some(SessionCommand::Toggle), SessionCommand::Cancel),
            SessionCommand::Cancel
        );
        assert_eq!(
            coalesce_session_command(Some(SessionCommand::Cancel), SessionCommand::Toggle),
            SessionCommand::Cancel
        );
        assert_eq!(
            coalesce_session_command(Some(SessionCommand::Toggle), SessionCommand::Toggle),
            SessionCommand::Toggle
        );
    }

    #[test]
    fn register_failure_uses_ll_hook() {
        assert!(escape_register_fallback_to_ll_hook(false));
        assert!(!escape_register_fallback_to_ll_hook(true));
    }
}
