//! Task 706: the F1 / Ctrl+/ shortcuts sheet never drifts from the key maps. The sheet's tables
//! (`src/shortcuts.rs`) are tested against the real `map_key` of both windows: every row's key
//! gives the row's action, and every action that has a key is either in a table or in an explicit
//! `NOT_LISTED` list with a reason. No window is created (GC-19).
//!
//! The binary crate cannot be imported by an integration test, so the three pure modules are
//! compiled into this test crate from their source files; `crate::popup::keys` and
//! `crate::history::keys` resolve to the real key maps.

#![allow(dead_code)]

mod popup {
    #[path = "../../src/popup/keys.rs"]
    pub mod keys;
}
mod history {
    #[path = "../../src/history/keys.rs"]
    pub mod keys;
}
#[path = "../src/shortcuts.rs"]
mod shortcuts;

use darkclip_core::i18n::{t, Lang};
use history::keys::{HistoryAction, HistoryCtx, Layers};
use popup::keys::{Action, CopyKind, Key, KeyCtx, Mods, Move};
use shortcuts::{
    columns, history_sheet_action, key_label, popup_sheet_action, rows, sheet_key, Group,
    SheetAction, SheetKey, Win, HISTORY_NOT_LISTED, HISTORY_SHORTCUTS, POPUP_NOT_LISTED,
    POPUP_SHORTCUTS,
};

const NONE: Mods = Mods {
    ctrl: false,
    shift: false,
    alt: false,
    meta: false,
};
const CTRL: Mods = Mods { ctrl: true, ..NONE };

/// The popup context that lets every listed key through: the list has the focus, an undo is
/// available, the preview pane is closed.
const POPUP_CTX: KeyCtx = KeyCtx {
    query_empty: true,
    search_focused: false,
    undo_available: true,
    chips_focused: false,
    control_focused: false,
    detail_open: false,
};
const HISTORY_CTX: HistoryCtx = HistoryCtx {
    query_empty: true,
    search_focused: false,
    chips_focused: false,
    confirming: false,
};
/// The narrow layout, where Right opens the details drawer.
const NARROW: Layers = Layers {
    narrow: true,
    drawer_open: false,
    overlay_open: false,
    menu_open: false,
};

// ---- Every row goes through the real key map ----

#[test]
fn every_popup_row_gives_its_action_through_the_real_map_key() {
    assert!(POPUP_SHORTCUTS.len() >= 20, "the popup table is too short");
    for r in POPUP_SHORTCUTS {
        let got = popup::keys::map_key(r.key, r.mods, POPUP_CTX).map(popup_sheet_action);
        assert_eq!(
            got,
            Some(r.action),
            "popup row {} ({:?})",
            key_label(r.key, r.mods),
            r.action
        );
    }
}

#[test]
fn every_history_row_gives_its_action_through_the_real_map_key() {
    assert!(
        HISTORY_SHORTCUTS.len() >= 25,
        "the history table is too short"
    );
    for r in HISTORY_SHORTCUTS {
        let got = history::keys::map_key_in(r.key, r.mods, HISTORY_CTX, NARROW)
            .and_then(history_sheet_action);
        assert_eq!(
            got,
            Some(r.action),
            "history row {} ({:?})",
            key_label(r.key, r.mods),
            r.action
        );
    }
}

// ---- Every action with a key is listed or explicitly not listed ----

/// Every `Action`. The exhaustive `match` below has no wildcard: a new variant stops compiling
/// here until it is added to this list (and so to a table or `POPUP_NOT_LISTED`).
fn all_popup_actions() -> Vec<Action> {
    let all = vec![
        Action::Move(Move::Up),
        Action::Move(Move::Down),
        Action::Move(Move::PageUp),
        Action::Move(Move::PageDown),
        Action::Move(Move::Home),
        Action::Move(Move::End),
        Action::Copy(CopyKind::Original),
        Action::Copy(CopyKind::Plain),
        Action::Copy(CopyKind::Paste),
        Action::CopyNth(0),
        Action::Delete,
        Action::TogglePin,
        Action::Undo,
        Action::Close,
        Action::ToggleDetail,
        Action::OpenDetail,
        Action::CloseDetail,
        Action::OpenHistory,
        Action::CopyAs,
        Action::ToggleFavorite,
        Action::FocusSearch,
        Action::OpenSettings,
        Action::RowMenu,
        Action::Segment(1),
        Action::MoreFilters,
        Action::Shortcuts,
    ];
    for a in &all {
        match a {
            Action::Move(_)
            | Action::Copy(_)
            | Action::CopyNth(_)
            | Action::Delete
            | Action::TogglePin
            | Action::Undo
            | Action::Close
            | Action::ToggleDetail
            | Action::OpenDetail
            | Action::CloseDetail
            | Action::OpenHistory
            | Action::CopyAs
            | Action::ToggleFavorite
            | Action::FocusSearch
            | Action::OpenSettings
            | Action::RowMenu
            | Action::Segment(_)
            | Action::MoreFilters
            | Action::Shortcuts => {}
        }
    }
    all
}

fn all_history_actions() -> Vec<HistoryAction> {
    let all = vec![
        HistoryAction::Move(Move::Up),
        HistoryAction::Move(Move::Down),
        HistoryAction::Move(Move::PageUp),
        HistoryAction::Move(Move::PageDown),
        HistoryAction::Move(Move::Home),
        HistoryAction::Move(Move::End),
        HistoryAction::Extend(Move::Down),
        HistoryAction::MoveKeep(Move::Down),
        HistoryAction::Copy(CopyKind::Original),
        HistoryAction::Copy(CopyKind::Plain),
        HistoryAction::Delete,
        HistoryAction::TogglePin,
        HistoryAction::CopyAs,
        HistoryAction::Edit,
        HistoryAction::ToggleFavorite,
        HistoryAction::EditNote,
        HistoryAction::EditTags,
        HistoryAction::AddToCollection,
        HistoryAction::RemoveFromCollection,
        HistoryAction::NewCollection,
        HistoryAction::RenameCollection,
        HistoryAction::DeleteCollection,
        HistoryAction::Export,
        HistoryAction::SelectAll,
        HistoryAction::ToggleMark,
        HistoryAction::OpenMenu,
        HistoryAction::Escape,
        HistoryAction::Confirm,
        HistoryAction::Cancel,
        HistoryAction::Swallow,
        HistoryAction::ToggleSidebar,
        HistoryAction::FocusSearch,
        HistoryAction::OpenSettings,
        HistoryAction::NextRegion,
        HistoryAction::PrevRegion,
        HistoryAction::OpenDrawer,
        HistoryAction::CloseDrawer,
        HistoryAction::Shortcuts,
    ];
    for a in &all {
        match a {
            HistoryAction::Move(_)
            | HistoryAction::Extend(_)
            | HistoryAction::MoveKeep(_)
            | HistoryAction::Copy(_)
            | HistoryAction::Delete
            | HistoryAction::TogglePin
            | HistoryAction::CopyAs
            | HistoryAction::Edit
            | HistoryAction::ToggleFavorite
            | HistoryAction::EditNote
            | HistoryAction::EditTags
            | HistoryAction::AddToCollection
            | HistoryAction::RemoveFromCollection
            | HistoryAction::NewCollection
            | HistoryAction::RenameCollection
            | HistoryAction::DeleteCollection
            | HistoryAction::Export
            | HistoryAction::SelectAll
            | HistoryAction::ToggleMark
            | HistoryAction::OpenMenu
            | HistoryAction::Escape
            | HistoryAction::Confirm
            | HistoryAction::Cancel
            | HistoryAction::Swallow
            | HistoryAction::ToggleSidebar
            | HistoryAction::FocusSearch
            | HistoryAction::OpenSettings
            | HistoryAction::NextRegion
            | HistoryAction::PrevRegion
            | HistoryAction::OpenDrawer
            | HistoryAction::CloseDrawer
            | HistoryAction::Shortcuts => {}
        }
    }
    all
}

#[test]
fn every_popup_action_is_in_the_table_or_not_listed_with_a_reason() {
    for a in all_popup_actions() {
        let s = popup_sheet_action(a);
        let listed = POPUP_SHORTCUTS.iter().any(|r| r.action == s);
        let skipped = POPUP_NOT_LISTED.iter().any(|(n, _)| *n == s);
        assert!(
            listed || skipped,
            "popup action {a:?} is neither listed nor excused"
        );
        assert!(!(listed && skipped), "{a:?} is both listed and not listed");
    }
    for (a, why) in POPUP_NOT_LISTED {
        assert!(!why.is_empty(), "{a:?} has no reason");
    }
}

#[test]
fn every_history_action_is_in_the_table_or_not_listed_with_a_reason() {
    for a in all_history_actions() {
        // `None`: a key the modal delete confirmation swallows or answers; it has its own hint.
        let Some(s) = history_sheet_action(a) else {
            continue;
        };
        let listed = HISTORY_SHORTCUTS.iter().any(|r| r.action == s);
        let skipped = HISTORY_NOT_LISTED.iter().any(|(n, _)| *n == s);
        assert!(
            listed || skipped,
            "history action {a:?} is neither listed nor excused"
        );
        assert!(!(listed && skipped), "{a:?} is both listed and not listed");
    }
    for (a, why) in HISTORY_NOT_LISTED {
        assert!(!why.is_empty(), "{a:?} has no reason");
    }
}

#[test]
fn the_history_sheet_lists_no_popup_only_key_and_the_reverse() {
    let history_actions: Vec<SheetAction> = all_history_actions()
        .into_iter()
        .filter_map(history_sheet_action)
        .collect();
    let popup_actions: Vec<SheetAction> = all_popup_actions()
        .into_iter()
        .map(popup_sheet_action)
        .collect();
    for r in HISTORY_SHORTCUTS {
        assert!(
            history_actions.contains(&r.action),
            "history lists a popup-only key: {} ({:?})",
            key_label(r.key, r.mods),
            r.action
        );
    }
    for r in POPUP_SHORTCUTS {
        assert!(
            popup_actions.contains(&r.action),
            "popup lists a history-only key: {} ({:?})",
            key_label(r.key, r.mods),
            r.action
        );
    }
    // Concrete cases: Alt+1 and Ctrl+Enter are popup keys, Ctrl+E and Space (mark) history keys.
    let has = |table: &[shortcuts::Shortcut], a: SheetAction| table.iter().any(|r| r.action == a);
    assert!(has(POPUP_SHORTCUTS, SheetAction::CopyNth));
    assert!(!has(HISTORY_SHORTCUTS, SheetAction::CopyNth));
    assert!(has(HISTORY_SHORTCUTS, SheetAction::Edit));
    assert!(!has(POPUP_SHORTCUTS, SheetAction::Edit));
}

// ---- Labels, F1 / Ctrl+/ and the key text ----

#[test]
fn every_row_has_an_english_and_a_vietnamese_label() {
    for table in [POPUP_SHORTCUTS, HISTORY_SHORTCUTS] {
        for r in table {
            for lang in [Lang::En, Lang::Vi] {
                assert!(!t(lang, r.label).is_empty(), "{:?} {lang:?}", r.action);
            }
            assert!(!key_label(r.key, r.mods).is_empty());
        }
    }
}

#[test]
fn both_windows_list_f1_and_ctrl_slash_for_the_sheet_itself() {
    for table in [POPUP_SHORTCUTS, HISTORY_SHORTCUTS] {
        let sheet: Vec<String> = table
            .iter()
            .filter(|r| r.action == SheetAction::Shortcuts)
            .map(|r| key_label(r.key, r.mods))
            .collect();
        assert!(sheet.contains(&"F1".to_string()), "{sheet:?}");
        assert!(sheet.contains(&"Ctrl+/".to_string()), "{sheet:?}");
    }
}

#[test]
fn history_opens_the_sheet_with_f1_and_ctrl_slash_from_everywhere_except_a_modal() {
    let typing = HistoryCtx {
        query_empty: false,
        search_focused: true,
        ..HISTORY_CTX
    };
    let chips = HistoryCtx {
        chips_focused: true,
        ..HISTORY_CTX
    };
    for ctx in [HISTORY_CTX, typing, chips] {
        assert_eq!(
            history::keys::map_key(Key::F1, NONE, ctx),
            Some(HistoryAction::Shortcuts)
        );
        assert_eq!(
            history::keys::map_key(Key::Char('/'), CTRL, ctx),
            Some(HistoryAction::Shortcuts)
        );
    }
    // Alt+F1 and a bare slash are not the sheet; the delete confirmation swallows the keys.
    let alt = Mods { alt: true, ..NONE };
    assert_eq!(history::keys::map_key(Key::F1, alt, HISTORY_CTX), None);
    assert_eq!(history::keys::map_key(Key::Char('/'), NONE, typing), None);
    let confirming = HistoryCtx {
        confirming: true,
        ..HISTORY_CTX
    };
    assert_eq!(
        history::keys::map_key(Key::F1, NONE, confirming),
        Some(HistoryAction::Swallow)
    );
}

#[test]
fn key_text_matches_the_tooltip_key_caps() {
    let shift = Mods {
        shift: true,
        ..NONE
    };
    let ctrl_shift = Mods {
        ctrl: true,
        shift: true,
        ..NONE
    };
    let alt = Mods { alt: true, ..NONE };
    assert_eq!(key_label(Key::Char('t'), CTRL), "Ctrl+T");
    assert_eq!(key_label(Key::Enter, shift), "Shift+Enter");
    assert_eq!(key_label(Key::Delete, NONE), "Del");
    assert_eq!(key_label(Key::Escape, NONE), "Esc");
    assert_eq!(key_label(Key::Char(' '), NONE), "Space");
    assert_eq!(key_label(Key::Char(','), CTRL), "Ctrl+,");
    assert_eq!(key_label(Key::Char('f'), ctrl_shift), "Ctrl+Shift+F");
    assert_eq!(key_label(Key::Char('1'), alt), "Alt+1");
    assert_eq!(key_label(Key::F10, shift), "Shift+F10");
    assert_eq!(key_label(Key::PageDown, NONE), "PgDn");
    assert_eq!(key_label(Key::Menu, NONE), "Menu");
    assert_eq!(
        key_label(Key::Char(char::from(slint::platform::Key::F6)), NONE),
        "F6"
    );
}

// ---- The sheet's rows ----

#[test]
fn rows_have_a_header_per_group_then_its_keys_in_both_languages() {
    for lang in [Lang::En, Lang::Vi] {
        for win in [Win::Popup, Win::History] {
            let r = rows(win, lang);
            let headers: Vec<&str> = r
                .iter()
                .filter(|x| x.header)
                .map(|x| x.label.as_str())
                .collect();
            assert_eq!(headers.len(), 4, "{win:?} {lang:?}: {headers:?}");
            assert!(r[0].header, "the first row is a group header");
            for x in &r {
                assert!(!x.label.is_empty());
                assert_eq!(x.header, x.key.is_empty(), "{x:?}");
            }
            let table = match win {
                Win::Popup => POPUP_SHORTCUTS,
                Win::History => HISTORY_SHORTCUTS,
            };
            assert_eq!(r.iter().filter(|x| !x.header).count(), table.len());
        }
    }
}

#[test]
fn the_groups_are_navigate_copy_organize_window_in_that_order() {
    for table in [POPUP_SHORTCUTS, HISTORY_SHORTCUTS] {
        let mut seen: Vec<Group> = Vec::new();
        for r in table {
            if seen.last() != Some(&r.group) {
                assert!(!seen.contains(&r.group), "{:?} is split", r.group);
                seen.push(r.group);
            }
        }
        assert_eq!(
            seen,
            vec![Group::Navigate, Group::Copy, Group::Organize, Group::Window]
        );
    }
}

#[test]
fn the_two_columns_keep_every_row_and_split_between_groups() {
    for win in [Win::Popup, Win::History] {
        let all = rows(win, Lang::En);
        let (a, b) = columns(all.clone());
        assert_eq!(a.len() + b.len(), all.len());
        let mut joined = a.clone();
        joined.extend(b.clone());
        assert_eq!(joined, all, "{win:?}: order is kept");
        assert!(
            a[0].header && b[0].header,
            "each column starts with a header"
        );
        // No column is more than a group longer than the other.
        let diff = a.len().abs_diff(b.len());
        assert!(diff <= 12, "{win:?}: columns {} and {}", a.len(), b.len());
    }
}

// ---- The sheet's state ----

#[test]
fn f1_opens_and_f1_ctrl_slash_or_esc_close_the_sheet() {
    // Closed: F1 and Ctrl+/ open it; everything else is the window's.
    assert_eq!(sheet_key(false, Key::F1, NONE), SheetKey::Open);
    assert_eq!(sheet_key(false, Key::Char('/'), CTRL), SheetKey::Open);
    assert_eq!(sheet_key(false, Key::Escape, NONE), SheetKey::Pass);
    assert_eq!(sheet_key(false, Key::Char('a'), NONE), SheetKey::Pass);
    // Open: F1, Ctrl+/ and Esc close it.
    assert_eq!(sheet_key(true, Key::F1, NONE), SheetKey::Close);
    assert_eq!(sheet_key(true, Key::Char('/'), CTRL), SheetKey::Close);
    assert_eq!(sheet_key(true, Key::Escape, NONE), SheetKey::Close);
}

#[test]
fn every_other_key_is_swallowed_while_the_sheet_is_open() {
    let shift = Mods {
        shift: true,
        ..NONE
    };
    for (k, m) in [
        (Key::Char('a'), NONE),
        (Key::Enter, NONE),
        (Key::Down, NONE),
        (Key::Delete, NONE),
        (Key::Tab, NONE),
        (Key::Char('t'), CTRL),
        (Key::Char('p'), CTRL),
        (Key::Enter, shift),
        (Key::Other, NONE),
        (Key::F10, shift),
        // A bare slash and Alt+F1 are typing and the window's own, so the sheet swallows them.
        (Key::Char('/'), NONE),
        (Key::F1, Mods { alt: true, ..NONE }),
    ] {
        assert_eq!(sheet_key(true, k, m), SheetKey::Swallow, "{k:?} {m:?}");
    }
}

#[test]
fn esc_closes_the_sheet_before_the_window_and_the_next_esc_is_the_windows() {
    // The hosts ask `sheet_key` first: with the sheet open Esc is `Close` (the sheet), and only
    // with it closed does Esc reach the window's own map (`Pass`).
    let mut open = true;
    assert_eq!(sheet_key(open, Key::Escape, NONE), SheetKey::Close);
    open = false;
    assert_eq!(sheet_key(open, Key::Escape, NONE), SheetKey::Pass);
    assert_eq!(
        popup::keys::map_key(Key::Escape, NONE, POPUP_CTX),
        Some(Action::Close)
    );
    assert_eq!(
        history::keys::map_key(Key::Escape, NONE, HISTORY_CTX),
        Some(HistoryAction::Escape)
    );
}

// ---- Width budget of one sheet row (A7) ----

/// 7 px per character for text, 6 px per character plus 8 for a key cap (as in `popup_budget`).
fn text_px(s: &str) -> f32 {
    s.chars().count() as f32 * 7.0
}
fn cap_px(s: &str) -> f32 {
    s.chars().count() as f32 * 6.0 + 8.0
}

/// One column of the sheet at a window width: the panel is `min(560, width - 24)` wide with 16 px
/// padding each side, two columns and a 16 px gap (`ui/transform.slint`, `ShortcutSheet`).
fn column_px(window: f32) -> f32 {
    let panel = (window - 24.0).min(560.0);
    (panel - 32.0 - 16.0) / 2.0
}

fn widest_row(win: Win, lang: Lang) -> (f32, String) {
    let table = match win {
        Win::Popup => POPUP_SHORTCUTS,
        Win::History => HISTORY_SHORTCUTS,
    };
    let mut worst = (0.0_f32, String::new());
    for r in table {
        let key = key_label(r.key, r.mods);
        // The label, 8 px, the key cap.
        let px = text_px(t(lang, r.label)) + 8.0 + cap_px(&key);
        if px > worst.0 {
            worst = (px, format!("{key} {}", t(lang, r.label)));
        }
    }
    worst
}

#[test]
fn a_popup_sheet_row_fits_its_column_in_both_languages() {
    // The popup is 440 px wide.
    let col = column_px(440.0);
    for lang in [Lang::En, Lang::Vi] {
        let (px, what) = widest_row(Win::Popup, lang);
        assert!(
            px <= col,
            "{lang:?} popup sheet row {what:?} needs {px} px of {col}"
        );
    }
}

#[test]
fn a_history_sheet_row_fits_its_column_at_the_narrowest_window() {
    // The history window is at least 640 px wide; the panel stops growing at 560 px.
    let col = column_px(640.0);
    for lang in [Lang::En, Lang::Vi] {
        let (px, what) = widest_row(Win::History, lang);
        assert!(
            px <= col,
            "{lang:?} history sheet row {what:?} needs {px} px of {col}"
        );
    }
}

#[test]
fn the_sheet_titles_fit_one_line_of_the_popup_panel() {
    let inner = 440.0 - 24.0 - 32.0;
    for lang in [Lang::En, Lang::Vi] {
        for m in [
            darkclip_core::i18n::Msg::SheetTitle,
            darkclip_core::i18n::Msg::SheetEscCloses,
        ] {
            assert!(text_px(t(lang, m)) <= inner, "{lang:?} {m:?}");
        }
    }
}
