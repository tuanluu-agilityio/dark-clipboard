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
            Msg::SettingsSectionTranslate,
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
            // Task 606: the translate direction.
            Msg::SettingsTrDirAuto,
            Msg::SettingsTrDirEn,
            Msg::SettingsTrDirVi,
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

// ---- Settings > Translate and the overlay (Task 606) ------------------------------------------

/// The model select of Settings > Translate is wider than the kit's 200 px.
const MODEL_SELECT_PX: f32 = 260.0;
/// Inside a card at the minimum window width: content minus the card's 16 px padding each side.
const CARD_INNER_PX: f32 = MIN_WINDOW_PX - NAV_PX - 2.0 * CONTENT_PAD_PX - 2.0 * 16.0;
/// A setting row's gap between the label column and the control.
const ROW_GAP_PX: f32 = 16.0;
/// The label column keeps at least this much beside the control (it wraps).
const MIN_LABEL_PX: f32 = 80.0;
/// The key field, the shortcut field and the gap between the key row's parts.
const KEY_FIELD_PX: f32 = 220.0;
const SHORTCUT_FIELD_PX: f32 = 220.0;
const KEY_GAP_PX: f32 = 6.0;
/// The overlay: 340 px wide, padding 10 left and 4 right, the 16 px icon, the 28 px close button
/// and two 8 px gaps leave the text column.
const OVERLAY_PX: f32 = 340.0;
const OVERLAY_TEXT_PX: f32 = OVERLAY_PX - 10.0 - 4.0 - 16.0 - 28.0 - 2.0 * 8.0;

fn button_px(label: &str) -> f32 {
    text_px(label) + BUTTON_PAD_PX
}

#[test]
fn model_names_fit_the_wider_select() {
    let room = MODEL_SELECT_PX - 2.0 * SELECT_PAD_PX - SELECT_CHEVRON_PX - SELECT_GAP_PX;
    for lang in [Lang::En, Lang::Vi] {
        for m in [Msg::SettingsTrModelOpus, Msg::SettingsTrModelHaiku] {
            let px = text_px(t(lang, m));
            assert!(px <= room, "{lang:?} model {m:?} needs {px} px of {room}");
        }
    }
}

#[test]
fn translate_row_controls_fit_beside_their_labels_at_the_minimum_width() {
    let room = CARD_INNER_PX - ROW_GAP_PX - MIN_LABEL_PX;
    for lang in [Lang::En, Lang::Vi] {
        let tx = |m| t(lang, m);
        let rows = [
            (
                "key entry",
                KEY_FIELD_PX + KEY_GAP_PX + button_px(tx(Msg::SettingsTrKeySave)),
            ),
            (
                "key stored",
                button_px(tx(Msg::SettingsTrKeyReplace))
                    + KEY_GAP_PX
                    + button_px(tx(Msg::SettingsTrKeyRemove)),
            ),
            ("test", button_px(tx(Msg::SettingsTrTestButton))),
            ("shortcut", SHORTCUT_FIELD_PX),
            ("model", MODEL_SELECT_PX),
            ("direction", SELECT_PX),
        ];
        for (name, px) in rows {
            assert!(
                px <= room,
                "{lang:?} {name} row control needs {px} px of {room}"
            );
        }
        // The consent dialog's buttons fit its 420 px panel (16 px padding, 8 px gap).
        let buttons = button_px(tx(Msg::UiCancel)) + 8.0 + button_px(tx(Msg::SettingsTrConsentOn));
        assert!(
            buttons <= 420.0 - 32.0,
            "{lang:?} consent buttons need {buttons} px"
        );
    }
}

#[test]
fn every_overlay_text_fits_two_lines_of_the_340_px_overlay() {
    let overlay = [
        Msg::TranslateOvSending,
        Msg::TranslateOvEscToCancel,
        Msg::TranslateOvDone,
        Msg::TranslateOvDoneDetail,
        Msg::TranslateOvCancelled,
        Msg::TranslateOvLeft,
        Msg::TranslateOvLeftDetail,
        Msg::TranslateOvNothingSent,
        Msg::TranslateOvUnchanged,
        Msg::TranslateOvTryLater,
        Msg::TranslateOvOff,
        Msg::TranslateOvOffDetail,
        Msg::TranslateOvNoKey,
        Msg::TranslateOvNoKeyDetail,
        Msg::TranslateOvPrivate,
        Msg::TranslateOvPrivateDetail,
        Msg::TranslateOvTerminal,
        Msg::TranslateOvUnknownApp,
        Msg::TranslateOvExcluded,
        Msg::TranslateOvExcludedDetail,
        Msg::TranslateOvNotNormalWindow,
        Msg::TranslateOvKeysHeld,
        Msg::TranslateOvNothingCopied,
        Msg::TranslateOvNothingCopiedDetail,
        Msg::TranslateOvProtected,
        Msg::TranslateOvNotText,
        Msg::TranslateOvSensitive,
        Msg::TranslateOvSensitiveDetail,
        Msg::TranslateOvEmpty,
        Msg::TranslateOvTooLong,
        Msg::TranslateOvClipboardChanged,
        Msg::TranslateOvClipboardChangedDetail,
        Msg::TranslateOvWriteFailed,
        Msg::TranslateOvAuth,
        Msg::TranslateOvAuthDetail,
        Msg::TranslateOvRateLimited,
        Msg::TranslateOvOverloaded,
        Msg::TranslateOvNetwork,
        Msg::TranslateOvRefused,
        Msg::TranslateOvOther,
    ];
    for lang in [Lang::En, Lang::Vi] {
        for m in overlay {
            // The count goes up to 8000 (four digits).
            let text = t(lang, m).replace("{n}", "8000");
            let px = text_px(&text);
            assert!(
                px <= 2.0 * OVERLAY_TEXT_PX,
                "{lang:?} overlay text {m:?} needs {px} px of two lines of {OVERLAY_TEXT_PX}"
            );
        }
    }
}

#[test]
fn the_translate_sources_match_the_budget() {
    let parts = read("ui/settings_parts.slint");
    assert!(parts.contains("width: 260px;"), "model select width");
    assert!(parts.contains("width: 220px;"), "key field width");
    assert!(parts.contains("spacing: 6px;"), "key row gap");
    let ov = read("ui/translate_overlay.slint");
    assert!(ov.contains("width: 340px;"), "overlay width");
    assert!(ov.contains("padding-left: 10px;"), "overlay left padding");
    assert!(ov.contains("padding-right: 4px;"), "overlay right padding");
    assert!(ov.contains("spacing: 8px;"), "overlay gap");
}
