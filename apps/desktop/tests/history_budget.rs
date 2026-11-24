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
    // The layout in the markup uses the same numbers.
    let slint = fs::read_to_string(manifest().join("ui/history.slint")).unwrap_or_default();
    assert!(slint.contains("parent.width - 216px - 340px"), "list width");
    assert!(slint.contains("width: 340px;"), "detail width");
    assert!(slint.contains("width: 216px;"), "sidebar width");
    assert!(slint.contains("height: 48px;"), "toolbar height");
    let row = fs::read_to_string(manifest().join("ui/history_row.slint")).unwrap_or_default();
    assert!(
        row.contains("out property <length> height: 56px;"),
        "row height"
    );
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
