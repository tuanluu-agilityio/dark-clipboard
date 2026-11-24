//! Width budgets of the popup (Task 701, acceptance A7). They replace `footer_budget.rs`: the
//! popup has no text footer any more (its five buttons became row actions, the row menu and keys),
//! so the budget now covers what the new layout puts on one line: the header, the filter segment,
//! the status line and the two menus, in English and Vietnamese. No window is created (GC-19);
//! this is a measured character budget, not a layout run, and nothing was loosened.
//!
//! Budget: the popup is 440 px wide (`ui/app.slint`), minus a 1 px border on each side and 12 px
//! of padding on each side = 414 px of inner width. Labels are 12 to 14 px system UI text; 7 px
//! per character is a deliberately pessimistic average (Latin text measures about 6.2 px,
//! Vietnamese with stacked diacritics about 6.5 px). A key cap is 6 px per character plus 8 px.

use darkclip_core::i18n::{t, Lang, Msg};
use std::fs;
use std::path::PathBuf;

const WINDOW_PX: f32 = 440.0;
const INNER_PX: f32 = WINDOW_PX - 2.0 - 24.0;
const PX_PER_CHAR: f32 = 7.0;
const CAP_PX_PER_CHAR: f32 = 6.0;
const CAP_PADDING_PX: f32 = 8.0;
const BUTTON_PX: f32 = 28.0;
const GAP_PX: f32 = 4.0;
/// The widest the row menu is drawn (`ROW_MENU_WIDTH` in `src/ui/popup_host.rs`).
const ROW_MENU_WIDTH_PX: f32 = 340.0;
/// The More filters menu keeps the kit's default width.
const MORE_MENU_WIDTH_PX: f32 = 220.0;
/// Menu border (2), list padding (8) and item padding (16) around an item's content.
const MENU_CHROME_PX: f32 = 2.0 + 8.0 + 16.0;

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
fn the_header_fits_in_both_languages() {
    for lang in [Lang::En, Lang::Vi] {
        // Search field: 1 px border, 8 px padding each side, icon 16 + 8, placeholder, 8 + key cap.
        let search = 2.0
            + 16.0
            + 16.0
            + 8.0
            + text_px(t(lang, Msg::UiPopupSearchHint))
            + 8.0
            + cap_px(t(lang, Msg::UiKeyCtrlK));
        // Four 28 px buttons, four 4 px gaps (one before each button).
        let header = search + 4.0 * BUTTON_PX + 4.0 * GAP_PX;
        assert!(
            header <= INNER_PX,
            "{lang:?} header needs {header} px but has {INNER_PX} px"
        );
    }
}

fn tab_labels(lang: Lang) -> Vec<&'static str> {
    vec![
        t(lang, Msg::UiFilterAll),
        t(lang, Msg::ClipTypeText),
        t(lang, Msg::ClipTypeUrl),
        t(lang, Msg::ClipTypeCode),
        t(lang, Msg::ClipTypeImage),
        t(lang, Msg::ClipTypeFiles),
        t(lang, Msg::UiFilterPinned),
        t(lang, Msg::OrgFavorites),
    ]
}

#[test]
fn the_segment_leaves_room_for_an_extra_filter_chip() {
    // The smallest a chip shrinks to (`ChipView.min-width`): an x icon and a few letters.
    const CHIP_MIN_PX: f32 = 56.0;
    for lang in [Lang::En, Lang::Vi] {
        let labels = tab_labels(lang);
        let widest = labels.iter().map(|l| text_px(l)).fold(0.0_f32, f32::max);
        // One 28 px tab per label (eight: All, five kinds, Pinned, Favorites), the widest tab
        // name once (only the first active tab shows its name) with a 4 px gap, the 9 px
        // divider, 1 px padding each side, the gap and the 28 px More filters button, then a
        // gap before the extras.
        let segment = labels.len() as f32 * BUTTON_PX
            + 4.0
            + widest
            + 9.0
            + 2.0
            + GAP_PX
            + BUTTON_PX
            + GAP_PX;
        assert!(
            segment + CHIP_MIN_PX <= INNER_PX,
            "{lang:?} segment needs {segment} px, leaving {} px for the extras",
            INNER_PX - segment
        );
    }
}

/// The status line's hints for the three cases of `paste::status_hints`: (key, label).
fn hint_sets() -> Vec<Vec<(Msg, Msg)>> {
    vec![
        // Paste works, "Enter pastes" off.
        vec![
            (Msg::UiKeyEnter, Msg::UiCopy),
            (Msg::UiKeyCtrlEnter, Msg::UiHintPaste),
            (Msg::UiKeySpace, Msg::UiHintPreview),
        ],
        // "Enter pastes" on.
        vec![
            (Msg::UiKeyEnter, Msg::UiHintPaste),
            (Msg::UiKeySpace, Msg::UiHintPreview),
        ],
        // Paste unavailable.
        vec![
            (Msg::UiKeyEnter, Msg::UiCopy),
            (Msg::UiKeySpace, Msg::UiHintPreview),
        ],
    ]
}

#[test]
fn the_status_line_fits_its_longest_state_and_three_hints() {
    let states = [
        Msg::UiStatusCapturing,
        Msg::UiStatusPaused,
        Msg::UiStatusPrivate,
        Msg::UiStatusOff,
    ];
    for lang in [Lang::En, Lang::Vi] {
        let longest_state = states
            .iter()
            .map(|m| text_px(t(lang, *m)))
            .fold(0.0_f32, f32::max);
        for hints in hint_sets() {
            assert!(hints.len() <= 3);
            let hint_px: f32 = hints
                .iter()
                .map(|(k, l)| cap_px(t(lang, *k)) + GAP_PX + text_px(t(lang, *l)))
                .sum::<f32>()
                + (hints.len() as f32 - 1.0) * 8.0;
            // Dot 8 + gap 8, the state, gap 8, the hints.
            let line = 8.0 + 8.0 + longest_state + 8.0 + hint_px;
            assert!(
                line <= INNER_PX,
                "{lang:?} status line needs {line} px but has {INNER_PX} px"
            );
        }
    }
}

/// The widest content of a menu item: icon 16 + gap 8, the label, gap 8 and the key cap.
fn item_px(label: &str, key: Option<&str>) -> f32 {
    16.0 + 8.0 + text_px(label) + key.map_or(0.0, |k| 8.0 + cap_px(k))
}

#[test]
fn the_row_menu_fits_its_width_in_both_languages() {
    for lang in [Lang::En, Lang::Vi] {
        for pinned in [false, true] {
            for fav in [false, true] {
                let items = [
                    (Msg::UiCopy, Msg::UiKeyEnter),
                    (Msg::UiCopyPlain, Msg::UiKeyShiftEnter),
                    (Msg::TfCopyAs, Msg::UiKeyCtrlT),
                    (
                        if pinned { Msg::UiUnpin } else { Msg::UiPin },
                        Msg::UiKeyCtrlP,
                    ),
                    (
                        if fav {
                            Msg::UiMenuUnfavorite
                        } else {
                            Msg::UiMenuFavorite
                        },
                        Msg::UiKeyCtrlD,
                    ),
                    (Msg::UiDelete, Msg::UiKeyDel),
                ];
                for (label, key) in items {
                    let px = item_px(t(lang, label), Some(t(lang, key))) + MENU_CHROME_PX;
                    assert!(
                        px <= ROW_MENU_WIDTH_PX,
                        "{lang:?} row menu item {label:?} needs {px} px of {ROW_MENU_WIDTH_PX}"
                    );
                }
            }
        }
    }
}

#[test]
fn the_more_filters_menu_fits_its_width_in_both_languages() {
    for lang in [Lang::En, Lang::Vi] {
        for label in [
            Msg::UiFilterToday,
            Msg::UiFilterLast7Days,
            Msg::ClipTypeRichText,
            Msg::ClipTypeColor,
            Msg::OrgGroupCollections,
            Msg::UiFilterApps,
        ] {
            // A check or a chevron may sit at the end: 8 + 16.
            let px = item_px(t(lang, label), None) + 8.0 + 16.0 + MENU_CHROME_PX;
            assert!(
                px <= MORE_MENU_WIDTH_PX,
                "{lang:?} More filters item {label:?} needs {px} px of {MORE_MENU_WIDTH_PX}"
            );
        }
    }
}

#[test]
fn the_sources_of_the_budget_match_the_code() {
    let host = fs::read_to_string(manifest().join("src/ui/popup_host.rs")).unwrap_or_default();
    assert!(
        host.contains("const ROW_MENU_WIDTH: f32 = 340.0;"),
        "popup_host.rs must set the row menu width this budget assumes"
    );
    let app = fs::read_to_string(manifest().join("ui/app.slint")).unwrap_or_default();
    assert!(app.contains("width: 440px;"), "popup width");
    assert!(app.contains("height: 520px;"), "popup height");
    // The default width of the kit menu is the More filters menu's width.
    let menu = fs::read_to_string(manifest().join("ui/kit/menu.slint")).unwrap_or_default();
    assert!(menu.contains("menu-width: 220px;"));
}
