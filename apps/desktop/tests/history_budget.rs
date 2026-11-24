//! Width budgets of the history window (Task 703, acceptance A7), in the style of
//! `popup_budget.rs`: a measured character budget, not a layout run (no window is created,
//! GC-19), in English and Vietnamese, and nothing is loosened to make a test pass.
//!
//! The wide layout at its smallest useful width is 1000 px: a 216 px sidebar, a 340 px detail
//! panel and a list column of 1000 - 216 - 340 = 444 px (`ui/history.slint`). Labels are 11 to
//! 15 px system UI text; 7 px per character is a deliberately pessimistic average (Latin text
//! measures about 6.2 px, Vietnamese with stacked diacritics about 6.5 px). A key cap is 6 px per
//! character plus 8 px; 11 px counts in the sidebar use the same 6 px.

use darkclip_core::i18n::{t, Lang, Msg};
use std::fs;
use std::path::PathBuf;

const WINDOW_PX: f32 = 1000.0;
const SIDEBAR_PX: f32 = 216.0;
const DETAIL_PX: f32 = 340.0;
const LIST_PX: f32 = WINDOW_PX - SIDEBAR_PX - DETAIL_PX;
const PX_PER_CHAR: f32 = 7.0;
const CAP_PX_PER_CHAR: f32 = 6.0;
const CAP_PADDING_PX: f32 = 8.0;
const BUTTON_PX: f32 = 28.0;
const GAP_PX: f32 = 4.0;

fn text_px(s: &str) -> f32 {
    s.chars().count() as f32 * PX_PER_CHAR
}

fn cap_px(s: &str) -> f32 {
    s.chars().count() as f32 * CAP_PX_PER_CHAR + CAP_PADDING_PX
}

fn manifest() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

#[test]
fn the_list_column_is_444_px_at_1000() {
    assert_eq!(LIST_PX, 444.0);
    // The layout in the markup uses the same numbers: Rust (`src/history/layout.rs`) hands the
    // pane widths over, and the defaults are the wide layout.
    let slint = fs::read_to_string(manifest().join("ui/history.slint")).unwrap_or_default();
    assert!(
        slint.contains("parent.width - root.sidebar-w - (root.drawer-mode ? 0px : root.detail-w)"),
        "list width"
    );
    assert!(
        slint.contains("in property <length> detail-w: 340px;"),
        "detail width"
    );
    assert!(
        slint.contains("in property <length> sidebar-w: 216px;"),
        "sidebar width"
    );
    assert!(slint.contains("height: 48px;"), "toolbar height");
    assert!(
        slint.contains("min-width: 640px;"),
        "the window minimum is 640 px (F-34)"
    );
    let layout = layout_src();
    assert_eq!(layout_const(&layout, "SIDEBAR_FULL"), SIDEBAR_PX as u32);
    assert_eq!(layout_const(&layout, "DETAIL_WIDE"), DETAIL_PX as u32);
    let row = fs::read_to_string(manifest().join("ui/history_row.slint")).unwrap_or_default();
    assert!(
        row.contains("out property <length> height: 56px;"),
        "row height"
    );
}

fn layout_src() -> String {
    fs::read_to_string(manifest().join("src/history/layout.rs")).unwrap_or_default()
}

/// `pub const NAME: u32 = 123;` of `layout.rs`.
fn layout_const(src: &str, name: &str) -> u32 {
    let key = format!("pub const {name}: u32 = ");
    src.lines()
        .find_map(|l| l.strip_prefix(&key))
        .and_then(|v| v.trim_end_matches(';').trim().parse().ok())
        .unwrap_or(0)
}

#[test]
fn the_toolbar_fits_in_both_languages() {
    // 12 px padding each side.
    let inner = LIST_PX - 24.0;
    // The title keeps at least 48 px (it elides), the search field at least 140 px
    // (`min-width` in the markup), then the date button and the select toggle, with 8 px gaps.
    const TITLE_MIN: f32 = 48.0;
    const SEARCH_MIN: f32 = 140.0;
    for lang in [Lang::En, Lang::Vi] {
        let widest_date = [
            Msg::UiFilterAnyTime,
            Msg::UiFilterToday,
            Msg::UiFilterLast7Days,
            Msg::UiFilterLast30Days,
        ]
        .iter()
        .map(|m| text_px(t(lang, *m)))
        .fold(0.0_f32, f32::max);
        // 8 padding, 16 icon, 6 gap, label, 6 gap, 16 chevron, 8 padding.
        let date_button = 8.0 + 16.0 + 6.0 + widest_date + 6.0 + 16.0 + 8.0;
        let needed = TITLE_MIN + SEARCH_MIN + date_button + 2.0 * BUTTON_PX + 3.0 * 8.0;
        assert!(
            needed <= inner,
            "{lang:?} toolbar needs {needed} px but has {inner} px"
        );
    }
}

#[test]
fn the_widest_sidebar_label_and_count_fit() {
    // Border 1, padding 8 + 8, row padding 12 + 8, icon 16, two 8 px gaps.
    let room = SIDEBAR_PX - 1.0 - 16.0 - 12.0 - 8.0 - 16.0 - 16.0;
    // A four-digit count in 11 px text.
    let count = 4.0 * CAP_PX_PER_CHAR;
    let labels = [
        Msg::UiGroupLibrary,
        Msg::UiFilterAll,
        Msg::UiFilterPinned,
        Msg::OrgFavorites,
        Msg::UiGroupType,
        Msg::ClipTypeText,
        Msg::ClipTypeRichText,
        Msg::ClipTypeUrl,
        Msg::ClipTypeCode,
        Msg::ClipTypeImage,
        Msg::ClipTypeFiles,
        Msg::ClipTypeColor,
        Msg::OrgGroupCollections,
        Msg::OrgGroupTags,
        Msg::UiFilterApps,
    ];
    for lang in [Lang::En, Lang::Vi] {
        for m in labels {
            let need = text_px(t(lang, m)) + count;
            assert!(
                need <= room,
                "{lang:?} {m:?} with a count needs {need} px but has {room} px"
            );
        }
    }
}

#[test]
fn the_detail_toolbar_fits_the_panel() {
    // 12 px padding each side; seven 28 px buttons, six 4 px gaps and the 9 px divider.
    let inner = DETAIL_PX - 24.0;
    let needed = 7.0 * BUTTON_PX + 6.0 * GAP_PX + 9.0;
    assert!(needed <= inner, "toolbar needs {needed} px, has {inner} px");
    // Every button is at least 28 px (A6).
    let panes = fs::read_to_string(manifest().join("ui/history_panes.slint")).unwrap_or_default();
    assert!(panes.contains("width: 28px;") && panes.contains("height: 28px;"));
}

#[test]
fn the_fact_labels_fit_their_column() {
    // The facts' name column is 64 px wide.
    for lang in [Lang::En, Lang::Vi] {
        for m in [Msg::UiGroupType, Msg::UiFactFrom, Msg::UiFactCopied] {
            let px = text_px(t(lang, m));
            assert!(px <= 64.0, "{lang:?} {m:?} needs {px} px of 64");
        }
    }
}

#[test]
fn the_status_line_fits_the_count_and_two_hints() {
    // 12 px padding each side, 8 px between the count and each hint.
    let inner = LIST_PX - 24.0;
    for lang in [Lang::En, Lang::Vi] {
        // The longest count: "9999+ clips".
        let count = t(lang, Msg::UiClipsMany).replace("{n}", "9999+");
        let hints = [
            (Msg::UiKeySpace, Msg::UiHintMark),
            (Msg::UiKeyMenu, Msg::UiHintActions),
        ];
        let hint_px: f32 = hints
            .iter()
            .map(|(k, l)| cap_px(t(lang, *k)) + 4.0 + text_px(t(lang, *l)) + 8.0)
            .sum();
        let needed = text_px(&count) + hint_px;
        assert!(
            needed <= inner,
            "{lang:?} status line needs {needed} px but has {inner} px"
        );
    }
}

#[test]
fn the_date_menu_fits_its_width() {
    // Kit default width 220: border 2, list padding 8, item padding 16, icon 16 + gap 8 and the
    // check mark 16 + gap 8 around the label.
    let room = 220.0 - 2.0 - 8.0 - 16.0 - 16.0 - 8.0 - 16.0 - 8.0;
    for lang in [Lang::En, Lang::Vi] {
        for m in [
            Msg::UiFilterAnyTime,
            Msg::UiFilterToday,
            Msg::UiFilterLast7Days,
            Msg::UiFilterLast30Days,
        ] {
            let px = text_px(t(lang, m));
            assert!(px <= room, "{lang:?} {m:?} needs {px} px of {room}");
        }
    }
}

#[test]
fn the_history_window_has_no_text_footer_and_no_literal_strings() {
    let slint = fs::read_to_string(manifest().join("ui/history.slint")).unwrap_or_default();
    let panes = fs::read_to_string(manifest().join("ui/history_panes.slint")).unwrap_or_default();
    let row = fs::read_to_string(manifest().join("ui/history_row.slint")).unwrap_or_default();
    for (name, src) in [("history", &slint), ("panes", &panes), ("row", &row)] {
        for (i, line) in src.lines().enumerate() {
            let code = line.split("//").next().unwrap_or_default();
            // A `text:` or `label:` binding must be an expression, never a quoted literal
            // (strings come from i18n; ": " joins an accessible name from two i18n strings).
            for key in ["text: \"", "label: \"", "placeholder-text: \""] {
                assert!(
                    !code.contains(key) || code.contains("text: \"\""),
                    "{name}.slint:{}: literal user-visible string: {line}",
                    i + 1
                );
            }
        }
    }
    // The one-line `Alt+N` number and the kind word are not on a history row.
    assert!(
        !row.contains("data.number"),
        "the history row has no Alt+N number"
    );
    // The long placeholder elides in the search field instead of pushing the toolbar wider.
    assert!(
        slint.contains("text: Strings.search-placeholder;") && slint.contains("overflow: elide;"),
        "the placeholder must elide"
    );
}

// ---- Task 704: the narrow layouts, the bulk bar, the rail and the row menu ----

/// The list column at a window width (the drawer of the narrow layout floats over it).
fn list_px(width: u32, rail: bool, detail: u32, drawer: bool) -> f32 {
    let side = if rail { 52.0 } else { 216.0 };
    width as f32 - side - if drawer { 0.0 } else { detail as f32 }
}

#[test]
fn the_pane_widths_and_breakpoints_match_the_markup_and_the_spec() {
    let l = layout_src();
    assert_eq!(layout_const(&l, "WIDE_MIN"), 1000);
    assert_eq!(layout_const(&l, "MEDIUM_MIN"), 820);
    assert_eq!(layout_const(&l, "RAIL"), 52);
    assert_eq!(layout_const(&l, "DETAIL_MEDIUM"), 300);
    assert_eq!(layout_const(&l, "DRAWER"), 340);
    // The smallest window still gives the list room for the toolbar (title 48, search 140).
    assert!(list_px(640, true, 340, true) >= 240.0);
    assert!(list_px(820, true, 300, false) >= 240.0);
    assert!(list_px(1000, false, 340, false) >= 240.0);
}

#[test]
fn the_bulk_bar_fits_at_640_and_1000_px_in_both_languages() {
    // The bar is 8 px in from the list column on each side, 1 px border, 12 px left padding,
    // 8 px right padding; "N selected" then seven 28 px buttons with 4 px gaps.
    for (name, list) in [
        ("1000", list_px(1000, false, 340, false)),
        ("640", list_px(640, true, 340, true)),
    ] {
        let inner = list - 16.0 - 2.0 - 12.0 - 8.0;
        let buttons = 7.0 * BUTTON_PX + 7.0 * 4.0;
        for lang in [Lang::En, Lang::Vi] {
            // Four digits: the longest count a person marks by hand or with Ctrl+A.
            let text = t(lang, Msg::HistSelectedCount).replace("{n}", "9999");
            let need = text_px(&text);
            assert!(
                need + buttons <= inner,
                "{lang:?} bulk bar at {name} px needs {} px but has {inner} px",
                need + buttons
            );
        }
    }
    // The bar is 36 px high with a 28 px button inside.
    let panes = fs::read_to_string(manifest().join("ui/history_panes.slint")).unwrap_or_default();
    assert!(panes.contains("height: 36px;"));
    assert!(panes.contains("border-color: Theme.accent;"));
}

#[test]
fn the_bulk_bar_has_the_seven_controls_as_icon_buttons_with_labels() {
    let panes = fs::read_to_string(manifest().join("ui/history_panes.slint")).unwrap_or_default();
    let bar = &panes[panes.find("export component BulkBar").unwrap_or(0)..];
    assert_eq!(bar.matches("BulkBtn {").count(), 7, "seven controls");
    for cb in [
        "act-pin",
        "act-favorite",
        "act-add-to-collection",
        "act-tag",
        "act-export",
        "act-delete",
        "act-clear",
    ] {
        assert!(bar.contains(&format!("root.{cb}();")), "{cb}");
    }
    // Every control has a translated name (no literal).
    for label in [
        "Strings.pin",
        "Strings.hist-favorite",
        "Strings.hist-add-to-collection",
        "Strings.hist-bulk-tag",
        "Strings.hist-export",
        "Strings.delete",
        "Strings.clear-selection",
    ] {
        assert!(bar.contains(&format!("label: {label};")), "{label}");
    }
    for lang in [Lang::En, Lang::Vi] {
        for m in [
            Msg::HistBulkTag,
            Msg::HistMenuAddToCollection,
            Msg::HistMenuExport,
            Msg::HistSelectedCount,
        ] {
            assert!(!t(lang, m).is_empty());
        }
    }
}

#[test]
fn rail_tooltips_fit_inside_the_narrowest_window() {
    // The tooltip bubble is 8 px padding each side and sits beside the 52 px rail; the longest
    // text is a label with a four-digit count in brackets, "Label (9999)".
    let room = 640.0 - 52.0 - 8.0 - 16.0;
    let labels = [
        Msg::UiGroupLibrary,
        Msg::UiFilterAll,
        Msg::UiFilterPinned,
        Msg::OrgFavorites,
        Msg::ClipTypeText,
        Msg::ClipTypeRichText,
        Msg::ClipTypeUrl,
        Msg::ClipTypeCode,
        Msg::ClipTypeImage,
        Msg::ClipTypeFiles,
        Msg::ClipTypeColor,
        Msg::OrgGroupCollections,
        Msg::OrgGroupTags,
        Msg::UiFilterApps,
    ];
    for lang in [Lang::En, Lang::Vi] {
        for m in labels {
            let px = text_px(&format!("{} (9999)", t(lang, m)));
            assert!(px <= room, "{lang:?} {m:?} tooltip needs {px} of {room} px");
        }
    }
    // The rail is 52 px with 36 px rows and carries the name as its accessible label.
    let panes = fs::read_to_string(manifest().join("ui/history_panes.slint")).unwrap_or_default();
    assert!(panes.contains("root.compact ? 36px : 28px"));
    assert!(panes.contains("accessible-label: root.compact ? root.item.tip : root.item.label;"));
    assert!(panes.contains("text: root.item.tip;"));
}

#[test]
fn the_row_menu_is_230_px_and_its_texts_are_not_literals() {
    let slint = fs::read_to_string(manifest().join("ui/history.slint")).unwrap_or_default();
    assert!(
        slint.contains("menu-width: 230px;"),
        "the row menu is 230 px wide"
    );
    assert!(
        !slint.contains("PopupWindow"),
        "the row menu is the kit Menu, not a mouse-only PopupWindow"
    );
    assert!(!slint.contains("CtxEntry"));
    // The key walk is in Rust: the markup only forwards the key text.
    assert!(slint.contains("return root.row-menu-key(text);"));
}

#[test]
fn the_new_texts_exist_in_both_languages_and_are_short() {
    for lang in [Lang::En, Lang::Vi] {
        for m in [
            Msg::HistDrawerOpen,
            Msg::HistDrawerClose,
            Msg::HistCheckLabel,
            Msg::HistMenuCopyPlain,
            Msg::HistMenuRemoveFromCollection,
        ] {
            let s = t(lang, m);
            assert!(!s.is_empty());
            // Tooltips and accessible names, 26 characters at most.
            assert!(s.chars().count() <= 26, "{lang:?} {m:?} is {s}");
        }
    }
}

// ---- Task 706: empty states ----

#[test]
fn the_history_empty_states_fit_on_one_line_each_in_the_narrowest_list() {
    // The narrowest list column is the medium layout's: 820 - 216 - 300 = 304 px; the
    // `EmptyState` keeps 20 px of padding on each side (`ui/kit/empty_state.slint`).
    let room = list_px(820, false, 300, false) - 40.0;
    assert_eq!(room, 264.0);
    for lang in [Lang::En, Lang::Vi] {
        for m in [
            Msg::UiEmptyHistory,
            Msg::UiNoResults,
            Msg::UiLoadFailed,
            Msg::StateHintEmpty,
            Msg::StateHintNoResults,
            Msg::StateHintFailed,
        ] {
            let px = text_px(t(lang, m));
            assert!(px <= room, "{lang:?} {m:?} needs {px} px of {room}");
        }
    }
}

#[test]
fn the_status_line_names_the_sheet_in_a_tooltip_not_a_hint() {
    // D-084: at the medium layout (304 px list) the count and the two hints leave no room for
    // an F1 hint, so the line carries a tooltip. The count, two hints and an F1 hint:
    let inner = list_px(820, false, 300, false) - 24.0;
    for lang in [Lang::En, Lang::Vi] {
        let count = t(lang, Msg::UiClipsMany).replace("{n}", "9999+");
        let hints = [
            (Msg::UiKeySpace, Msg::UiHintMark),
            (Msg::UiKeyMenu, Msg::UiHintActions),
            (Msg::UiKeyEsc, Msg::SheetShortcuts),
        ];
        let hint_px: f32 = hints
            .iter()
            .map(|(k, l)| cap_px(t(lang, *k)) + 4.0 + text_px(t(lang, *l)) + 8.0)
            .sum();
        assert!(
            text_px(&count) + hint_px > inner,
            "{lang:?}: an F1 hint would fit, so D-084 must be revisited"
        );
    }
    let panes = fs::read_to_string(manifest().join("ui/history_panes.slint")).unwrap_or_default();
    assert!(
        panes.contains("tip-key"),
        "the history status line has the tooltip"
    );
}
