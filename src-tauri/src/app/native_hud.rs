//! Experimental renderer selection is process-local; persisted settings stay unchanged.
use crate::app::session::AppContext;
use crate::windows_int::native_hud::{Frame, NativeHud};
use std::sync::{atomic::Ordering, Arc};
use tauri::{AppHandle, Manager};

pub fn prepare(app: &AppHandle) -> bool {
    let ctx = app.state::<Arc<AppContext>>();
    if ctx
        .native_hud
        .lock()
        .as_ref()
        .is_some_and(|hud| hud.hwnd() != 0)
    {
        return true;
    }
    ctx.native_hud.lock().take();
    if !std::env::args_os().any(|a| a == "--native-hud") {
        return false;
    }
    let weak = Arc::downgrade(ctx.inner());
    let handle = app.clone();
    match NativeHud::spawn(
        move || {
            let ctx = weak.upgrade()?;
            // Never make the HUD thread wait for capture, cancel or persistence.
            let _lifecycle = ctx.session_lifecycle.try_lock()?;
            let state = ctx.state.try_lock()?.clone();
            let overlay = *ctx.overlay.try_lock()?;
            let settings = ctx.settings.try_lock()?;
            let locale = if settings.ui_language == "auto" {
                system_locale()
            } else {
                settings.ui_language.clone()
            };
            let target = ctx
                .captured_target
                .try_lock()?
                .map(|target| target.hwnd as isize)
                .unwrap_or(0);
            let levels = if overlay.visible {
                ctx.capture
                    .try_lock()?
                    .as_ref()
                    .map(|capture| capture.meter().levels)
                    .unwrap_or_default()
            } else {
                Vec::new()
            };
            Some(Frame {
                snapshot: overlay.snapshot(state),
                epoch: overlay.epoch,
                generation: ctx.session_generation.load(Ordering::SeqCst),
                theme: settings.theme.clone(),
                accent_color: settings.accent_color.clone(),
                locale,
                target,
                levels,
            })
        },
        move |generation, epoch| {
            let handle = handle.clone();
            // Do not wait for the Tauri thread from the native window procedure.
            tauri::async_runtime::spawn_blocking(move || {
                if let Err(error) =
                    crate::app::session::cancel_recording_from_hud(&handle, generation, epoch)
                {
                    tracing::warn!(%error, "native HUD cancellation failed");
                }
            });
        },
    ) {
        Ok(hud) => {
            tracing::info!(hwnd = hud.hwnd(), "experimental native HUD ready");
            *ctx.native_hud.lock() = Some(hud);
            true
        }
        Err(error) => {
            tracing::error!(%error, "native HUD unavailable; using WebView HUD");
            false
        }
    }
}

fn system_locale() -> String {
    use winreg::{enums::HKEY_CURRENT_USER, RegKey};
    let name: String = RegKey::predef(HKEY_CURRENT_USER)
        .open_subkey("Control Panel\\International")
        .and_then(|key| key.get_value("LocaleName"))
        .unwrap_or_default();
    if name.to_ascii_lowercase().starts_with("ru") {
        "ru"
    } else {
        "en"
    }
    .into()
}
