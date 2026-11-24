//! Where the history window opens and what is remembered about it (Task 211). Pure, in
//! physical pixels (sizes are stored in logical pixels so they survive a DPI change). The
//! window is remembered per monitor: the key is the monitor's geometry, never a device name.
//! The Win32 queries live in `winsys.rs`.

use crate::popup::place::{scale, Rect};
use darkclip_app::settings::{WindowRect, MAX_REMEMBERED_MONITORS};
use std::collections::BTreeMap;

/// Size of a first-time window, in logical pixels.
pub const DEFAULT_LOGICAL: (u32, u32) = (1060, 660);
/// The smallest size the window can be resized to, in logical pixels (matches the markup).
pub const MIN_LOGICAL: (u32, u32) = (640, 460);

/// Where to put the window: physical position and size.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Placed {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

/// A stable name for a monitor from its full rectangle (physical pixels).
pub fn monitor_key(full: Rect) -> String {
    format!(
        "{},{},{}x{}",
        full.left,
        full.top,
        full.right.saturating_sub(full.left),
        full.bottom.saturating_sub(full.top)
    )
}

fn to_u32(v: i32) -> u32 {
    u32::try_from(v).unwrap_or(0)
}

fn to_i32(v: u32) -> i32 {
    i32::try_from(v).unwrap_or(i32::MAX)
}

/// The window's rectangle on a monitor with work area `work` at `dpi`: the remembered one
/// when there is one, else the default size centred. Always inside the work area (a window
/// too big for it is shrunk, but never below the minimum size unless the screen is smaller).
pub fn initial(saved: Option<WindowRect>, work: Rect, dpi: u32) -> Placed {
    let (lw, lh) = saved.map_or(DEFAULT_LOGICAL, |s| (s.width, s.height));
    let avail_w = to_u32(work.right.saturating_sub(work.left));
    let avail_h = to_u32(work.bottom.saturating_sub(work.top));
    let size = |logical: u32, min: u32, avail: u32| {
        let wanted = to_u32(scale(to_i32(logical), dpi));
        let floor = to_u32(scale(to_i32(min), dpi));
        wanted.max(floor).min(avail)
    };
    let width = size(lw, MIN_LOGICAL.0, avail_w);
    let height = size(lh, MIN_LOGICAL.1, avail_h);
    let (w, h) = (to_i32(width), to_i32(height));
    let (x, y) = match saved {
        Some(s) => (s.x, s.y),
        None => (
            work.left + (work.right - work.left - w) / 2,
            work.top + (work.bottom - work.top - h) / 2,
        ),
    };
    // Right/bottom first, then left/top (it wins when the window is too big).
    let x = x.min(work.right.saturating_sub(w)).max(work.left);
    let y = y.min(work.bottom.saturating_sub(h)).max(work.top);
    Placed {
        x,
        y,
        width,
        height,
    }
}

/// The rectangle to remember for a window at `pos` with physical `size` at `dpi`.
pub fn to_saved(pos: (i32, i32), size: (u32, u32), dpi: u32) -> WindowRect {
    let dpi = if dpi == 0 { 96 } else { dpi };
    let logical = |v: u32| {
        u32::try_from((u64::from(v) * 96 + u64::from(dpi) / 2) / u64::from(dpi)).unwrap_or(u32::MAX)
    };
    WindowRect {
        x: pos.0,
        y: pos.1,
        width: logical(size.0).max(MIN_LOGICAL.0),
        height: logical(size.1).max(MIN_LOGICAL.1),
    }
}

/// Stores `rect` for monitor `key`; at most `MAX_REMEMBERED_MONITORS` monitors are kept (the
/// first key in name order goes when a new one does not fit).
pub fn remember(map: &mut BTreeMap<String, WindowRect>, key: String, rect: WindowRect) {
    while !map.contains_key(&key) && map.len() >= MAX_REMEMBERED_MONITORS {
        let Some(first) = map.keys().next().cloned() else {
            break;
        };
        map.remove(&first);
    }
    map.insert(key, rect);
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

    fn saved(x: i32, y: i32, w: u32, h: u32) -> WindowRect {
        WindowRect {
            x,
            y,
            width: w,
            height: h,
        }
    }

    #[test]
    fn a_monitor_is_named_by_its_geometry() {
        let full = Rect {
            left: -2560,
            top: 0,
            right: 0,
            bottom: 1440,
        };
        assert_eq!(monitor_key(full), "-2560,0,2560x1440");
        assert_ne!(monitor_key(full), monitor_key(Rect { left: 0, ..full }));
    }

    #[test]
    fn the_first_open_is_centred_at_the_default_size() {
        let p = initial(None, WORK, 96);
        assert_eq!((p.width, p.height, p.x, p.y), (1060, 660, 430, 190), "{p:?}");
        assert!(p.width >= 1000, "the default is the wide layout");
    }

    #[test]
    fn the_default_size_scales_with_the_dpi() {
        let p = initial(None, WORK, 144);
        assert_eq!((p.width, p.height), (1590, 990));
    }

    #[test]
    fn a_remembered_rectangle_comes_back() {
        let p = initial(Some(saved(100, 80, 1000, 700)), WORK, 96);
        assert_eq!(
            p,
            Placed {
                x: 100,
                y: 80,
                width: 1000,
                height: 700
            }
        );
    }

    #[test]
    fn a_remembered_rectangle_off_the_screen_is_pulled_back() {
        // The monitor it was on is gone or smaller now.
        let p = initial(Some(saved(5000, 4000, 1000, 700)), WORK, 96);
        assert_eq!((p.x, p.y), (920, 340));
        let p = initial(Some(saved(-3000, -500, 1000, 700)), WORK, 96);
        assert_eq!((p.x, p.y), (0, 0));
    }

    #[test]
    fn the_size_is_limited_by_the_work_area_and_by_the_minimum() {
        let small = Rect {
            left: 0,
            top: 0,
            right: 800,
            bottom: 600,
        };
        let p = initial(Some(saved(0, 0, 2000, 2000)), small, 96);
        assert_eq!((p.width, p.height), (800, 600));
        // A tiny saved size grows to the minimum.
        let p = initial(Some(saved(0, 0, 100, 100)), WORK, 96);
        assert_eq!((p.width, p.height), (640, 460));
    }

    #[test]
    fn a_saved_size_below_the_minimum_is_clamped_up_and_larger_ones_are_kept() {
        // F-34: 500 x 300 was allowed once; now it opens at the 640 x 460 minimum.
        let p = initial(Some(saved(10, 10, 500, 300)), WORK, 96);
        assert_eq!((p.width, p.height), (640, 460));
        // Only the side below its minimum grows.
        let p = initial(Some(saved(10, 10, 500, 700)), WORK, 96);
        assert_eq!((p.width, p.height), (640, 700));
        // The minimum itself and anything above it stays as saved.
        let p = initial(Some(saved(10, 10, 640, 460)), WORK, 96);
        assert_eq!((p.width, p.height), (640, 460));
        let p = initial(Some(saved(10, 10, 820, 500)), WORK, 96);
        assert_eq!((p.width, p.height), (820, 500));
        // The minimum scales with the DPI.
        let p = initial(Some(saved(10, 10, 500, 300)), WORK, 144);
        assert_eq!((p.width, p.height), (960, 690));
        // The work area still wins over the minimum on a small screen.
        let tiny = Rect {
            left: 0,
            top: 0,
            right: 600,
            bottom: 400,
        };
        let p = initial(Some(saved(0, 0, 500, 300)), tiny, 96);
        assert_eq!((p.width, p.height), (600, 400));
    }

    #[test]
    fn a_secondary_monitor_with_negative_coordinates_works() {
        let left = Rect {
            left: -2880,
            top: 0,
            right: 0,
            bottom: 1560,
        };
        let p = initial(Some(saved(-500, 100, 980, 640)), left, 144);
        assert_eq!((p.width, p.height), (1470, 960));
        // Too far right for the scaled size: clamped to the right edge.
        assert_eq!(p.x, -1470);
        assert_eq!(p.y, 100);
    }

    #[test]
    fn saving_converts_the_size_back_to_logical_pixels() {
        assert_eq!(
            to_saved((10, 20), (1470, 960), 144),
            saved(10, 20, 980, 640)
        );
        // A DPI of 0 counts as 100 %; a window below the minimum is saved as the minimum.
        assert_eq!(to_saved((0, 0), (1000, 700), 0), saved(0, 0, 1000, 700));
        assert_eq!(to_saved((0, 0), (10, 10), 96), saved(0, 0, 640, 460));
        assert_eq!(to_saved((0, 0), (500, 300), 96), saved(0, 0, 640, 460));
        assert_eq!(to_saved((0, 0), (700, 500), 96), saved(0, 0, 700, 500));
    }

    #[test]
    fn at_most_sixteen_monitors_are_remembered() {
        let mut m = BTreeMap::new();
        for i in 0..20 {
            remember(&mut m, format!("m{i:02}"), saved(i, 0, 800, 600));
        }
        assert_eq!(m.len(), MAX_REMEMBERED_MONITORS);
        // Updating a known monitor never evicts another one.
        remember(&mut m, "m19".into(), saved(99, 0, 900, 700));
        assert_eq!(m.len(), MAX_REMEMBERED_MONITORS);
        assert_eq!(m["m19"].x, 99);
    }
}
