//! Width budgets of the secondary surfaces (Task 707, acceptance A7), in the style of
//! `popup_budget.rs`: welcome, the F1 shortcuts sheet, the Copy-as and Edit panels and the
//! organize dialogs, in English and Vietnamese. No window is created (GC-19); this is a measured
//! character budget, nothing was loosened and no font was shrunk.
//!
//! 7 px per character is a deliberately pessimistic average for 13 px UI text (Latin measures
//! about 6.2 px, Vietnamese with stacked diacritics about 6.5 px). A key cap is 6 px per
//! character plus 8 px. A text `Button` is its label plus 24 px (`min-width` in `kit/button.slint`).
//! The panels are `min(W, window - 24)` wide with 16 px padding; the popup is 440 px wide and the
//! history window is at least 876 px wide.

#![allow(dead_code)]

// `#[path]` only at the top level (rustfmt on Linux/macOS; see tests/shortcuts.rs).
#[path = "../src/popup/keys.rs"]
pub mod popup_keys;
mod popup {
    pub use super::popup_keys as keys;
}
#[path = "../src/history/keys.rs"]
pub mod history_keys;
mod history {
    pub use super::history_keys as keys;
}
#[path = "../src/shortcuts.rs"]
mod shortcuts;

use darkclip_core::i18n::{t, Lang, Msg};
use shortcuts::{columns, rows, Win};

const PX_PER_CHAR: f32 = 7.0;
const CAP_PX_PER_CHAR: f32 = 6.0;
const CAP_PADDING_PX: f32 = 8.0;
const BUTTON_PADDING_PX: f32 = 24.0;
const BUTTON_GAP_PX: f32 = 8.0;
const PANEL_PADDING_PX: f32 = 16.0;
const POPUP_PX: f32 = 440.0;
const HISTORY_MIN_PX: f32 = 876.0;
const WELCOME_MIN_PX: f32 = 480.0;

fn text_px(s: &str) -> f32 {
    s.chars().count() as f32 * PX_PER_CHAR
}

fn cap_px(s: &str) -> f32 {
    s.chars().count() as f32 * CAP_PX_PER_CHAR + CAP_PADDING_PX
}

fn button_px(label: &str) -> f32 {
    text_px(label) + BUTTON_PADDING_PX
}

/// The width of a row of buttons with the 8 px gap between them.
fn buttons_px(labels: &[&str]) -> f32 {
    labels.iter().map(|l| button_px(l)).sum::<f32>()
        + BUTTON_GAP_PX * labels.len().saturating_sub(1) as f32
}

/// The inner width of a panel `panel_w` wide in a window `window_w` wide.
fn panel_inner(panel_w: f32, window_w: f32) -> f32 {
    panel_w.min(window_w - 24.0) - 2.0 * PANEL_PADDING_PX
}

#[test]
fn the_welcome_buttons_fit_in_both_languages() {
    // 24 px of window padding, 20 px of card padding are not on the button row: the row has the
    // window's 24 px padding on each side.
    let inner = WELCOME_MIN_PX - 48.0;
    for lang in [Lang::En, Lang::Vi] {
        let two = buttons_px(&[t(lang, Msg::WelcomeNotNow), t(lang, Msg::WelcomeStart)]);
        assert!(
            two <= inner,
            "{lang:?} welcome buttons need {two} of {inner}"
        );
        let one = buttons_px(&[t(lang, Msg::UiClose)]);
        assert!(one <= inner, "{lang:?} Close needs {one} of {inner}");
    }
}

#[test]
fn the_sheet_key_and_action_columns_fit_in_both_windows_and_languages() {
    for (win, window_w) in [(Win::Popup, POPUP_PX), (Win::History, HISTORY_MIN_PX)] {
        // Sheet width min(560, window - 24), 16 px padding, two columns and a 16 px gap.
        let column = (panel_inner(560.0, window_w) - 16.0) / 2.0;
        for lang in [Lang::En, Lang::Vi] {
            let (left, right) = columns(rows(win, lang));
            assert!(
                left.len() + right.len() >= 15,
                "the sheet has too few lines"
            );
            for r in left.iter().chain(right.iter()) {
                if r.header {
                    assert!(
                        text_px(&r.label) <= column,
                        "{win:?} {lang:?} header {:?} needs {} of {column}",
                        r.label,
                        text_px(&r.label)
                    );
                } else {
                    // Action text, an 8 px gap and the key cap.
                    let need = text_px(&r.label) + 8.0 + cap_px(&r.key);
                    assert!(
                        need <= column,
                        "{win:?} {lang:?} row {:?} [{}] needs {need} of {column}",
                        r.label,
                        r.key
                    );
                }
            }
        }
    }
}

#[test]
fn the_copy_as_and_edit_button_rows_fit_in_the_popup_in_both_languages() {
    for lang in [Lang::En, Lang::Vi] {
        let copy_as = buttons_px(&[t(lang, Msg::UiCancel), t(lang, Msg::UiCopy)]);
        let inner = panel_inner(600.0, POPUP_PX);
        assert!(
            copy_as <= inner,
            "{lang:?} Copy as needs {copy_as} of {inner}"
        );
        let edit = buttons_px(&[t(lang, Msg::UiCancel), t(lang, Msg::TfSaveNew)]);
        let inner = panel_inner(720.0, POPUP_PX);
        assert!(edit <= inner, "{lang:?} Edit needs {edit} of {inner}");
    }
}

#[test]
fn the_organize_dialog_buttons_fit_in_both_languages() {
    // Collection (new, rename) and note: Cancel + Save; tags: Close or Cancel + Add tag; the
    // pickers: Cancel + Apply. The dialog is min(480, window - 24) wide.
    let inner = panel_inner(480.0, HISTORY_MIN_PX);
    for lang in [Lang::En, Lang::Vi] {
        for cancel in [Msg::UiCancel, Msg::UiClose] {
            for submit in [Msg::OrgSave, Msg::OrgAddTag, Msg::OrgApply] {
                let row = buttons_px(&[t(lang, cancel), t(lang, submit)]);
                assert!(
                    row <= inner,
                    "{lang:?} {cancel:?} + {submit:?} need {row} of {inner}"
                );
            }
        }
    }
}
