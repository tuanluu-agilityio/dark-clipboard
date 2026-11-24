//! Width budgets of the settings window (Task 705, acceptance A7), in English and Vietnamese. No
//! window is created (GC-19): this is a measured character budget, not a layout run, in the same
//! spirit as `popup_budget.rs`, and nothing was loosened.
//!
//! Budget: 7 px per character is a deliberately pessimistic average for 13 px system UI text
//! (Latin measures about 6.2 px, Vietnamese with stacked diacritics about 6.5 px). A key cap is
//! 6 px per character plus 8 px.

use darkclip_core::i18n::{t, Lang, Msg};
use std::fs;
use std::path::PathBuf;

const PX_PER_CHAR: f32 = 7.0;
const CAP_PX_PER_CHAR: f32 = 6.0;
const CAP_PADDING_PX: f32 = 8.0;

const NAV_PX: f32 = 220.0;
const MIN_WINDOW_PX: f32 = 680.0;
/// Content padding on each side (`SettingsWindow` content column).
const CONTENT_PAD_PX: f32 = 24.0;
const NAV_PAD_PX: f32 = 12.0;
const NAV_ICON_PX: f32 = 16.0;
const NAV_GAP_PX: f32 = 8.0;
const CARD_GAP_PX: f32 = 8.0;
/// Padding inside one capture-mode card, each side.
const MODE_PAD_PX: f32 = 12.0;
/// Save bar: padding each side, gap between its parts, button padding, check icon and its gap.
const BAR_PAD_PX: f32 = 12.0;
const BAR_GAP_PX: f32 = 8.0;
const BUTTON_PAD_PX: f32 = 24.0;
const SAVE_ICON_PX: f32 = 16.0;
const SAVE_ICON_GAP_PX: f32 = 6.0;
const SELECT_PX: f32 = 200.0;
/// Select: padding each side, chevron and the gap before it.
const SELECT_PAD_PX: f32 = 10.0;
const SELECT_CHEVRON_PX: f32 = 16.0;
const SELECT_GAP_PX: f32 = 8.0;

fn text_px(s: &str) -> f32 {
    s.chars().count() as f32 * PX_PER_CHAR
}

fn cap_px(s: &str) -> f32 {
    s.chars().count() as f32 * CAP_PX_PER_CHAR + CAP_PADDING_PX
}

fn manifest() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn read(rel: &str) -> String {
    fs::read_to_string(manifest().join(rel)).unwrap_or_default()
}

#[test]
fn nav_labels_fit_beside_the_icon() {
    let room = NAV_PX - 2.0 * NAV_PAD_PX - NAV_ICON_PX - NAV_GAP_PX;
    for lang in [Lang::En, Lang::Vi] {
        for m in [
            Msg::SettingsSectionGeneral,
            Msg::SettingsSectionCapture,
            Msg::SettingsSectionStorage,
            Msg::SettingsSectionData,
            Msg::SettingsSectionAbout,
        ] {
            let px = text_px(t(lang, m));
            assert!(
                px <= room,
                "{lang:?} nav label {m:?} needs {px} px of {room}"
            );
        }
    }
}

#[test]
fn the_three_capture_mode_cards_fit_side_by_side_at_the_minimum_width() {
    let content = MIN_WINDOW_PX - NAV_PX - 2.0 * CONTENT_PAD_PX;
    let card = (content - 2.0 * CARD_GAP_PX) / 3.0;
    let inner = card - 2.0 * MODE_PAD_PX;
    for lang in [Lang::En, Lang::Vi] {
        let titles = [
            Msg::SettingsCardRecording,
            Msg::SettingsCardPrivate,
            Msg::SettingsCardPaused,
        ];
        let widest = titles
            .iter()
            .map(|m| text_px(t(lang, *m)))
            .fold(0.0_f32, f32::max);
        assert!(
            widest <= inner,
            "{lang:?} widest capture card title needs {widest} px of {inner}"
        );
        // The one-line description may wrap to two lines.
        for m in [
            Msg::SettingsCardRecordingLine,
            Msg::SettingsCardPrivateLine,
            Msg::SettingsCardPausedLine,
        ] {
            let px = text_px(t(lang, m));
            assert!(
                px <= 2.0 * inner,
                "{lang:?} capture card line {m:?} needs {px} px of two lines of {inner}"
            );
        }
    }
}

#[test]
fn the_save_bar_fits_at_the_minimum_width() {
    let content = MIN_WINDOW_PX - NAV_PX;
    for lang in [Lang::En, Lang::Vi] {
        let message = text_px(t(lang, Msg::SettingsUnsaved));
        let revert = text_px(t(lang, Msg::SettingsRevert)) + BUTTON_PAD_PX;
        let save = BUTTON_PAD_PX
            + SAVE_ICON_PX
            + SAVE_ICON_GAP_PX
            + text_px(t(lang, Msg::SettingsSave))
            + BAR_GAP_PX
            + cap_px(t(lang, Msg::SettingsKeyCtrlS));
        // Message, gap, Revert, gap, Save.
        let bar = 2.0 * BAR_PAD_PX + message + BAR_GAP_PX + revert + BAR_GAP_PX + save;
        assert!(
            bar <= content,
            "{lang:?} save bar needs {bar} px but has {content} px"
        );
    }
}

#[test]
fn select_labels_fit_their_width() {
    let room = SELECT_PX - 2.0 * SELECT_PAD_PX - SELECT_CHEVRON_PX - SELECT_GAP_PX;
    for lang in [Lang::En, Lang::Vi] {
        for m in [
            Msg::SettingsLangEn,
            Msg::SettingsLangVi,
            Msg::SettingsPosCursor,
            Msg::SettingsPosCenter,
            Msg::SettingsSensDrop,
            Msg::SettingsSensMasked,
            Msg::SettingsSensStore,
        ] {
            let px = text_px(t(lang, m));
            assert!(
                px <= room,
                "{lang:?} select item {m:?} needs {px} px of {room}"
            );
        }
    }
}

#[test]
fn the_sources_of_the_budget_match_the_code() {
    let w = read("ui/settings.slint");
    assert!(w.contains("preferred-width: 860px;"), "preferred width");
    assert!(w.contains("preferred-height: 640px;"), "preferred height");
    assert!(w.contains("min-width: 680px;"), "minimum width");
    assert!(w.contains("min-height: 480px;"), "minimum height");
    assert!(w.contains("width: 220px;"), "nav width");
    let parts = read("ui/settings_parts.slint");
    assert!(parts.contains("padding-left: 12px;"), "nav item padding");
    let select = read("ui/kit/select.slint");
    assert!(select.contains("width: 200px;"), "select width");
}
