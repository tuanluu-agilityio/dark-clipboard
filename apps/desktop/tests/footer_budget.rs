//! Regression test (Task 414): the popup footer must fit the popup in both languages.
//!
//! In 0.1.0 the Vietnamese footer ("Sao chép dạng văn bản thuần", "Sao chép dưới dạng...") was
//! wider than the popup and the last buttons were clipped. No window is created (GC-19); this is
//! a measured character budget instead of a layout run.
//!
//! Budget: the popup is 420 px wide (`ui/app.slint`), minus a 1 px border and 10 px padding on
//! each side = 398 px for the row. Each `ActionButton` is its label width + 16 px, with 4 px
//! between buttons. Labels are 12 px Segoe UI / system UI text; 7 px per character is a
//! deliberately pessimistic average (Latin text measures about 6.2 px, Vietnamese with stacked
//! diacritics about 6.5 px), which leaves room for wider fallback fonts on other systems.

use darkclip_core::i18n::{t, Lang, Msg};

const ROW_WIDTH_PX: f32 = 420.0 - 2.0 - 20.0;
const PX_PER_CHAR: f32 = 7.0;
const BUTTON_PADDING_PX: f32 = 16.0;
const SPACING_PX: f32 = 4.0;

/// The footer labels as the popup shows them: Copy, plain copy, copy as, Pin/Unpin, Delete.
fn footer_labels(lang: Lang, pinned: bool) -> Vec<&'static str> {
    vec![
        t(lang, Msg::UiCopy),
        t(lang, Msg::UiCopyPlain),
        t(lang, Msg::TfCopyAs),
        t(lang, if pinned { Msg::UiUnpin } else { Msg::UiPin }),
        t(lang, Msg::UiDelete),
    ]
}

fn row_width(labels: &[&str]) -> f32 {
    let chars: usize = labels.iter().map(|l| l.chars().count()).sum();
    chars as f32 * PX_PER_CHAR
        + labels.len() as f32 * BUTTON_PADDING_PX
        + (labels.len() - 1) as f32 * SPACING_PX
}

#[test]
fn every_footer_button_fits_the_popup_in_both_languages() {
    for lang in [Lang::En, Lang::Vi] {
        for pinned in [false, true] {
            let labels = footer_labels(lang, pinned);
            let w = row_width(&labels);
            assert!(
                w <= ROW_WIDTH_PX,
                "{lang:?} footer needs {w} px but the popup row has {ROW_WIDTH_PX} px: {labels:?}"
            );
        }
    }
}
