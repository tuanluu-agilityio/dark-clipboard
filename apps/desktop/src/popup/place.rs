//! Where the popup goes (Task 204): near the cursor, on the monitor under it, inside that
//! monitor's work area. Pure, in physical pixels; the Win32 queries live in `winsys.rs`.

/// Popup size in logical pixels (scaled by the monitor's DPI).
pub const POPUP_LOGICAL: (u32, u32) = (440, 520);
/// Popup width with the preview pane open, in logical pixels (Task 702).
pub const PREVIEW_LOGICAL_WIDTH: u32 = 760;
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

/// How the preview pane is shown.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PaneMode {
    /// The window grows to the wide width; the list keeps its width.
    Side,
    /// The monitor is too narrow: the window keeps `list_w` and the pane draws over the list.
    Overlay,
}

/// Where a popup at `origin` (physical) goes when it widens from `list_w` to `wide_w` on the
/// monitor with work area `work`. The caller scales both widths with the DPI of the monitor the
/// window is on. The left edge stays put unless the wide window would leave the work area, then
/// it shifts left just enough (never below `work.left`); `y` never changes. A monitor narrower
/// than `wide_w` keeps the origin and says `Overlay`. Closing restores the width, not the
/// position (no second jump).
pub fn widen(origin: (i32, i32), list_w: i32, wide_w: i32, work: Rect) -> ((i32, i32), PaneMode) {
    let _ = list_w;
    let work_w = i64::from(work.right) - i64::from(work.left);
    if i64::from(wide_w) > work_w {
        return (origin, PaneMode::Overlay);
    }
    let x = origin
        .0
        .min(work.right.saturating_sub(wide_w))
        .max(work.left);
    ((x, origin.1), PaneMode::Side)
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

    // ---- widen (Task 702) ----

    fn wide(origin: (i32, i32), dpi: u32, work: Rect) -> ((i32, i32), PaneMode) {
        widen(
            origin,
            scale(440, dpi),
            scale(to_i32(PREVIEW_LOGICAL_WIDTH), dpi),
            work,
        )
    }

    fn to_i32(v: u32) -> i32 {
        i32::try_from(v).unwrap_or(i32::MAX)
    }

    #[test]
    fn widening_mid_screen_keeps_the_left_edge() {
        assert_eq!(wide((740, 212), 96, WORK), ((740, 212), PaneMode::Side));
    }

    #[test]
    fn widening_at_the_right_edge_shifts_left_once() {
        // The popup sits at x = 1480 (right edge of 1920); 760 wide needs x <= 1160.
        assert_eq!(wide((1480, 112), 96, WORK), ((1160, 112), PaneMode::Side));
        // Already shifted: widening again from there does not move it.
        assert_eq!(wide((1160, 112), 96, WORK), ((1160, 112), PaneMode::Side));
    }

    #[test]
    fn widening_on_a_second_monitor_never_goes_below_its_left_edge() {
        let left = Rect {
            left: -1920,
            top: 0,
            right: 0,
            bottom: 1040,
        };
        // At its left edge: stays.
        assert_eq!(wide((-1920, 50), 96, left), ((-1920, 50), PaneMode::Side));
        // At its right edge: shifted to right - 760.
        assert_eq!(wide((-440, 50), 96, left), ((-760, 50), PaneMode::Side));
    }

    #[test]
    fn exactly_760_wide_is_a_side_pane_at_the_left_edge() {
        let w = Rect {
            left: 100,
            top: 0,
            right: 860,
            bottom: 1040,
        };
        assert_eq!(wide((400, 10), 96, w), ((100, 10), PaneMode::Side));
    }

    #[test]
    fn a_monitor_narrower_than_760_logical_overlays_the_list() {
        let narrow = Rect {
            left: 0,
            top: 0,
            right: 759,
            bottom: 1040,
        };
        assert_eq!(wide((300, 10), 96, narrow), ((300, 10), PaneMode::Overlay));
    }

    #[test]
    fn widening_scales_with_the_dpi_of_the_monitor() {
        // 150 %: 660 -> 1140 on a 2880 wide work area.
        let w150 = Rect {
            left: 0,
            top: 0,
            right: 2880,
            bottom: 1560,
        };
        assert_eq!(wide((900, 20), 144, w150), ((900, 20), PaneMode::Side));
        assert_eq!(wide((2220, 20), 144, w150), ((1740, 20), PaneMode::Side));
        // 200 %: 1520 on a 3840 wide area; and a 3840 area is too narrow at 600 %.
        let w200 = Rect {
            left: 0,
            top: 0,
            right: 3840,
            bottom: 2080,
        };
        assert_eq!(wide((3000, 20), 192, w200), ((2320, 20), PaneMode::Side));
        assert_eq!(wide((3000, 20), 576, w200), ((3000, 20), PaneMode::Overlay));
    }

    #[test]
    fn mixed_dpi_monitors_each_use_their_own_scale() {
        // A 100 % monitor on the left, a 200 % one on the right (physical coordinates).
        let m100 = Rect {
            left: 0,
            top: 0,
            right: 1920,
            bottom: 1040,
        };
        let m200 = Rect {
            left: 1920,
            top: 0,
            right: 1920 + 3840,
            bottom: 2080,
        };
        assert_eq!(wide((1480, 0), 96, m100), ((1160, 0), PaneMode::Side));
        // Same cursor-relative spot on the 200 % monitor: 880 -> 1520 physical wide.
        assert_eq!(wide((4840, 0), 192, m200), ((4240, 0), PaneMode::Side));
        // The 100 % result must not be used for the 200 % monitor.
        assert_ne!(wide((4840, 0), 192, m200).0 .0, 4840 - 320);
    }

    #[test]
    fn widening_never_changes_y() {
        for y in [i32::MIN, -5, 0, 700, i32::MAX] {
            assert_eq!(wide((1480, y), 96, WORK).0 .1, y);
            assert_eq!(wide((10, y), 96, WORK).0 .1, y);
        }
    }

    #[test]
    fn widening_does_not_overflow_on_extreme_inputs() {
        let huge = Rect {
            left: i32::MIN,
            top: i32::MIN,
            right: i32::MAX,
            bottom: i32::MAX,
        };
        let _ = widen((i32::MAX, 0), i32::MAX, i32::MAX, huge);
        let _ = widen((i32::MIN, 0), i32::MIN, i32::MIN, huge);
        let _ = widen((i32::MAX, i32::MAX), 440, 760, huge);
        let _ = widen((0, 0), 440, i32::MAX, WORK);
        let (_, mode) = widen((0, 0), 440, i32::MAX, WORK);
        assert_eq!(mode, PaneMode::Overlay);
    }
}
