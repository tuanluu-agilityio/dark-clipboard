//! Where the popup goes (Task 204): near the cursor, on the monitor under it, inside that
//! monitor's work area. Pure, in physical pixels; the Win32 queries live in `winsys.rs`.

/// Popup size in logical pixels (scaled by the monitor's DPI).
pub const POPUP_LOGICAL: (u32, u32) = (440, 520);
/// Gap between the cursor and the popup, in logical pixels.
const GAP_LOGICAL: i32 = 12;

/// A rectangle in physical pixels; `right` and `bottom` are exclusive.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rect {
    pub left: i32,
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
}

/// `logical` pixels at `dpi` (96 = 100 %), rounded.
pub fn scale(logical: i32, dpi: u32) -> i32 {
    let dpi = if dpi == 0 { 96 } else { i64::from(dpi) };
    let v = (i64::from(logical) * dpi * 2 + 96) / (96 * 2);
    i32::try_from(v).unwrap_or(logical)
}

/// Top-left corner of a popup of `size` (physical) for a cursor at `cursor` on a monitor whose
/// work area is `work` at `dpi`. Centered horizontally on the cursor, just below it; above it
/// when there is no room below; always inside the work area (top-left wins if it is too big).
pub fn popup_origin(cursor: (i32, i32), size: (i32, i32), work: Rect, dpi: u32) -> (i32, i32) {
    let (w, h) = size;
    let gap = scale(GAP_LOGICAL, dpi);
    let below = cursor.1.saturating_add(gap);
    let above = cursor.1.saturating_sub(gap).saturating_sub(h);
    let y = if below.saturating_add(h) <= work.bottom {
        below
    } else if above >= work.top {
        above
    } else {
        below
    };
    let x = cursor.0.saturating_sub(w / 2);
    // Clamp: the right/bottom limit first, then the left/top one (it wins when too big).
    let x = x.min(work.right.saturating_sub(w)).max(work.left);
    let y = y.min(work.bottom.saturating_sub(h)).max(work.top);
    (x, y)
}

/// Top-left corner of a popup of `size` (physical) centered in the work area `work` (the
/// "center of the screen" setting, Task 207); inside the work area, top-left wins if too big.
pub fn center_origin(size: (i32, i32), work: Rect) -> (i32, i32) {
    let x = work.left + (work.right - work.left - size.0) / 2;
    let y = work.top + (work.bottom - work.top - size.1) / 2;
    let x = x.min(work.right.saturating_sub(size.0)).max(work.left);
    let y = y.min(work.bottom.saturating_sub(size.1)).max(work.top);
    (x, y)
}

#[cfg(test)]
mod tests {
    use super::*;

    const WORK: Rect = Rect {
        left: 0,
        top: 0,
        right: 1920,
        bottom: 1040,
    };

    #[test]
    fn centered_in_the_work_area_and_never_outside_it() {
        assert_eq!(center_origin((440, 520), WORK), (740, 260));
        // Another monitor, with negative coordinates.
        let left = Rect {
            left: -1920,
            top: 0,
            right: 0,
            bottom: 1040,
        };
        assert_eq!(center_origin((440, 520), left), (-1180, 260));
        // Larger than the work area: the top-left corner stays visible.
        let tiny = Rect {
            left: 0,
            top: 0,
            right: 300,
            bottom: 300,
        };
        assert_eq!(center_origin((440, 520), tiny), (0, 0));
    }

    #[test]
    fn scales_by_dpi() {
        assert_eq!(scale(440, 96), 440);
        assert_eq!(scale(440, 144), 660);
        assert_eq!(scale(520, 120), 650);
        assert_eq!(scale(12, 168), 21);
        // A DPI of 0 (query failed) counts as 100 %.
        assert_eq!(scale(440, 0), 440);
    }

    #[test]
    fn below_and_centered_on_the_cursor() {
        assert_eq!(popup_origin((960, 200), (440, 520), WORK, 96), (740, 212));
    }

    #[test]
    fn above_the_cursor_when_there_is_no_room_below() {
        assert_eq!(popup_origin((960, 900), (440, 520), WORK, 96), (740, 368));
    }

    #[test]
    fn clamped_into_the_work_area() {
        // Right edge.
        assert_eq!(popup_origin((1910, 100), (440, 520), WORK, 96), (1480, 112));
        // Left edge.
        assert_eq!(popup_origin((5, 100), (440, 520), WORK, 96), (0, 112));
        // Middle of a short screen: neither below nor above fits.
        let short = Rect {
            bottom: 700,
            ..WORK
        };
        assert_eq!(popup_origin((960, 350), (440, 520), short, 96), (740, 180));
        // Larger than the work area: the top-left corner stays visible.
        let tiny = Rect {
            left: 0,
            top: 0,
            right: 300,
            bottom: 300,
        };
        assert_eq!(popup_origin((150, 150), (440, 520), tiny, 96), (0, 0));
    }

    #[test]
    fn a_secondary_monitor_with_negative_coordinates_and_another_dpi() {
        // A 150 % monitor left of the primary one; its taskbar takes 60 px at the bottom.
        let left = Rect {
            left: -2880,
            top: 0,
            right: 0,
            bottom: 1560,
        };
        let size = (scale(440, 144), scale(520, 144));
        assert_eq!(size, (660, 780));
        // Gap of 12 logical px = 18 physical px.
        assert_eq!(popup_origin((-1440, 300), size, left, 144), (-1770, 318));
        assert_eq!(popup_origin((-10, 1500), size, left, 144), (-660, 702));
    }
}
