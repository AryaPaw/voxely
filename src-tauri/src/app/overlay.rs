use serde::Serialize;
use std::time::Instant;

pub struct WorkArea {
    pub left: i32,
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
}

pub const OVERLAY_WIDTH: f64 = 320.0;
pub const OVERLAY_HEIGHT: f64 = 72.0;
pub const OVERLAY_GAP_PX: i32 = 48;

pub fn overlay_physical_position(work: WorkArea, width: u32, height: u32, gap: i32) -> (i32, i32) {
    let area_w = (work.right - work.left).max(0);
    let x = work.left + (area_w - width as i32).max(0) / 2;
    let y = (work.bottom - height as i32 - gap).max(work.top);
    (x, y)
}

pub fn center_physical_position(work: WorkArea, width: u32, height: u32) -> (i32, i32) {
    let area_w = (work.right - work.left).max(0);
    let area_h = (work.bottom - work.top).max(0);
    let x = work.left + (area_w - width as i32).max(0) / 2;
    let y = work.top + (area_h - height as i32).max(0) / 2;
    (x, y)
}

#[derive(Debug, Default, Clone)]
pub struct OverlayTimeline {
    origin: Option<Instant>,
    window_created: Option<Instant>,
    react_mounted: Option<Instant>,
    first_frame: Option<Instant>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct OverlayTimingReport {
    pub hotkey_ms: Option<u128>,
    pub window_created_ms: Option<u128>,
    pub react_mounted_ms: Option<u128>,
    pub first_frame_ms: Option<u128>,
}

fn since_origin(origin: Instant, mark: Option<Instant>) -> Option<u128> {
    Some(mark?.saturating_duration_since(origin).as_millis())
}

impl OverlayTimeline {
    pub fn begin_show(&mut self) {
        let now = Instant::now();
        *self = Self {
            origin: Some(now),
            ..Self::default()
        };
    }

    pub fn mark_window_created(&mut self) {
        if self.window_created.is_none() {
            self.window_created = Some(Instant::now());
        }
    }

    pub fn mark_phase(&mut self, phase: &str) {
        match phase {
            "react" if self.react_mounted.is_none() => {
                self.react_mounted = Some(Instant::now());
            }
            "frame" if self.first_frame.is_none() => {
                self.first_frame = Some(Instant::now());
            }
            _ => {}
        }
    }

    pub fn hide(&mut self) {
        self.origin = None;
    }

    pub fn elapsed_ms(&self) -> Option<u128> {
        self.origin.map(|t| t.elapsed().as_millis())
    }

    pub fn report(&self) -> OverlayTimingReport {
        let Some(origin) = self.origin else {
            return OverlayTimingReport {
                hotkey_ms: None,
                window_created_ms: None,
                react_mounted_ms: None,
                first_frame_ms: None,
            };
        };
        OverlayTimingReport {
            hotkey_ms: Some(0),
            window_created_ms: since_origin(origin, self.window_created),
            react_mounted_ms: since_origin(origin, self.react_mounted),
            first_frame_ms: since_origin(origin, self.first_frame),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn places_center_bottom_not_screen_center() {
        let work = WorkArea {
            left: 0,
            top: 0,
            right: 1920,
            bottom: 1040,
        };
        let (x, y) = overlay_physical_position(
            work,
            OVERLAY_WIDTH as u32,
            OVERLAY_HEIGHT as u32,
            OVERLAY_GAP_PX,
        );
        assert_eq!(x, (1920 - OVERLAY_WIDTH as i32) / 2);
        assert_eq!(y, 1040 - OVERLAY_HEIGHT as i32 - OVERLAY_GAP_PX);
        assert_eq!(OVERLAY_WIDTH, 320.0);
        assert_eq!(OVERLAY_HEIGHT, 72.0);
    }

    #[test]
    fn respects_secondary_monitor_origin() {
        let work = WorkArea {
            left: 1920,
            top: 100,
            right: 3840,
            bottom: 1180,
        };
        let (x, y) = overlay_physical_position(
            work,
            OVERLAY_WIDTH as u32,
            OVERLAY_HEIGHT as u32,
            OVERLAY_GAP_PX,
        );
        assert_eq!(x, 1920 + (1920 - OVERLAY_WIDTH as i32) / 2);
        assert_eq!(y, 1180 - OVERLAY_HEIGHT as i32 - OVERLAY_GAP_PX);
    }

    #[test]
    fn centers_main_window_in_work_area() {
        let work = WorkArea {
            left: 1920,
            top: 0,
            right: 3840,
            bottom: 1080,
        };
        let (x, y) = center_physical_position(work, 960, 680);
        assert_eq!(x, 1920 + (1920 - 960) / 2);
        assert_eq!(y, (1080 - 680) / 2);
    }

    #[test]
    fn overlay_timeline_is_first_paint_not_show_elapsed_only() {
        let mut timeline = OverlayTimeline::default();
        assert!(timeline.elapsed_ms().is_none());
        timeline.begin_show();
        timeline.mark_window_created();
        timeline.mark_phase("react");
        timeline.mark_phase("frame");
        let report = timeline.report();
        assert_eq!(report.hotkey_ms, Some(0));
        assert!(report.window_created_ms.is_some());
        assert!(report.react_mounted_ms.is_some());
        assert!(report.first_frame_ms.is_some());
        timeline.hide();
        assert!(timeline.elapsed_ms().is_none());
    }

    #[test]
    fn overlay_window_is_not_declared_in_tauri_conf() {
        let conf = include_str!("../../tauri.conf.json");
        let session = include_str!("session.rs");
        assert!(!conf.contains("\"label\": \"overlay\""));
        assert!(session.contains("overlay.html"));
        assert!(session.contains(".transparent(true)"));
        assert!(!session.contains("index.html?overlay="));
    }
}
