//! Opt-in Win32 HUD. This thread owns every HWND/GDI resource; no WebView or audio worker.
use crate::app::{
    machine::{is_cancellable, SessionState},
    overlay::WorkArea,
    overlay_controller::OverlaySnapshot,
};
use std::sync::{
    atomic::{AtomicBool, AtomicIsize, Ordering},
    Arc,
};
use std::time::{Duration, Instant};
use windows::core::{w, Result};
use windows::Win32::Foundation::*;
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::WindowsAndMessaging::*;

mod render;
const GEOMETRY_DIRTY: u32 = WM_APP + 1;
#[cfg(test)]
mod tests;

pub struct Frame {
    pub snapshot: OverlaySnapshot,
    pub epoch: u64,
    pub generation: u64,
    pub theme: String,
    pub accent_color: crate::settings::AccentColor,
    pub locale: String,
    pub target: isize,
    pub levels: Vec<f32>,
}

type Source = Box<dyn Fn() -> Option<Frame> + Send>;
type Cancel = Box<dyn Fn(u64, u64) + Send>;

pub struct NativeHud {
    hwnd: Arc<AtomicIsize>,
    stop: Arc<AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl NativeHud {
    pub fn spawn(
        source: impl Fn() -> Option<Frame> + Send + 'static,
        cancel: impl Fn(u64, u64) + Send + 'static,
    ) -> std::result::Result<Self, String> {
        let hwnd = Arc::new(AtomicIsize::new(0));
        let stop = Arc::new(AtomicBool::new(false));
        let (ready_tx, ready_rx) = std::sync::mpsc::sync_channel(1);
        let thread_hwnd = hwnd.clone();
        let thread_stop = stop.clone();
        let thread = std::thread::Builder::new()
            .name("native-hud".into())
            .spawn(move || unsafe {
                let mut data = Box::new(WindowData {
                    source: Box::new(source),
                    cancel: Box::new(cancel),
                    hwnd: thread_hwnd,
                    stop: thread_stop,
                    frame: None,
                    shown: false,
                    hovered: false,
                    pressed: None,
                    started: Instant::now(),
                    position: POINT::default(),
                    scale: 1.0,
                    painter: None,
                    geometry_dirty: true,
                });
                let result = create_window(data.as_mut());
                let _ = ready_tx.send(result.as_ref().map(|_| ()).map_err(ToString::to_string));
                if let Ok(window) = result {
                    let mut msg = MSG::default();
                    while GetMessageW(&mut msg, None, 0, 0).0 > 0 {
                        let _ = TranslateMessage(&msg);
                        DispatchMessageW(&msg);
                    }
                    // WM_CLOSE normally destroys it; also clean up after GetMessage failure.
                    if IsWindow(window).as_bool() {
                        let _ = DestroyWindow(window);
                    }
                }
                data.hwnd.store(0, Ordering::SeqCst);
            })
            .map_err(|e| e.to_string())?;
        let hud = Self {
            hwnd,
            stop,
            thread: Some(thread),
        };
        ready_rx
            .recv_timeout(Duration::from_secs(3))
            .map_err(|e| e.to_string())??;
        Ok(hud)
    }

    pub fn hwnd(&self) -> isize {
        self.hwnd.load(Ordering::SeqCst)
    }
}

impl Drop for NativeHud {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        let raw = self.hwnd();
        if raw != 0 {
            unsafe {
                let _ = PostMessageW(HWND(raw as *mut _), WM_CLOSE, WPARAM(0), LPARAM(0));
            }
        }
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

struct WindowData {
    source: Source,
    cancel: Cancel,
    hwnd: Arc<AtomicIsize>,
    stop: Arc<AtomicBool>,
    frame: Option<Frame>,
    shown: bool,
    hovered: bool,
    pressed: Option<(u64, u64)>,
    started: Instant,
    position: POINT,
    scale: f64,
    painter: Option<render::Painter>,
    geometry_dirty: bool,
}

unsafe fn create_window(data: &mut WindowData) -> Result<HWND> {
    let module = GetModuleHandleW(None)?;
    let class = w!("VoxelyNativeHud");
    let wc = WNDCLASSW {
        lpfnWndProc: Some(window_proc),
        hInstance: module.into(),
        lpszClassName: class,
        hCursor: LoadCursorW(None, IDC_ARROW)?,
        ..Default::default()
    };
    // One worker in production, parallel isolated fixtures may reuse this class.
    if RegisterClassW(&wc) == 0 && GetLastError() != ERROR_CLASS_ALREADY_EXISTS {
        return Err(windows::core::Error::from_win32());
    }
    let hwnd = CreateWindowExW(
        WS_EX_LAYERED | WS_EX_NOACTIVATE | WS_EX_TOOLWINDOW | WS_EX_TOPMOST,
        class,
        w!("Voxely Native HUD"),
        WS_POPUP,
        0,
        0,
        320,
        72,
        None,
        None,
        module,
        Some(data as *mut _ as *const _),
    )?;
    data.hwnd.store(hwnd.0 as isize, Ordering::SeqCst);
    if SetTimer(hwnd, 1, 50, None) == 0 {
        let error = windows::core::Error::from_win32();
        let _ = DestroyWindow(hwnd);
        return Err(error);
    }
    Ok(hwnd)
}

unsafe extern "system" fn window_proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    if msg == WM_NCCREATE {
        let create = &*(lp.0 as *const CREATESTRUCTW);
        SetWindowLongPtrW(hwnd, GWLP_USERDATA, create.lpCreateParams as isize);
    }
    let pointer = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut WindowData;
    if !pointer.is_null() {
        match msg {
            WM_TIMER => {
                if (*pointer).stop.load(Ordering::SeqCst) {
                    let _ = DestroyWindow(hwnd);
                } else if let Err(error) = (*pointer).tick(hwnd) {
                    // Never show an unpainted/black window after a compositing failure.
                    let _ = ShowWindow(hwnd, SW_HIDE);
                    (*pointer).shown = false;
                    tracing::error!(%error, "native HUD frame failed");
                    let _ = DestroyWindow(hwnd);
                }
                return LRESULT(0);
            }
            WM_MOUSEACTIVATE => return LRESULT(MA_NOACTIVATE as isize),
            WM_DPICHANGED | WM_DISPLAYCHANGE => {
                // UpdateLayeredWindow/SetWindowPos can synchronously reenter this
                // procedure while tick owns &mut WindowData. Defer all mutation.
                let _ = PostMessageW(hwnd, GEOMETRY_DIRTY, WPARAM(0), LPARAM(0));
                return LRESULT(0);
            }
            GEOMETRY_DIRTY => {
                (*pointer).geometry_dirty = true;
                return LRESULT(0);
            }
            WM_LBUTTONDOWN => {
                let data = &mut *pointer;
                if data.hovered {
                    data.pressed = data
                        .frame
                        .as_ref()
                        .filter(|f| is_cancellable(&f.snapshot.state))
                        .map(|f| (f.generation, f.epoch));
                }
                return LRESULT(0);
            }
            WM_LBUTTONUP => {
                let data = &mut *pointer;
                if let Some(ticket) = data.pressed.take() {
                    if data.pointer_inside(hwnd)
                        && data.frame.as_ref().is_some_and(|f| {
                            f.snapshot.visible
                                && is_cancellable(&f.snapshot.state)
                                && (f.generation, f.epoch) == ticket
                        })
                    {
                        (data.cancel)(ticket.0, ticket.1);
                    }
                }
                return LRESULT(0);
            }
            WM_CLOSE => {
                let _ = DestroyWindow(hwnd);
                return LRESULT(0);
            }
            WM_DESTROY => {
                let _ = KillTimer(hwnd, 1);
                (*pointer).hwnd.store(0, Ordering::SeqCst);
                PostQuitMessage(0);
                return LRESULT(0);
            }
            WM_NCDESTROY => {
                SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0);
            }
            _ => {}
        }
    }
    DefWindowProcW(hwnd, msg, wp, lp)
}

impl WindowData {
    unsafe fn pointer_inside(&self, hwnd: HWND) -> bool {
        let mut point = POINT::default();
        if GetCursorPos(&mut point).is_err() || WindowFromPoint(point) != hwnd {
            return false;
        }
        let x = (point.x - self.position.x) as f64 / self.scale;
        let y = (point.y - self.position.y) as f64 / self.scale;
        render::pill_contains(x, y)
    }

    unsafe fn tick(&mut self, hwnd: HWND) -> Result<()> {
        let Some(frame) = (self.source)() else {
            return Ok(());
        };
        let fresh = self
            .frame
            .as_ref()
            .is_none_or(|old| old.epoch != frame.epoch || old.generation != frame.generation);
        if fresh {
            self.started = Instant::now();
            self.hovered = false;
            self.pressed = None;
        }
        if !frame.snapshot.visible {
            if self.shown {
                let _ = ShowWindow(hwnd, SW_HIDE);
            }
            self.shown = false;
            self.hovered = false;
            self.pressed = None;
            self.frame = Some(frame);
            return Ok(());
        }
        if !self.shown || fresh || self.geometry_dirty {
            let work = super::overlay::work_area_for_hwnd(frame.target)
                .or_else(super::overlay::work_area_for_cursor)
                .unwrap_or(WorkArea {
                    left: 0,
                    top: 0,
                    right: 1920,
                    bottom: 1040,
                });
            self.scale = super::overlay::dpi_scale_for_hwnd(frame.target)
                .or_else(super::overlay::dpi_scale_for_cursor)
                .unwrap_or(1.0)
                .clamp(0.75, 4.0);
            let (x, y) = crate::app::overlay::overlay_physical_position(
                work,
                (320.0 * self.scale).round() as u32,
                (72.0 * self.scale).round() as u32,
                (48.0 * self.scale).round() as i32,
            );
            self.position = POINT { x, y };
        }
        self.hovered =
            self.shown && self.pointer_inside(hwnd) && is_cancellable(&frame.snapshot.state);
        if !self.hovered {
            self.pressed = None;
        }
        if self
            .painter
            .as_ref()
            .is_none_or(|p| p.scale() != self.scale)
        {
            self.painter = Some(render::Painter::new(self.scale)?);
        }
        if let Some(painter) = self.painter.as_mut() {
            painter.paint(
                hwnd,
                &frame,
                self.hovered,
                self.started.elapsed(),
                self.position,
            )?;
        }
        if !self.shown {
            SetWindowPos(
                hwnd,
                HWND_TOPMOST,
                0,
                0,
                0,
                0,
                SWP_NOACTIVATE | SWP_NOMOVE | SWP_NOSIZE | SWP_SHOWWINDOW,
            )?;
        }
        self.shown = true;
        self.geometry_dirty = false;
        self.frame = Some(frame);
        Ok(())
    }
}

fn system_dark() -> bool {
    use winreg::{enums::HKEY_CURRENT_USER, RegKey};
    RegKey::predef(HKEY_CURRENT_USER)
        .open_subkey("Software\\Microsoft\\Windows\\CurrentVersion\\Themes\\Personalize")
        .and_then(|key| key.get_value::<u32, _>("AppsUseLightTheme"))
        .unwrap_or(0)
        == 0
}
