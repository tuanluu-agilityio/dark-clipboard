//! Responsive layout of the history window (Task 704). Pure: the width decides which panes show,
//! how wide they are and which regions F6 walks. Widths are logical pixels. The Slint markup
//! (`ui/history.slint`) only draws what these numbers say.

/// Window width from which the full sidebar and the 340 px detail panel both fit.
pub const WIDE_MIN: u32 = 1000;
/// Window width from which the detail panel still sits beside the list (as a 300 px column).
pub const MEDIUM_MIN: u32 = 820;
/// The smallest window (`place::MIN_LOGICAL`).
pub const NARROW_MIN: u32 = 640;

pub const SIDEBAR_FULL: u32 = 216;
pub const RAIL: u32 = 52;
pub const DETAIL_WIDE: u32 = 340;
pub const DETAIL_MEDIUM: u32 = 300;
/// The drawer of the narrow layout is as wide as the wide detail panel.
pub const DRAWER: u32 = 340;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LayoutMode {
    /// 1000 and wider: sidebar 216, detail 340.
    Wide,
    /// 820 to 999: sidebar as a 52 px rail, detail 300.
    Medium,
    /// 640 to 819: rail, detail as a drawer over the list.
    Narrow,
}

/// The layout for a window `width` logical pixels wide.
pub fn layout_mode(width: u32) -> LayoutMode {
    if width >= WIDE_MIN {
        LayoutMode::Wide
    } else if width >= MEDIUM_MIN {
        LayoutMode::Medium
    } else {
        LayoutMode::Narrow
    }
}

/// What the markup needs to place the panes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Geometry {
    pub mode: LayoutMode,
    /// The sidebar is drawn as a rail (always below 1000, or when the user collapsed it).
    pub rail: bool,
    pub sidebar_w: u32,
    pub detail_w: u32,
    /// The detail panel is a drawer over the list (it takes no room beside it).
    pub drawer: bool,
}

/// The geometry for `width`; `rail_forced` is the user's Ctrl+B collapse at wide widths.
pub fn geometry(width: u32, rail_forced: bool) -> Geometry {
    let mode = layout_mode(width);
    let rail = rail_forced || mode != LayoutMode::Wide;
    Geometry {
        mode,
        rail,
        sidebar_w: if rail { RAIL } else { SIDEBAR_FULL },
        detail_w: match mode {
            LayoutMode::Wide => DETAIL_WIDE,
            LayoutMode::Medium => DETAIL_MEDIUM,
            LayoutMode::Narrow => DRAWER,
        },
        drawer: mode == LayoutMode::Narrow,
    }
}

/// A part of the window F6 can land on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Region {
    /// The sidebar or the rail.
    Sidebar,
    List,
    /// The detail panel, or the drawer while it is open.
    Detail,
}

/// The visible regions in F6 order.
pub fn regions(mode: LayoutMode, drawer_open: bool) -> Vec<Region> {
    let mut v = vec![Region::Sidebar, Region::List];
    if mode != LayoutMode::Narrow || drawer_open {
        v.push(Region::Detail);
    }
    v
}

/// The region after (or, with `back`, before) `from`, wrapping. A region that is not visible
/// (the drawer closed under the keyboard focus) counts as the list.
pub fn next_region(mode: LayoutMode, drawer_open: bool, from: Region, back: bool) -> Region {
    let all = regions(mode, drawer_open);
    let at = all.iter().position(|r| *r == from).unwrap_or(1);
    let n = all.len();
    all[if back { (at + n - 1) % n } else { (at + 1) % n }]
}

/// The region a reported focus id belongs to (`ChipFocus` ids: -50 sidebar, -51 rail, -52 the
/// overlay copy of the sidebar, -110 and -120 to -122 the detail panel, -140 the drawer's close
/// button). Anything else, including no id, is the list.
pub fn region_of_focus(id: Option<i32>) -> Region {
    match id {
        Some(-52..=-50) => Region::Sidebar,
        Some(-122..=-120 | -110 | -140) => Region::Detail,
        _ => Region::List,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_layout_mode_changes_at_640_820_and_1000() {
        for (w, m) in [
            (639, LayoutMode::Narrow),
            (640, LayoutMode::Narrow),
            (819, LayoutMode::Narrow),
            (820, LayoutMode::Medium),
            (999, LayoutMode::Medium),
            (1000, LayoutMode::Wide),
            (1060, LayoutMode::Wide),
        ] {
            assert_eq!(layout_mode(w), m, "{w}");
        }
    }

    #[test]
    fn each_mode_has_its_pane_widths() {
        let g = geometry(1060, false);
        assert_eq!((g.sidebar_w, g.detail_w, g.rail, g.drawer), (216, 340, false, false));
        let g = geometry(900, false);
        assert_eq!((g.sidebar_w, g.detail_w, g.rail, g.drawer), (52, 300, true, false));
        let g = geometry(700, false);
        assert_eq!((g.sidebar_w, g.detail_w, g.rail, g.drawer), (52, 340, true, true));
        // Ctrl+B at a wide width collapses the sidebar to the rail and nothing else.
        let g = geometry(1060, true);
        assert_eq!((g.sidebar_w, g.detail_w, g.rail, g.drawer), (52, 340, true, false));
    }

    #[test]
    fn the_list_never_gets_narrower_than_240_px() {
        for w in [640, 700, 819, 820, 900, 999, 1000, 1060, 1920] {
            for forced in [false, true] {
                let g = geometry(w, forced);
                let beside = if g.drawer { 0 } else { g.detail_w };
                assert!(w - g.sidebar_w - beside >= 240, "{w} {forced}");
            }
        }
    }

    #[test]
    fn f6_walks_three_regions_when_the_detail_is_beside_the_list() {
        for mode in [LayoutMode::Wide, LayoutMode::Medium] {
            assert_eq!(
                regions(mode, false),
                [Region::Sidebar, Region::List, Region::Detail]
            );
            assert_eq!(next_region(mode, false, Region::Sidebar, false), Region::List);
            assert_eq!(next_region(mode, false, Region::List, false), Region::Detail);
            assert_eq!(next_region(mode, false, Region::Detail, false), Region::Sidebar);
            assert_eq!(next_region(mode, false, Region::Sidebar, true), Region::Detail);
            assert_eq!(next_region(mode, false, Region::Detail, true), Region::List);
        }
    }

    #[test]
    fn f6_at_narrow_widths_visits_the_drawer_only_while_it_is_open() {
        assert_eq!(
            regions(LayoutMode::Narrow, false),
            [Region::Sidebar, Region::List]
        );
        assert_eq!(
            regions(LayoutMode::Narrow, true),
            [Region::Sidebar, Region::List, Region::Detail]
        );
        let n = LayoutMode::Narrow;
        assert_eq!(next_region(n, false, Region::List, false), Region::Sidebar);
        assert_eq!(next_region(n, false, Region::Sidebar, true), Region::List);
        assert_eq!(next_region(n, true, Region::List, false), Region::Detail);
        // The drawer closed under the focus: the cycle goes on from the list.
        assert_eq!(next_region(n, false, Region::Detail, false), Region::Sidebar);
    }

    #[test]
    fn focus_ids_belong_to_a_region() {
        assert_eq!(region_of_focus(None), Region::List);
        assert_eq!(region_of_focus(Some(-50)), Region::Sidebar);
        assert_eq!(region_of_focus(Some(-51)), Region::Sidebar);
        assert_eq!(region_of_focus(Some(-110)), Region::Detail);
        assert_eq!(region_of_focus(Some(-121)), Region::Detail);
        assert_eq!(region_of_focus(Some(-102)), Region::List);
    }
}
