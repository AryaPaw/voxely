pub struct WorkArea {
    pub left: i32,
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
}

pub const OVERLAY_WIDTH: f64 = 440.0;
pub const OVERLAY_HEIGHT: f64 = 108.0;
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
        assert_eq!(OVERLAY_WIDTH, 440.0);
        assert_eq!(OVERLAY_HEIGHT, 108.0);
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
}
