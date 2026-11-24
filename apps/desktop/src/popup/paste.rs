//! Restore vs paste after a copy (Task 205, D-044). Pure: which keys paste, which hint line the
//! popup shows, which toast explains a paste that did not happen.
//!
//! Enter restores the clip to the clipboard and returns the focus (default). Ctrl+Enter, and
//! Enter when the opt-in setting "Enter also pastes" is on, also pastes into the window that
//! had the focus before the popup. Nothing pastes where the platform cannot (`can_paste`).

use super::keys::CopyKind;
use darkclip_core::i18n::Msg;
use darkclip_platform::{PasteTarget, PlatformError};

/// What happens after the clip is on the clipboard.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AfterCopy {
    /// Close the popup and return the focus to the previous window.
    Restore,
    /// Return the focus and paste there (only after every safety check).
    Paste,
}

/// Where the copy came from: the Enter key, or a button / Alt+1..9 / a mouse double-click
/// (those always only restore, D-056).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Via {
    Enter,
    Other,
}

/// The paste target after the popup lost the focus. When the popup closes itself the target is
/// kept for the next show (it is replaced then); when it stays open (the "close on focus loss"
/// setting is off) the user may have moved on, so the target is forgotten and Ctrl+Enter only
/// copies (T-19).
pub fn previous_after_focus_loss(
    previous: Option<(u64, u32)>,
    close_on_focus_loss: bool,
) -> Option<(u64, u32)> {
    if close_on_focus_loss {
        previous
    } else {
        None
    }
}

/// What a row activation (mouse double-click) copies, and as what: the original, restore only.
/// It never pastes, whatever "Enter also pastes" says (D-050, D-056, T-19).
pub fn row_activation() -> (CopyKind, Via) {
    (CopyKind::Original, Via::Other)
}

pub fn after_copy(kind: CopyKind, via: Via, enter_pastes: bool, can_paste: bool) -> AfterCopy {
    if !can_paste {
        return AfterCopy::Restore;
    }
    match (kind, via) {
        (CopyKind::Paste, _) => AfterCopy::Paste,
        (CopyKind::Original, Via::Enter) if enter_pastes => AfterCopy::Paste,
        _ => AfterCopy::Restore,
    }
}

/// The key hint line: Ctrl+Enter is mentioned only where pasting works.
pub fn key_hints(can_paste: bool, enter_pastes: bool) -> Msg {
    match (can_paste, enter_pastes) {
        (false, _) => Msg::UiKeyHints,
        (true, false) => Msg::UiKeyHintsPaste,
        (true, true) => Msg::UiKeyHintsEnterPastes,
    }
}

/// One hint of the popup's status line: a key cap and what it does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Hint {
    pub key: Msg,
    pub label: Msg,
}

/// The status line's hints (at most three), by the same rule as `key_hints`: Ctrl+Enter is
/// mentioned only where pasting works and Enter does not already paste. F1 is not advertised.
pub fn status_hints(can_paste: bool, enter_pastes: bool) -> Vec<Hint> {
    let enter = |label| Hint {
        key: Msg::UiKeyEnter,
        label,
    };
    let preview = Hint {
        key: Msg::UiKeySpace,
        label: Msg::UiHintPreview,
    };
    match (can_paste, enter_pastes) {
        (true, true) => vec![enter(Msg::UiHintPaste), preview],
        (true, false) => vec![
            enter(Msg::UiCopy),
            Hint {
                key: Msg::UiKeyCtrlEnter,
                label: Msg::UiHintPaste,
            },
            preview,
        ],
        (false, _) => vec![enter(Msg::UiCopy), preview],
    }
}

/// The capture state the status line names (from the tray's state, read when the popup shows).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CaptureShown {
    Capturing,
    Paused,
    Private,
    /// Off, not started or failed.
    Off,
}

impl CaptureShown {
    /// The dot's colour: 1 success, 2 warning (paused), 3 text-2 (private), 0 text-3 (off).
    /// The state is always named in words too: colour is never the only cue.
    pub fn tone(self) -> i32 {
        match self {
            CaptureShown::Capturing => 1,
            CaptureShown::Paused => 2,
            CaptureShown::Private => 3,
            CaptureShown::Off => 0,
        }
    }
}

pub fn capture_label(s: CaptureShown) -> Msg {
    match s {
        CaptureShown::Capturing => Msg::UiStatusCapturing,
        CaptureShown::Paused => Msg::UiStatusPaused,
        CaptureShown::Private => Msg::UiStatusPrivate,
        CaptureShown::Off => Msg::UiStatusOff,
    }
}

/// The toast after a refused or failed paste (the clip is on the clipboard either way).
pub fn paste_toast(err: &PlatformError) -> Msg {
    match err {
        PlatformError::PermissionDenied(_) => Msg::UiPasteElevated,
        _ => Msg::UiPasteManual,
    }
}

/// The toast for a copy asked while a focus return or paste is still running (the copy is not
/// started: it would race the paste).
pub const BUSY_TOAST: Msg = Msg::ErrClipboardBusy;

/// Content-free log name of a paste result.
pub fn outcome_name(r: &Result<(), PlatformError>) -> &'static str {
    match r {
        Ok(()) => "ok",
        Err(PlatformError::PermissionDenied(_)) => "elevated",
        Err(PlatformError::Unsupported(_)) => "refused",
        Err(_) => "failed",
    }
}

/// The target recorded at show time (window handle value and process id).
pub fn target(recorded: Option<(u64, u32)>) -> Option<PasteTarget> {
    recorded.map(|(window, pid)| PasteTarget {
        app_id: None,
        window: Some(window),
        pid: Some(pid),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn enter_restores_by_default_and_ctrl_enter_pastes() {
        assert_eq!(
            after_copy(CopyKind::Original, Via::Enter, false, true),
            AfterCopy::Restore
        );
        assert_eq!(
            after_copy(CopyKind::Paste, Via::Enter, false, true),
            AfterCopy::Paste
        );
        assert_eq!(
            after_copy(CopyKind::Plain, Via::Enter, false, true),
            AfterCopy::Restore
        );
    }

    #[test]
    fn enter_pastes_only_with_the_opt_in_setting() {
        assert_eq!(
            after_copy(CopyKind::Original, Via::Enter, true, true),
            AfterCopy::Paste
        );
        // Buttons and Alt+1..9 never paste; Shift+Enter (plain) stays restore-only.
        assert_eq!(
            after_copy(CopyKind::Original, Via::Other, true, true),
            AfterCopy::Restore
        );
        assert_eq!(
            after_copy(CopyKind::Plain, Via::Enter, true, true),
            AfterCopy::Restore
        );
    }

    #[test]
    fn a_double_click_never_pastes() {
        let (kind, via) = row_activation();
        assert_eq!(kind, CopyKind::Original);
        assert_eq!(via, Via::Other);
        for enter_pastes in [false, true] {
            assert_eq!(
                after_copy(kind, via, enter_pastes, true),
                AfterCopy::Restore
            );
        }
    }

    #[test]
    fn nothing_pastes_where_the_platform_cannot() {
        for kind in [CopyKind::Original, CopyKind::Plain, CopyKind::Paste] {
            for via in [Via::Enter, Via::Other] {
                for setting in [false, true] {
                    assert_eq!(after_copy(kind, via, setting, false), AfterCopy::Restore);
                }
            }
        }
    }

    #[test]
    fn ctrl_enter_hint_only_where_paste_works() {
        assert_eq!(key_hints(false, false), Msg::UiKeyHints);
        assert_eq!(key_hints(false, true), Msg::UiKeyHints);
        assert_eq!(key_hints(true, false), Msg::UiKeyHintsPaste);
        assert_eq!(key_hints(true, true), Msg::UiKeyHintsEnterPastes);
        for lang in [darkclip_core::i18n::Lang::En, darkclip_core::i18n::Lang::Vi] {
            let t = |m| darkclip_core::i18n::t(lang, m);
            assert!(!t(Msg::UiKeyHints).contains("Ctrl+Enter"));
            assert!(t(Msg::UiKeyHintsPaste).contains("Ctrl+Enter"));
        }
    }

    #[test]
    fn elevated_targets_get_their_own_toast() {
        assert_eq!(
            paste_toast(&PlatformError::PermissionDenied("x")),
            Msg::UiPasteElevated
        );
        assert_eq!(
            paste_toast(&PlatformError::Unsupported("x")),
            Msg::UiPasteManual
        );
        assert_eq!(
            paste_toast(&PlatformError::Backend { code: 5 }),
            Msg::UiPasteManual
        );
    }

    #[test]
    fn a_copy_during_a_paste_says_busy() {
        assert_eq!(BUSY_TOAST, Msg::ErrClipboardBusy);
    }

    #[test]
    fn outcome_names_are_static_and_coarse() {
        assert_eq!(outcome_name(&Ok(())), "ok");
        assert_eq!(
            outcome_name(&Err(PlatformError::PermissionDenied("x"))),
            "elevated"
        );
        assert_eq!(
            outcome_name(&Err(PlatformError::Unsupported("x"))),
            "refused"
        );
        assert_eq!(outcome_name(&Err(PlatformError::Busy)), "failed");
    }

    #[test]
    fn target_carries_window_and_pid_only() {
        assert_eq!(target(None), None);
        assert_eq!(
            target(Some((0x10, 42))),
            Some(PasteTarget {
                app_id: None,
                window: Some(0x10),
                pid: Some(42),
            })
        );
    }

    #[test]
    fn losing_focus_without_closing_forgets_the_paste_target() {
        // T-19: a popup that stays open must not paste into a window from earlier.
        let prev = Some((42, 7));
        assert_eq!(previous_after_focus_loss(prev, true), prev);
        assert_eq!(previous_after_focus_loss(prev, false), None);
        assert_eq!(previous_after_focus_loss(None, false), None);
    }

    // ---- Task 701: the status line ----

    fn pairs(h: &[Hint]) -> Vec<(Msg, Msg)> {
        h.iter().map(|h| (h.key, h.label)).collect()
    }

    #[test]
    fn status_hints_follow_the_key_hint_rule() {
        // Paste works, "Enter pastes" off: Enter copies, Ctrl+Enter pastes.
        assert_eq!(
            pairs(&status_hints(true, false)),
            [
                (Msg::UiKeyEnter, Msg::UiCopy),
                (Msg::UiKeyCtrlEnter, Msg::UiHintPaste),
                (Msg::UiKeySpace, Msg::UiHintPreview)
            ]
        );
        // "Enter pastes" on: Enter pastes and there is no Ctrl+Enter hint.
        assert_eq!(
            pairs(&status_hints(true, true)),
            [
                (Msg::UiKeyEnter, Msg::UiHintPaste),
                (Msg::UiKeySpace, Msg::UiHintPreview)
            ]
        );
        // Paste unavailable: nothing mentions pasting, whatever the setting.
        for enter_pastes in [false, true] {
            assert_eq!(
                pairs(&status_hints(false, enter_pastes)),
                [
                    (Msg::UiKeyEnter, Msg::UiCopy),
                    (Msg::UiKeySpace, Msg::UiHintPreview)
                ]
            );
        }
    }

    #[test]
    fn status_hints_are_at_most_three_and_never_advertise_f1() {
        for can in [false, true] {
            for enter in [false, true] {
                let h = status_hints(can, enter);
                assert!(h.len() <= 3);
                assert!(!h.is_empty());
                for lang in [darkclip_core::i18n::Lang::En, darkclip_core::i18n::Lang::Vi] {
                    for hint in &h {
                        let key = darkclip_core::i18n::t(lang, hint.key);
                        assert!(!key.contains("F1"), "{key}");
                        assert!(!darkclip_core::i18n::t(lang, hint.label).is_empty());
                    }
                }
            }
        }
    }

    #[test]
    fn the_capture_state_has_a_name_for_every_state() {
        for lang in [darkclip_core::i18n::Lang::En, darkclip_core::i18n::Lang::Vi] {
            for s in [
                CaptureShown::Capturing,
                CaptureShown::Paused,
                CaptureShown::Private,
                CaptureShown::Off,
            ] {
                assert!(!darkclip_core::i18n::t(lang, capture_label(s)).is_empty());
            }
        }
        assert_eq!(
            capture_label(CaptureShown::Capturing),
            Msg::UiStatusCapturing
        );
        assert_eq!(capture_label(CaptureShown::Off), Msg::UiStatusOff);
        // Only an active capture is a "good" dot.
        assert_eq!(CaptureShown::Capturing.tone(), 1);
        assert_eq!(CaptureShown::Paused.tone(), 2);
        assert_eq!(CaptureShown::Private.tone(), 3);
        assert_eq!(CaptureShown::Off.tone(), 0);
    }
}
