//! The translate-in-place overlay window (Task 606): the real [`OverlaySink`].
//!
//! The flow thread calls [`WindowOverlay::show`]; it writes the same content-free log line as
//! before (`LogOverlay`) and hands the state to the event loop with
//! `slint::invoke_from_event_loop`. On the event loop the state becomes the prototype's text
//! ([`view`], i18n EN/VI), the window is placed near the mouse cursor on that monitor
//! (`popup/place.rs`) and hidden again after 1.5 s (done, cancelled) or 4 s (blocked, failed,
//! left on the clipboard); while sending it stays until the next state.
//!
//! **No focus, ever.** The overlay is shown while the user types in another app, and the paste
//! goes to whatever window has the focus, so the overlay must never take it:
//! - its winit window is created with `active = false` ([`window_attributes`], the backend's
//!   window-attributes hook), so winit's first show is `SW_SHOWNOACTIVATE`;
//! - before that first show, and again right after it (winit rewrites the extended style when it
//!   shows a window), its HWND gets `WS_EX_NOACTIVATE | WS_EX_TOOLWINDOW | WS_EX_TOPMOST`
//!   (`winsys::set_overlay_style`), so a click on it does not activate it either;
//! - every later show and hide is Win32 only (`SetWindowPos(SWP_SHOWWINDOW | SWP_NOACTIVATE)`,
//!   `ShowWindow(SW_HIDE)`); Slint's `show()` is called once and its `hide()` never, because
//!   winit would show the window again with `SW_SHOW` (which activates);
//! - if the HWND cannot be had (another backend), the window is never shown: the log line stays.
//!   A show that still ends with the overlay in the foreground logs `translate_overlay_focus`.
//!
//! Nothing here carries or logs clip text, translations, keys or app names (GC-06).

// The window is Windows only; the pure mapping compiles and is tested everywhere (F-19).
#![cfg_attr(not(windows), allow(dead_code))]

use crate::log::Log;
use crate::translate::flow::{Block, FlowError, OverlayState};
use crate::translate::host::{LogOverlay, OverlaySink};
use darkclip_ai::AiError;
use darkclip_app::capture::ClaimError;
use darkclip_core::i18n::{t, Lang, Msg};
use std::sync::Arc;

/// Done and cancelled hide after this long.
pub(crate) const SHORT_HIDE_MS: u64 = 1500;
/// Blocked, failed and left on the clipboard hide after this long.
pub(crate) const LONG_HIDE_MS: u64 = 4000;
/// The overlay's width in logical pixels (the prototype's `max-width`).
pub(crate) const OVERLAY_WIDTH: i32 = 340;

/// The icon of a state (the prototype's icons; vector icons from `ui/icons`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ViewIcon {
    Spinner,
    Check,
    Cross,
    Info,
    Lock,
    EyeOff,
    Shield,
}

/// How the overlay shows one state. Pure data: codes, counts and i18n text only.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct OverlayView {
    /// 0 sending, 1 done, 2 cancelled, 3 left on the clipboard, 4 blocked, 5 failed.
    pub state: i32,
    pub message: String,
    pub detail: String,
    pub icon: ViewIcon,
    /// 0 neutral, 1 warning, 2 danger.
    pub tone: i32,
    /// `None`: shown until the next state.
    pub hide_after_ms: Option<u64>,
}

const NEUTRAL: i32 = 0;
const WARNING: i32 = 1;
const DANGER: i32 = 2;

/// Message, detail, icon and tone of a block (state 4).
fn block_parts(b: Block) -> (Msg, Option<Msg>, ViewIcon, i32) {
    match b {
        Block::Off => (
            Msg::TranslateOvOff,
            Some(Msg::TranslateOvOffDetail),
            ViewIcon::Info,
            WARNING,
        ),
        Block::NoKey => (
            Msg::TranslateOvNoKey,
            Some(Msg::TranslateOvNoKeyDetail),
            ViewIcon::Info,
            WARNING,
        ),
        Block::Private => (
            Msg::TranslateOvPrivate,
            Some(Msg::TranslateOvPrivateDetail),
            ViewIcon::EyeOff,
            WARNING,
        ),
        Block::Terminal => (
            Msg::TranslateOvTerminal,
            Some(Msg::TranslateOvNothingSent),
            ViewIcon::Info,
            WARNING,
        ),
        Block::UnknownApp => (
            Msg::TranslateOvUnknownApp,
            Some(Msg::TranslateOvNothingSent),
            ViewIcon::Info,
            WARNING,
        ),
        Block::ExcludedApp => (
            Msg::TranslateOvExcluded,
            Some(Msg::TranslateOvExcludedDetail),
            ViewIcon::Lock,
            DANGER,
        ),
        Block::NotNormalWindow => (
            Msg::TranslateOvNotNormalWindow,
            Some(Msg::TranslateOvNothingSent),
            ViewIcon::Info,
            WARNING,
        ),
        Block::KeysHeld => (
            Msg::TranslateOvKeysHeld,
            Some(Msg::TranslateOvNothingSent),
            ViewIcon::Info,
            WARNING,
        ),
    }
}

/// Message and detail of a request error (also the Test button's error text).
pub(crate) fn ai_error_parts(e: AiError) -> (Msg, Msg) {
    match e {
        AiError::Auth => (Msg::TranslateOvAuth, Msg::TranslateOvAuthDetail),
        AiError::RateLimited => (Msg::TranslateOvRateLimited, Msg::TranslateOvTryLater),
        AiError::Overloaded => (Msg::TranslateOvOverloaded, Msg::TranslateOvTryLater),
        AiError::Network | AiError::Timeout => (Msg::TranslateOvNetwork, Msg::TranslateOvUnchanged),
        AiError::Refused => (Msg::TranslateOvRefused, Msg::TranslateOvUnchanged),
        AiError::NoKey => (Msg::TranslateOvNoKey, Msg::TranslateOvNoKeyDetail),
        AiError::TooLong => (Msg::TranslateOvTooLong, Msg::TranslateOvNothingSent),
        AiError::Empty | AiError::Http(_) | AiError::Truncated | AiError::BadResponse => {
            (Msg::TranslateOvOther, Msg::TranslateOvUnchanged)
        }
    }
}

/// Message, detail and icon of a failure (state 5; `NotPasted` reads as left on the clipboard).
fn error_parts(e: FlowError) -> (Msg, Option<Msg>, ViewIcon) {
    match e {
        FlowError::NothingCopied | FlowError::Claim(ClaimError::Expired) => (
            Msg::TranslateOvNothingCopied,
            Some(Msg::TranslateOvNothingCopiedDetail),
            ViewIcon::Info,
        ),
        FlowError::Claim(ClaimError::Skipped(_) | ClaimError::NoCloud) => (
            Msg::TranslateOvProtected,
            Some(Msg::TranslateOvNothingSent),
            ViewIcon::Lock,
        ),
        FlowError::Claim(ClaimError::NotText) => (
            Msg::TranslateOvNotText,
            Some(Msg::TranslateOvNothingSent),
            ViewIcon::Info,
        ),
        FlowError::Claim(ClaimError::TooLong) | FlowError::TooLong => (
            Msg::TranslateOvTooLong,
            Some(Msg::TranslateOvNothingSent),
            ViewIcon::Info,
        ),
        FlowError::Claim(ClaimError::Busy) => (
            Msg::TranslateOvOther,
            Some(Msg::TranslateOvTryLater),
            ViewIcon::Info,
        ),
        FlowError::Sensitive => (
            Msg::TranslateOvSensitive,
            Some(Msg::TranslateOvSensitiveDetail),
            ViewIcon::Shield,
        ),
        FlowError::Empty => (
            Msg::TranslateOvEmpty,
            Some(Msg::TranslateOvNothingSent),
            ViewIcon::Info,
        ),
        FlowError::ClipboardChanged => (
            Msg::TranslateOvClipboardChanged,
            Some(Msg::TranslateOvClipboardChangedDetail),
            ViewIcon::Info,
        ),
        FlowError::NotPasted => (
            Msg::TranslateOvLeft,
            Some(Msg::TranslateOvLeftDetail),
            ViewIcon::Info,
        ),
        FlowError::WriteFailed => (
            Msg::TranslateOvWriteFailed,
            Some(Msg::TranslateOvUnchanged),
            ViewIcon::Info,
        ),
        FlowError::Ai(a) => {
            let (m, d) = ai_error_parts(a);
            (m, Some(d), ViewIcon::Info)
        }
    }
}

/// The overlay for `s` in `lang`. "Translated" never names a direction (the default is auto).
pub(crate) fn view(lang: Lang, s: &OverlayState) -> OverlayView {
    let text = |m: Msg| t(lang, m).to_string();
    let opt = |m: Option<Msg>| m.map(|m| t(lang, m).to_string()).unwrap_or_default();
    match *s {
        OverlayState::Sending { chars } => OverlayView {
            state: 0,
            message: t(lang, Msg::TranslateOvSending).replace("{n}", &chars.to_string()),
            detail: text(Msg::TranslateOvEscToCancel),
            icon: ViewIcon::Spinner,
            tone: NEUTRAL,
            hide_after_ms: None,
        },
        OverlayState::Done => OverlayView {
            state: 1,
            message: text(Msg::TranslateOvDone),
            detail: text(Msg::TranslateOvDoneDetail),
            icon: ViewIcon::Check,
            tone: NEUTRAL,
            hide_after_ms: Some(SHORT_HIDE_MS),
        },
        OverlayState::Cancelled => OverlayView {
            state: 2,
            message: text(Msg::TranslateOvCancelled),
            detail: String::new(),
            icon: ViewIcon::Cross,
            tone: NEUTRAL,
            hide_after_ms: Some(SHORT_HIDE_MS),
        },
        OverlayState::LeftOnClipboard | OverlayState::Failed(FlowError::NotPasted) => OverlayView {
            state: 3,
            message: text(Msg::TranslateOvLeft),
            detail: text(Msg::TranslateOvLeftDetail),
            icon: ViewIcon::Info,
            tone: WARNING,
            hide_after_ms: Some(LONG_HIDE_MS),
        },
        OverlayState::Blocked(b) => {
            let (m, d, icon, tone) = block_parts(b);
            OverlayView {
                state: 4,
                message: text(m),
                detail: opt(d),
                icon,
                tone,
                hide_after_ms: Some(LONG_HIDE_MS),
            }
        }
        OverlayState::Failed(e) => {
            let (m, d, icon) = error_parts(e);
            OverlayView {
                state: 5,
                message: text(m),
                detail: opt(d),
                icon,
                tone: DANGER,
                hide_after_ms: Some(LONG_HIDE_MS),
            }
        }
    }
}

/// The overlay sink of the running app: the content-free log line, then the window.
pub(crate) struct WindowOverlay {
    log: LogOverlay,
}

impl WindowOverlay {
    pub(crate) fn new(log: Arc<Log>) -> Self {
        Self {
            log: LogOverlay(log),
        }
    }
}

impl OverlaySink for WindowOverlay {
    fn show(&self, s: &OverlayState) {
        self.log.show(s);
        let s = *s;
        // Called on the flow thread: the window lives on the event loop.
        let _ = slint::invoke_from_event_loop(move || imp::apply(s));
    }
}

/// Creates the (hidden) overlay window. Main thread, once, before the event loop runs.
pub(crate) fn install(lang: Lang, log: Arc<Log>) {
    imp::install(lang, log);
}

/// The UI language changed (main thread).
pub(crate) fn set_lang(lang: Lang) {
    imp::set_lang(lang);
}

/// Hides the overlay at shutdown (main thread).
pub(crate) fn shutdown() {
    imp::hide();
}

/// The backend's window-attributes hook (`renderer::select`): the overlay's window is created
/// inactive, frameless, topmost and out of the taskbar; every other window is left as it is.
pub(crate) fn window_attributes(
    attrs: slint::winit_030::winit::window::WindowAttributes,
) -> slint::winit_030::winit::window::WindowAttributes {
    imp::window_attributes(attrs)
}

#[cfg(windows)]
mod imp {
    use super::{view, OverlayView, ViewIcon, OVERLAY_WIDTH};
    use crate::log::{Log, Val};
    use crate::popup::place::{popup_origin, scale};
    use crate::translate::flow::OverlayState;
    use crate::ui::generated::IconName;
    use crate::ui::TranslateOverlay;
    use darkclip_core::i18n::{t, Lang, Msg};
    use slint::winit_030::winit;
    use slint::ComponentHandle;
    use std::cell::{Cell, RefCell};
    use std::sync::Arc;
    use std::time::Duration;

    struct Overlay {
        win: TranslateOverlay,
        lang: Lang,
        log: Arc<Log>,
        /// Slint's `show()` ran (once; later shows are Win32 only).
        shown_once: bool,
        /// The current state is "sending" (the close button cancels then).
        sending: bool,
        paint: i32,
        timer: slint::Timer,
    }

    thread_local! {
        static OVERLAY: RefCell<Option<Overlay>> = const { RefCell::new(None) };
        /// Set only while the overlay component is created, so the hook changes that one
        /// window and no other.
        static CREATING: Cell<bool> = const { Cell::new(false) };
    }

    pub(super) fn window_attributes(
        attrs: winit::window::WindowAttributes,
    ) -> winit::window::WindowAttributes {
        if !CREATING.with(Cell::get) {
            return attrs;
        }
        use winit::platform::windows::WindowAttributesExtWindows;
        attrs
            .with_active(false)
            .with_decorations(false)
            .with_resizable(false)
            .with_window_level(winit::window::WindowLevel::AlwaysOnTop)
            .with_skip_taskbar(true)
    }

    pub(super) fn install(lang: Lang, log: Arc<Log>) {
        CREATING.with(|c| c.set(true));
        let created = TranslateOverlay::new();
        CREATING.with(|c| c.set(false));
        let Ok(win) = created else {
            log.event(
                "translate_overlay_window",
                &[("status", Val::Name("create_failed"))],
            );
            return;
        };
        crate::theme::apply(&win);
        win.on_cancel(on_cancel);
        OVERLAY.with(|o| {
            *o.borrow_mut() = Some(Overlay {
                win,
                lang,
                log,
                shown_once: false,
                sending: false,
                paint: 0,
                timer: slint::Timer::default(),
            });
        });
    }

    pub(super) fn set_lang(lang: Lang) {
        OVERLAY.with(|o| {
            if let Some(ov) = o.borrow_mut().as_mut() {
                ov.lang = lang;
            }
        });
    }

    /// The close button: cancels while sending (the same flag as Esc), otherwise hides.
    fn on_cancel() {
        let sending = OVERLAY.with(|o| o.borrow().as_ref().is_some_and(|ov| ov.sending));
        if sending {
            crate::translate::host::on_cancel();
        } else {
            hide();
        }
    }

    /// The overlay window's handle, once winit created it (the event loop ran once).
    fn hwnd(win: &TranslateOverlay) -> Option<u64> {
        use slint::winit_030::WinitWindowAccessor;
        use winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};
        win.window()
            .with_winit_window(|w| match w.window_handle().map(|h| h.as_raw()) {
                Ok(RawWindowHandle::Win32(h)) => u64::try_from(h.hwnd.get()).ok(),
                _ => None,
            })
            .flatten()
    }

    fn icon_name(i: ViewIcon) -> IconName {
        match i {
            ViewIcon::Check => IconName::Check,
            ViewIcon::Cross => IconName::X,
            ViewIcon::Lock => IconName::Lock,
            ViewIcon::EyeOff => IconName::EyeOff,
            ViewIcon::Shield => IconName::ShieldCheck,
            ViewIcon::Spinner | ViewIcon::Info => IconName::Info,
        }
    }

    pub(super) fn apply(s: OverlayState) {
        OVERLAY.with(|o| {
            let mut guard = o.borrow_mut();
            let Some(ov) = guard.as_mut() else {
                return;
            };
            let v = view(ov.lang, &s);
            fill(ov, &v);
            if !show(ov) {
                return;
            }
            ov.sending = v.state == 0;
            match v.hide_after_ms {
                Some(ms) => ov.timer.start(
                    slint::TimerMode::SingleShot,
                    Duration::from_millis(ms),
                    hide,
                ),
                None => ov.timer.stop(),
            }
        });
    }

    fn fill(ov: &mut Overlay, v: &OverlayView) {
        let w = &ov.win;
        w.set_state(v.state);
        w.set_message(v.message.as_str().into());
        w.set_detail(v.detail.as_str().into());
        w.set_state_icon(icon_name(v.icon));
        w.set_tone(v.tone);
        w.set_cancel_label(t(ov.lang, Msg::UiCancel).into());
        w.set_reduce_motion(crate::winsys::reduced_motion());
        ov.paint = ov.paint.wrapping_add(1) & 0x3FFF_FFFF;
        w.set_paint(ov.paint);
    }

    /// Places and shows the window without activating it. False when it cannot be shown safely.
    fn show(ov: &mut Overlay) -> bool {
        let Some(hwnd) = hwnd(&ov.win) else {
            // No winit window (yet, or another backend): never risk a focus change.
            return false;
        };
        // Before the first show, and before every show (a no-op when already set).
        if !crate::winsys::set_overlay_style(hwnd) {
            ov.log.event(
                "translate_overlay_window",
                &[("status", Val::Name("style_failed"))],
            );
            return false;
        }
        // Windows cannot tell where the cursor is: not shown (and no hide timer runs).
        let Some(m) = crate::winsys::monitor_at_cursor() else {
            ov.log.event(
                "translate_overlay_window",
                &[("status", Val::Name("no_monitor"))],
            );
            return false;
        };
        let h = ov.win.get_content_height().ceil();
        // Logical pixels, at most a few hundred: the cast cannot truncate.
        #[allow(clippy::cast_possible_truncation)]
        let h = h as i32;
        let size = (scale(OVERLAY_WIDTH, m.dpi), scale(h.max(1), m.dpi));
        let (x, y) = popup_origin(m.cursor, size, m.work, m.dpi);
        if !ov.shown_once {
            ov.win
                .window()
                .set_position(slint::PhysicalPosition::new(x, y));
            // The window was created with `active = false`: winit shows it with
            // SW_SHOWNOACTIVATE. Slint's show is never called again.
            if ov.win.show().is_err() {
                return false;
            }
            ov.shown_once = true;
            // winit rewrote the extended style while showing: set it again at once.
            let _ = crate::winsys::set_overlay_style(hwnd);
        }
        let _ = crate::winsys::show_no_activate_at(hwnd, x, y);
        ov.win.set_active(true);
        if !crate::winsys::has_overlay_ex_style(crate::winsys::ex_style(hwnd)) {
            ov.log.event(
                "translate_overlay_window",
                &[("status", Val::Name("style_incomplete"))],
            );
        }
        if crate::winsys::is_foreground(hwnd) {
            ov.log.event(
                "translate_overlay_focus",
                &[("took", Val::Name("foreground"))],
            );
        }
        true
    }

    pub(super) fn hide() {
        OVERLAY.with(|o| {
            let mut guard = o.borrow_mut();
            let Some(ov) = guard.as_mut() else {
                return;
            };
            ov.timer.stop();
            ov.sending = false;
            ov.win.set_active(false);
            if let Some(h) = hwnd(&ov.win) {
                crate::winsys::hide_window(h);
            }
        });
    }
}

#[cfg(not(windows))]
mod imp {
    use crate::log::Log;
    use crate::translate::flow::OverlayState;
    use darkclip_core::i18n::Lang;
    use std::sync::Arc;

    pub(super) fn window_attributes(
        attrs: slint::winit_030::winit::window::WindowAttributes,
    ) -> slint::winit_030::winit::window::WindowAttributes {
        attrs
    }
    pub(super) fn install(_lang: Lang, _log: Arc<Log>) {}
    pub(super) fn set_lang(_lang: Lang) {}
    pub(super) fn apply(_s: OverlayState) {}
    pub(super) fn hide() {}
}

#[cfg(test)]
mod tests {
    use super::*;
    use darkclip_core::policy::SkipReason;

    /// Every overlay state the flow can send.
    fn every_state() -> Vec<OverlayState> {
        let mut v = vec![
            OverlayState::Sending { chars: 42 },
            OverlayState::Done,
            OverlayState::Cancelled,
            OverlayState::LeftOnClipboard,
        ];
        for b in [
            Block::Off,
            Block::NoKey,
            Block::Private,
            Block::Terminal,
            Block::UnknownApp,
            Block::ExcludedApp,
            Block::NotNormalWindow,
            Block::KeysHeld,
        ] {
            v.push(OverlayState::Blocked(b));
        }
        let mut errors = vec![
            FlowError::NothingCopied,
            FlowError::Claim(ClaimError::Skipped(SkipReason::ExcludedApp)),
            FlowError::Claim(ClaimError::NoCloud),
            FlowError::Claim(ClaimError::NotText),
            FlowError::Claim(ClaimError::TooLong),
            FlowError::Claim(ClaimError::Expired),
            FlowError::Claim(ClaimError::Busy),
            FlowError::Sensitive,
            FlowError::Empty,
            FlowError::TooLong,
            FlowError::ClipboardChanged,
            FlowError::NotPasted,
            FlowError::WriteFailed,
        ];
        for a in [
            AiError::NoKey,
            AiError::TooLong,
            AiError::Empty,
            AiError::Network,
            AiError::Timeout,
            AiError::Auth,
            AiError::RateLimited,
            AiError::Overloaded,
            AiError::Http(500),
            AiError::Refused,
            AiError::Truncated,
            AiError::BadResponse,
        ] {
            errors.push(FlowError::Ai(a));
        }
        v.extend(errors.into_iter().map(OverlayState::Failed));
        v
    }

    #[test]
    fn the_prototype_texts_for_the_main_states() {
        let en = view(Lang::En, &OverlayState::Sending { chars: 87 });
        assert_eq!(en.state, 0);
        assert_eq!(en.message, "Sending 87 characters to Anthropic…");
        assert_eq!(en.detail, "Esc to cancel");
        assert_eq!(en.icon, ViewIcon::Spinner);
        assert_eq!(en.hide_after_ms, None);
        let vi = view(Lang::Vi, &OverlayState::Sending { chars: 87 });
        assert_eq!(vi.message, "Đang gửi 87 ký tự tới Anthropic…");
        assert_eq!(vi.detail, "Esc để huỷ");

        let d = view(Lang::En, &OverlayState::Done);
        assert_eq!((d.state, d.message.as_str()), (1, "Translated"));
        assert_eq!(d.detail, "Ctrl+Z brings the original back");
        assert_eq!(d.hide_after_ms, Some(1500));
        let d = view(Lang::Vi, &OverlayState::Done);
        assert_eq!(d.message, "Đã dịch");
        assert_eq!(d.detail, "Ctrl+Z để trả lại câu gốc");

        let c = view(Lang::En, &OverlayState::Cancelled);
        assert_eq!(
            (c.state, c.message.as_str()),
            (2, "Cancelled, nothing replaced")
        );
        assert_eq!(c.detail, "");
        assert_eq!(c.hide_after_ms, Some(1500));
        assert_eq!(
            view(Lang::Vi, &OverlayState::Cancelled).message,
            "Đã huỷ, không thay gì"
        );

        let l = view(Lang::En, &OverlayState::LeftOnClipboard);
        assert_eq!(l.state, 3);
        assert_eq!(
            l.message,
            "You switched windows: the translation is on the clipboard"
        );
        assert_eq!(l.detail, "Paste it with Ctrl+V");
        let l = view(Lang::Vi, &OverlayState::LeftOnClipboard);
        assert_eq!(
            l.message,
            "Bạn đã chuyển cửa sổ: bản dịch nằm trong clipboard"
        );
        assert_eq!(l.detail, "Dán bằng Ctrl+V");

        let term = view(Lang::En, &OverlayState::Blocked(Block::Terminal));
        assert_eq!(term.message, "Not supported in terminals");
        assert_eq!(
            view(Lang::Vi, &OverlayState::Blocked(Block::Terminal)).message,
            "Không hỗ trợ trong terminal"
        );
    }

    #[test]
    fn every_state_maps_in_both_languages_with_nothing_left_to_fill() {
        for s in every_state() {
            for lang in [Lang::En, Lang::Vi] {
                let v = view(lang, &s);
                assert!(!v.message.is_empty(), "{s:?} {lang:?}");
                for text in [&v.message, &v.detail] {
                    assert!(!text.contains('{') && !text.contains('}'), "{s:?}: {text}");
                    // No app name, no exe name.
                    assert!(!text.contains(".exe"), "{s:?}: {text}");
                }
                let want_state = match s {
                    OverlayState::Sending { .. } => 0,
                    OverlayState::Done => 1,
                    OverlayState::Cancelled => 2,
                    OverlayState::LeftOnClipboard | OverlayState::Failed(FlowError::NotPasted) => 3,
                    OverlayState::Blocked(_) => 4,
                    OverlayState::Failed(_) => 5,
                };
                assert_eq!(v.state, want_state, "{s:?}");
                // The spinner only while sending.
                assert_eq!(v.icon == ViewIcon::Spinner, v.state == 0, "{s:?}");
                let hide = match v.state {
                    0 => None,
                    1 | 2 => Some(SHORT_HIDE_MS),
                    _ => Some(LONG_HIDE_MS),
                };
                assert_eq!(v.hide_after_ms, hide, "{s:?}");
                if v.state == 5 {
                    assert_eq!(v.tone, 2, "{s:?}");
                }
            }
            // The two languages differ (except the shared detail-free states, none).
            assert_ne!(view(Lang::En, &s).message, view(Lang::Vi, &s).message);
        }
    }

    #[test]
    fn each_block_and_error_group_has_its_own_text() {
        let msg = |s: OverlayState| view(Lang::En, &s).message;
        let blocks = [
            Block::Off,
            Block::NoKey,
            Block::Private,
            Block::Terminal,
            Block::UnknownApp,
            Block::ExcludedApp,
            Block::NotNormalWindow,
            Block::KeysHeld,
        ];
        let mut seen = std::collections::BTreeSet::new();
        for b in blocks {
            assert!(seen.insert(msg(OverlayState::Blocked(b))), "{b:?}");
        }
        for e in [
            FlowError::NothingCopied,
            FlowError::Sensitive,
            FlowError::Empty,
            FlowError::TooLong,
            FlowError::ClipboardChanged,
            FlowError::Ai(AiError::Auth),
            FlowError::Ai(AiError::RateLimited),
            FlowError::Ai(AiError::Overloaded),
            FlowError::Ai(AiError::Network),
            FlowError::Ai(AiError::Refused),
            FlowError::Ai(AiError::BadResponse),
        ] {
            assert!(seen.insert(msg(OverlayState::Failed(e))), "{e:?}");
        }
        // Network and timeout read the same.
        assert_eq!(
            msg(OverlayState::Failed(FlowError::Ai(AiError::Timeout))),
            msg(OverlayState::Failed(FlowError::Ai(AiError::Network)))
        );
        // The prototype icons.
        assert_eq!(
            view(Lang::En, &OverlayState::Blocked(Block::ExcludedApp)).icon,
            ViewIcon::Lock
        );
        assert_eq!(
            view(Lang::En, &OverlayState::Blocked(Block::Private)).icon,
            ViewIcon::EyeOff
        );
        assert_eq!(
            view(Lang::En, &OverlayState::Failed(FlowError::Sensitive)).icon,
            ViewIcon::Shield
        );
        assert_eq!(view(Lang::En, &OverlayState::Done).icon, ViewIcon::Check);
        assert_eq!(
            view(Lang::En, &OverlayState::Cancelled).icon,
            ViewIcon::Cross
        );
    }
}
