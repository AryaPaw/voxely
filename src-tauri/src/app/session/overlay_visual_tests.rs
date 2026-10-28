use super::{
    build_overlay_window, hide_overlay_hwnd, reveal_overlay, AppContext, SessionState,
    OVERLAY_HEIGHT, OVERLAY_WIDTH,
};
use std::borrow::Cow;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tauri::{Manager, PhysicalPosition, Position, WebviewUrl, WebviewWindowBuilder};
use windows::Win32::Foundation::{HWND, POINT, RECT};
use windows::Win32::Graphics::Dwm::{DwmEnableBlurBehindWindow, DWM_BB_ENABLE, DWM_BLURBEHIND};
use windows::Win32::Graphics::Gdi::{ClientToScreen, GetDC, GetPixel, ReleaseDC};
use windows::Win32::UI::WindowsAndMessaging::{
    GetAncestor, GetForegroundWindow, GetWindowLongW, GetWindowRect, GetWindowThreadProcessId,
    IsWindowVisible, ShowWindow, WindowFromPoint, GA_ROOT, GWL_EXSTYLE, SW_SHOWNOACTIVATE,
    WS_EX_LAYERED, WS_EX_NOACTIVATE, WS_EX_NOREDIRECTIONBITMAP, WS_EX_TOOLWINDOW,
    WS_EX_TRANSPARENT,
};

struct FixtureAssets(std::path::PathBuf);

const FIXTURE_INSET: i32 = 96;

impl tauri::Assets<tauri::Wry> for FixtureAssets {
    fn get(&self, key: &tauri::utils::assets::AssetKey) -> Option<Cow<'_, [u8]>> {
        let path = key.as_ref().trim_start_matches('/');
        if path == "backdrop.html" {
            return Some(Cow::Borrowed(b"<html style='background:rgb(229,242,255);height:100%'><body style='margin:0;height:100%'></body></html>"));
        }
        if path == "legacy.html" {
            return Some(Cow::Borrowed(b"<html style='background:transparent'><body style='margin:0;background:transparent'><div style='margin:8px 12px;height:52px;border-radius:26px;background:rgb(10,20,30)'></div></body></html>"));
        }
        let relative = std::path::Path::new(path);
        if !relative
            .components()
            .all(|part| matches!(part, std::path::Component::Normal(_)))
        {
            return None;
        }
        std::fs::read(self.0.join(relative)).ok().map(Cow::Owned)
    }

    fn iter(&self) -> Box<tauri::utils::assets::AssetsIter<'_>> {
        Box::new(std::iter::empty())
    }

    fn csp_hashes(
        &self,
        _: &tauri::utils::assets::AssetKey,
    ) -> Box<dyn Iterator<Item = tauri::utils::assets::CspHash<'_>> + '_> {
        Box::new(std::iter::empty())
    }
}

fn check_hud_style(raw: isize, visible: bool) -> Result<(), String> {
    unsafe {
        let hwnd = HWND(raw as *mut _);
        let style = GetWindowLongW(hwnd, GWL_EXSTYLE) as u32;
        let required = WS_EX_NOACTIVATE.0 | WS_EX_TOOLWINDOW.0;
        if style & required != required
            || style & (WS_EX_LAYERED.0 | WS_EX_TRANSPARENT.0 | WS_EX_NOREDIRECTIONBITMAP.0) != 0
            || IsWindowVisible(hwnd).as_bool() != visible
            || GetForegroundWindow() == hwnd
        {
            return Err(format!(
                "HUD compositing/input invariants failed: {style:08x}, visible={visible}"
            ));
        }
        if visible {
            let mut rect = RECT::default();
            GetWindowRect(hwnd, &mut rect).map_err(|e| e.to_string())?;
            let hit = GetAncestor(
                WindowFromPoint(POINT {
                    x: (rect.left + rect.right) / 2,
                    y: (rect.top + rect.bottom) / 2,
                }),
                GA_ROOT,
            );
            if hit != hwnd {
                return Err("visible HUD doesn't receive pointer hit testing".into());
            }
        }
        Ok(())
    }
}

fn pixel_sample(backdrop: isize, overlay: isize) -> Result<String, String> {
    unsafe {
        let mut back = RECT::default();
        let mut hud = RECT::default();
        GetWindowRect(HWND(backdrop as *mut _), &mut back).map_err(|e| e.to_string())?;
        GetWindowRect(HWND(overlay as *mut _), &mut hud).map_err(|e| e.to_string())?;
        let mut client_origin = POINT::default();
        if !ClientToScreen(HWND(backdrop as *mut _), &mut client_origin).as_bool() {
            return Err("backdrop client origin unavailable".into());
        }
        let points = [
            POINT {
                x: client_origin.x + 12,
                y: client_origin.y + 12,
            },
            POINT {
                x: hud.left + 2,
                y: hud.top + 2,
            },
            POINT {
                x: (hud.left + hud.right) / 2,
                y: (hud.top + hud.bottom) / 2,
            },
        ];
        let mut client_bounds = RECT::default();
        windows::Win32::UI::WindowsAndMessaging::GetClientRect(
            HWND(backdrop as *mut _),
            &mut client_bounds,
        )
        .map_err(|e| e.to_string())?;
        for point in &points[1..] {
            if point.x < client_origin.x
                || point.y < client_origin.y
                || point.x >= client_origin.x + client_bounds.right
                || point.y >= client_origin.y + client_bounds.bottom
            {
                return Err(format!("fixture backdrop client doesn't contain HUD sample: back={back:?} hud={hud:?} origin={client_origin:?} client={client_bounds:?}"));
            }
        }
        // Read only pixels covered by our fixture. A user window covering any
        // point invalidates the sample instead of capturing its contents.
        for point in points {
            let root = GetAncestor(WindowFromPoint(point), GA_ROOT);
            let mut owner = 0;
            GetWindowThreadProcessId(root, Some(&mut owner));
            if owner != std::process::id() {
                return Err("fixture is occluded by another process".into());
            }
        }
        let dc = GetDC(HWND::default());
        if dc.0.is_null() {
            return Err("desktop DC unavailable".into());
        }
        let colors = points.map(|point| GetPixel(dc, point.x, point.y).0);
        let _ = ReleaseDC(HWND::default(), dc);
        let styles = GetWindowLongW(HWND(overlay as *mut _), GWL_EXSTYLE);
        let detail = format!(
            "background={:06x} corner={:06x} center={:06x} exstyle={styles:08x} backdrop={back:?} hud={hud:?}",
            colors[0], colors[1], colors[2]
        );
        if colors.contains(&0xffffffff) {
            return Err(format!("invalid desktop pixel: {detail}"));
        }
        if (colors[0] & 0xff) < 180 || ((colors[0] >> 8) & 0xff) < 180 {
            return Err(format!(
                "backdrop hasn't painted its light test color: {detail}"
            ));
        }
        if colors[0] == colors[2] {
            return Err(format!("opaque center didn't paint: {detail}"));
        }
        if (0..3).any(|shift| {
            let a = ((colors[0] >> (shift * 8)) & 0xff) as i32;
            let b = ((colors[1] >> (shift * 8)) & 0xff) as i32;
            (a - b).abs() > 4
        }) {
            return Err(format!(
                "transparent HUD corner doesn't reveal backdrop: {detail}"
            ));
        }
        Ok(detail)
    }
}

#[test]
#[ignore = "requires a visible Windows desktop; creates only isolated owned WebView2 windows"]
fn native_overlay_transparency_survives_visibility_cycles() {
    run_overlay_fixture(false);
}

#[test]
#[ignore = "requires a visible Windows desktop; uses only isolated owned windows and metadata"]
fn native_warmed_overlay_shows_after_hidden_idle() {
    run_overlay_fixture(true);
}

fn run_overlay_fixture(warmed: bool) {
    let temporary = tempfile::tempdir().unwrap();
    let dist = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../dist");
    assert!(
        dist.join("overlay.html").is_file(),
        "build frontend before running native fixture"
    );
    let mut context = tauri::generate_context!();
    context.set_assets(Box::new(FixtureAssets(dist)));
    context.config_mut().build.dev_url = None;
    context.config_mut().app.windows.clear();
    context.config_mut().identifier = format!("com.voxely.overlay-fixture.p{}", std::process::id());
    context.config_mut().app.app_directories_override = Some(
        tauri::utils::config::AppDirectoriesOverride::Root(temporary.path().to_path_buf()),
    );
    let result: Arc<Mutex<Option<Result<(), String>>>> = Arc::new(Mutex::new(None));
    let result_on_setup = Arc::clone(&result);
    let ctx = Arc::new(AppContext::initialize(temporary.path().join("data")).unwrap());
    ctx.settings.lock().theme = "dark".into();
    if !warmed {
        *ctx.state.lock() = SessionState::Recording;
        ctx.overlay.lock().show();
    }
    // No production bootstrap/plugins: no microphone, credentials, autostart,
    // history, global hotkeys, updater or diagnostic settings are touched.
    let app = tauri::Builder::default()
        .any_thread()
        .manage(ctx)
        .invoke_handler(tauri::generate_handler![
            crate::commands::get_settings,
            crate::commands::get_overlay_snapshot,
            crate::commands::get_meter,
            crate::commands::overlay_mark_frame,
            crate::commands::overlay_pointer_matches,
        ])
        .setup(move |app| {
            let backdrop = WebviewWindowBuilder::new(
                app.handle(),
                "backdrop",
                WebviewUrl::App("backdrop.html".into()),
            )
            .title("Voxely HUD transparency fixture")
            .decorations(false)
            .always_on_top(true)
            .focused(false)
            .focusable(false)
            .visible(false)
            .inner_size(500.0, 200.0)
            .position(80.0, 120.0)
            .build()?;
            let hud = if warmed {
                super::prepare_overlay_window(app.handle());
                app.get_webview_window("overlay").expect("prepared hidden overlay")
            } else {
                build_overlay_window(app.handle(), WebviewUrl::App("overlay.html".into()))?
            };
            let legacy = WebviewWindowBuilder::new(app.handle(), "legacy", WebviewUrl::App("legacy.html".into()))
                .title("Legacy HUD transparency control")
                .transparent(true).no_redirection_bitmap(false)
                .decorations(false).shadow(false).always_on_top(true)
                .focused(false).focusable(false).visible(false)
                .inner_size(OVERLAY_WIDTH, OVERLAY_HEIGHT).build()?;
            let pos = if warmed {
                let at = hud.inner_position()?;
                let position = PhysicalPosition::new(at.x - FIXTURE_INSET, at.y - FIXTURE_INSET);
                backdrop.set_position(Position::Physical(position))?;
                position
            } else {
                let position = backdrop.inner_position()?;
                hud.set_position(Position::Physical(PhysicalPosition::new(position.x + FIXTURE_INSET, position.y + FIXTURE_INSET)))?;
                position
            };
            let backdrop_raw = backdrop.hwnd()?.0 as isize;
            let hud_raw = hud.hwnd()?.0 as isize;
            let legacy_raw = legacy.hwnd()?.0 as isize;
            legacy.set_position(Position::Physical(PhysicalPosition::new(pos.x + FIXTURE_INSET, pos.y + FIXTURE_INSET)))?;
            unsafe {
                let _ = ShowWindow(HWND(backdrop_raw as *mut _), SW_SHOWNOACTIVATE);
            }
            if !warmed {
                reveal_overlay(&hud);
                hud.set_position(Position::Physical(PhysicalPosition::new(pos.x + FIXTURE_INSET, pos.y + FIXTURE_INSET)))?;
            }
            let handle = app.handle().clone();
            let worker_result = Arc::clone(&result_on_setup);
            std::thread::spawn(move || {
                // On panic the fixture is failed and immediately torn down; no
                // application state is resumed across this boundary.
                let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(
                    || -> Result<(), String> {
                        if warmed {
                            std::thread::sleep(Duration::from_millis(700));
                            check_hud_style(hud_raw, false)?;
                            on_ui(&handle, |app| {
                                *app.state::<Arc<AppContext>>().state.lock() = SessionState::Recording;
                                super::show_overlay(&app);
                                Ok(())
                            })?;
                        }
                        let deadline = std::time::Instant::now() + Duration::from_secs(5);
                        loop {
                            match pixel_sample(backdrop_raw, hud_raw) {
                                Err(error)
                                    if (error.starts_with("backdrop hasn't painted")
                                        || error.starts_with("opaque center didn't paint"))
                                        && std::time::Instant::now() < deadline =>
                                {
                                    std::thread::sleep(Duration::from_millis(100))
                                }
                                _ => break,
                            }
                        }
                        let mut failures = Vec::new();
                        for phase in 0..5 {
                            if phase >= 2 {
                                on_ui(&handle, |app| {
                                    let hud = app
                                        .get_webview_window("overlay")
                                        .ok_or("overlay missing")?;
                                    hide_overlay_hwnd(&hud);
                                    Ok(())
                                })?;
                                std::thread::sleep(Duration::from_millis(100));
                            }
                            if phase > 0 {
                                on_ui(&handle, move |app| {
                                    let hud = app
                                        .get_webview_window("overlay")
                                        .ok_or("overlay missing")?;
                                    if phase == 1 {
                                        crate::windows_int::overlay::apply_overlay_exstyle(hud_raw);
                                    } else {
                                        reveal_overlay(&hud);
                                        hud.set_position(Position::Physical(
                                            PhysicalPosition::new(pos.x + FIXTURE_INSET, pos.y + FIXTURE_INSET),
                                        ))
                                        .map_err(|e| e.to_string())?;
                                    }
                                    Ok(())
                                })?;
                                std::thread::sleep(Duration::from_millis(400));
                            }
                            match pixel_sample(backdrop_raw, hud_raw) {
                                Ok(sample) => println!("phase={phase} PASS {sample}"),
                                Err(error) => {
                                    println!("phase={phase} FAIL {error}");
                                    failures.push(error);
                                }
                            }
                        }
                        check_hud_style(hud_raw, true)?;
                        // Prove the legacy backing surface turns black when its
                        // DWM alpha state is lost, using only owned HWNDs.
                        on_ui(&handle, move |app| {
                            let hud = app.get_webview_window("overlay").ok_or("overlay missing")?;
                            hide_overlay_hwnd(&hud);
                            unsafe { let _ = ShowWindow(HWND(legacy_raw as *mut _), SW_SHOWNOACTIVATE); }
                            crate::windows_int::overlay::apply_overlay_exstyle(legacy_raw);
                            Ok(())
                        })?;
                        check_hud_style(hud_raw, false)?;
                        std::thread::sleep(Duration::from_millis(400));
                        println!("legacy_alpha_enabled PASS {}", pixel_sample(backdrop_raw, legacy_raw)?);
                        on_ui(&handle, move |_| {
                            unsafe {
                                DwmEnableBlurBehindWindow(HWND(legacy_raw as *mut _), &DWM_BLURBEHIND {
                                    dwFlags: DWM_BB_ENABLE, fEnable: false.into(), ..Default::default()
                                }).map_err(|e| e.to_string())?;
                            }
                            Ok(())
                        })?;
                        std::thread::sleep(Duration::from_millis(300));
                        let control = pixel_sample(backdrop_raw, legacy_raw);
                        if !matches!(&control, Err(error) if error.starts_with("transparent HUD corner")) {
                            return Err(format!("opaque negative control wasn't detected: {control:?}"));
                        }
                        println!("opaque_control EXPECTED_FAILURE {control:?}");
                        on_ui(&handle, move |app| {
                            let hud = app.get_webview_window("overlay").ok_or("overlay missing")?;
                            reveal_overlay(&hud);
                            hud.set_position(Position::Physical(PhysicalPosition::new(pos.x + FIXTURE_INSET, pos.y + FIXTURE_INSET)))
                                .map_err(|e| e.to_string())?;
                            check_hud_style(hud_raw, true)?;
                            unsafe { let _ = ShowWindow(HWND(legacy_raw as *mut _), windows::Win32::UI::WindowsAndMessaging::SW_HIDE); }
                            Ok(())
                        })?;
                        std::thread::sleep(Duration::from_millis(400));
                        // A subsequent normal show must restore lost alpha.
                        on_ui(&handle, move |_| {
                            unsafe {
                                DwmEnableBlurBehindWindow(HWND(hud_raw as *mut _), &DWM_BLURBEHIND {
                                    dwFlags: DWM_BB_ENABLE, fEnable: false.into(), ..Default::default()
                                }).map_err(|e| e.to_string())?;
                            }
                            Ok(())
                        })?;
                        std::thread::sleep(Duration::from_millis(400));
                        let damaged = pixel_sample(backdrop_raw, hud_raw);
                        if !matches!(&damaged, Err(error) if error.starts_with("transparent HUD corner")) {
                            return Err(format!("production alpha loss wasn't detected: {damaged:?}"));
                        }
                        on_ui(&handle, move |app| {
                            if warmed {
                                super::show_overlay(&app);
                            } else {
                                let hud = app.get_webview_window("overlay").ok_or("overlay missing")?;
                                reveal_overlay(&hud);
                                hud.set_position(Position::Physical(PhysicalPosition::new(pos.x + FIXTURE_INSET, pos.y + FIXTURE_INSET))).map_err(|e| e.to_string())?;
                            }
                            Ok(())
                        })?;
                        std::thread::sleep(Duration::from_millis(400));
                        println!("alpha_recovered_production PASS {}", pixel_sample(backdrop_raw, hud_raw)?);
                        check_hud_style(hud_raw, true)?;
                        for _ in 0..40 {
                            on_ui(&handle, move |app| {
                                let hud = app.get_webview_window("overlay").ok_or("overlay missing")?;
                                hide_overlay_hwnd(&hud);
                                check_hud_style(hud_raw, false)?;
                                reveal_overlay(&hud);
                                hud.set_position(Position::Physical(PhysicalPosition::new(pos.x + FIXTURE_INSET, pos.y + FIXTURE_INSET))).map_err(|e| e.to_string())?;
                                check_hud_style(hud_raw, true)
                            })?;
                        }
                        std::thread::sleep(Duration::from_millis(400));
                        println!("rapid_cycles PASS {}", pixel_sample(backdrop_raw, hud_raw)?);
                        if failures.is_empty() {
                            Ok(())
                        } else {
                            Err(failures.join("\n"))
                        }
                    },
                ))
                .unwrap_or_else(|_| Err("native fixture worker panicked".into()));
                *worker_result.lock().unwrap() = Some(outcome);
                handle.exit(0);
            });
            Ok(())
        })
        .build(context)
        .expect("build isolated real Wry runtime");
    let exit = app.run_return(|_, _| {});
    assert_eq!(exit, 0);
    result
        .lock()
        .unwrap()
        .take()
        .expect("fixture completed")
        .unwrap();
}

fn on_ui(
    app: &tauri::AppHandle,
    action: impl FnOnce(tauri::AppHandle) -> Result<(), String> + Send + 'static,
) -> Result<(), String> {
    let (tx, rx) = std::sync::mpsc::channel();
    let handle = app.clone();
    app.run_on_main_thread(move || {
        let _ = tx.send(action(handle));
    })
    .map_err(|e| e.to_string())?;
    rx.recv_timeout(Duration::from_secs(5))
        .map_err(|e| e.to_string())?
}
