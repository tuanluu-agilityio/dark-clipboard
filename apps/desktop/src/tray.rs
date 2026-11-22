//! Tray icon view-model and (on Windows) the tray itself (Task 202).
//!
//! The view-model is pure so it is tested on every platform: capture state -> icon id, tooltip
//! text, menu item states, and the mode a menu action asks for. The `Tray` type (Windows only)
//! lives on the Slint main thread; menu events hop to the event loop, mode changes go to the
//! worker thread that owns `App` (never run on the UI thread). Strings come from
//! `darkclip_core::i18n`; nothing here touches clipboard content.

// The tray runs on Windows first; other platforms compile the view-model and its tests.
#![cfg_attr(not(windows), allow(dead_code))]

use darkclip_core::i18n::{t, Lang, Msg};
use darkclip_core::{CaptureMode, Timestamp};

/// "Pause until resumed": a pause that never expires by itself.
pub const UNTIL_RESUMED: Timestamp = darkclip_core::FAR_FUTURE;
const FIVE_MINUTES_MS: i64 = 5 * 60_000;
const ONE_HOUR_MS: i64 = 60 * 60_000;

/// What the tray shows, from the capture settings and health.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrayState {
    Active,
    /// Paused until this time (`UNTIL_RESUMED` = no end).
    Paused(Timestamp),
    Private,
    /// Capture is switched off (before the first-run Start).
    Off,
    /// Capture failed (monitor stopped, storage or read errors).
    Failed,
}

/// Which glyph the tray icon uses.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IconId {
    Normal,
    Paused,
    Private,
    Unavailable,
}

/// Menu entries, in menu order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MenuAction {
    Open,
    Pause5Min,
    Pause1Hour,
    PauseUntilResumed,
    Resume,
    TogglePrivate,
    Settings,
    Quit,
}

/// Enabled and checked state of the menu entries that change.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MenuModel {
    pub pause_enabled: bool,
    pub resume_enabled: bool,
    pub private_enabled: bool,
    pub private_checked: bool,
}

/// Everything the tray needs for one refresh; built on the worker, applied on the UI thread.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TrayUpdate {
    pub state: TrayState,
    pub lang: Lang,
    pub utc_offset_min: i32,
    /// Shown under the state in the tooltip, e.g. "Shortcut in use by another app" (Task 203).
    pub warning: Option<Msg>,
}

/// Derives the tray state. `failed` is `AppStatus.capture == Unavailable`; a pause whose end has
/// passed counts as active.
pub fn tray_state(
    failed: bool,
    capture_enabled: bool,
    mode: CaptureMode,
    now: Timestamp,
) -> TrayState {
    if failed {
        return TrayState::Failed;
    }
    if !capture_enabled {
        return TrayState::Off;
    }
    match mode {
        CaptureMode::Normal => TrayState::Active,
        CaptureMode::Paused { until } if until > now => TrayState::Paused(until),
        CaptureMode::Paused { .. } => TrayState::Active,
        CaptureMode::Private => TrayState::Private,
    }
}

pub fn icon_id(s: TrayState) -> IconId {
    match s {
        TrayState::Active => IconId::Normal,
        TrayState::Paused(_) => IconId::Paused,
        TrayState::Private => IconId::Private,
        TrayState::Off | TrayState::Failed => IconId::Unavailable,
    }
}

pub fn menu_model(s: TrayState) -> MenuModel {
    let usable = !matches!(s, TrayState::Off | TrayState::Failed);
    MenuModel {
        pause_enabled: usable,
        resume_enabled: matches!(s, TrayState::Paused(_)),
        private_enabled: usable,
        private_checked: s == TrayState::Private,
    }
}

/// The capture mode a menu action requests, or `None` for actions that are not mode changes.
/// `TogglePrivate` goes back to normal when already private.
pub fn mode_for(action: MenuAction, current: TrayState, now: Timestamp) -> Option<CaptureMode> {
    let pause_for = |ms: i64| CaptureMode::Paused {
        until: Timestamp(now.0.saturating_add(ms).min(darkclip_core::FAR_FUTURE.0)),
    };
    match action {
        MenuAction::Pause5Min => Some(pause_for(FIVE_MINUTES_MS)),
        MenuAction::Pause1Hour => Some(pause_for(ONE_HOUR_MS)),
        MenuAction::PauseUntilResumed => Some(CaptureMode::Paused {
            until: UNTIL_RESUMED,
        }),
        MenuAction::Resume => Some(CaptureMode::Normal),
        MenuAction::TogglePrivate if current == TrayState::Private => Some(CaptureMode::Normal),
        MenuAction::TogglePrivate => Some(CaptureMode::Private),
        MenuAction::Open | MenuAction::Settings | MenuAction::Quit => None,
    }
}

/// `HH:MM` of a Unix-millisecond time at a UTC offset in minutes.
pub fn format_clock(ts: Timestamp, utc_offset_min: i32) -> String {
    let minutes = ts.0.div_euclid(60_000) + i64::from(utc_offset_min);
    let of_day = minutes.rem_euclid(1440);
    format!("{:02}:{:02}", of_day / 60, of_day % 60)
}

/// Tooltip text, e.g. "Dark Clipboard: paused until 14:05".
pub fn tooltip(lang: Lang, s: TrayState, utc_offset_min: i32) -> String {
    match s {
        TrayState::Active => t(lang, Msg::TrayTipActive).to_string(),
        TrayState::Paused(until) if until == UNTIL_RESUMED => {
            t(lang, Msg::TrayTipPaused).to_string()
        }
        TrayState::Paused(until) => format!(
            "{} {}",
            t(lang, Msg::TrayTipPausedUntil),
            format_clock(until, utc_offset_min)
        ),
        TrayState::Private => t(lang, Msg::TrayTipPrivate).to_string(),
        TrayState::Off => t(lang, Msg::TrayTipOff).to_string(),
        TrayState::Failed => t(lang, Msg::TrayTipFailed).to_string(),
    }
}

/// The tooltip of a refresh: the state text, and the warning on a second line when there is one.
pub fn tooltip_for(u: &TrayUpdate) -> String {
    let base = tooltip(u.lang, u.state, u.utc_offset_min);
    match u.warning {
        Some(w) => format!(
            "{base}
{}",
            t(u.lang, w)
        ),
        None => base,
    }
}

/// Side of the generated icon in pixels (Windows scales it to the tray size).
pub const ICON_SIZE: usize = 32;
/// Glyph colour: the dark theme's text colour (see `theme.rs`).
const GLYPH: [u8; 3] = [0xE7, 0xE9, 0xEC];

/// Original monochrome artwork, drawn in code: a clipboard outline with a clip on top, and one
/// inner mark per state (text lines, pause bars, a dot, or a slash). RGBA, `ICON_SIZE` square.
pub fn icon_rgba(id: IconId) -> Vec<u8> {
    let mut px = vec![0u8; ICON_SIZE * ICON_SIZE * 4];
    for y in 0..ICON_SIZE {
        for x in 0..ICON_SIZE {
            let board = (6..26).contains(&x) && (6..30).contains(&y);
            let inner = (8..24).contains(&x) && (8..28).contains(&y);
            let clip = (11..21).contains(&x) && (3..9).contains(&y);
            // The glyph is 32 px, so these casts cannot lose anything.
            let (xi, yi) = (x as i32, y as i32);
            let mark = match id {
                IconId::Normal => (10..22).contains(&x) && matches!(y, 14 | 15 | 19 | 20 | 24 | 25),
                IconId::Paused => {
                    ((11..14).contains(&x) || (18..21).contains(&x)) && (13..25).contains(&y)
                }
                IconId::Private => {
                    let (dx, dy) = (xi - 16, yi - 19);
                    dx * dx + dy * dy <= 36
                }
                IconId::Unavailable => (xi - yi).abs() <= 1 && (3..30).contains(&x),
            };
            if (board && !inner) || clip || mark {
                let i = (y * ICON_SIZE + x) * 4;
                px[i..i + 4].copy_from_slice(&[GLYPH[0], GLYPH[1], GLYPH[2], 0xFF]);
            }
        }
    }
    px
}

#[cfg(windows)]
pub use win::Tray;

/// The tray icon itself (Windows). Created and used only on the Slint main thread.
#[cfg(windows)]
mod win {
    use super::{
        icon_id, icon_rgba, menu_model, tooltip_for, IconId, MenuAction, TrayUpdate, ICON_SIZE,
    };
    use darkclip_core::i18n::{t, Msg};
    use std::sync::Arc;
    use tray_icon::menu::{CheckMenuItem, Menu, MenuEvent, MenuId, MenuItem, Submenu};
    use tray_icon::{
        Icon, MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent,
    };

    /// Called for every menu choice and tray click, on whatever thread tray-icon uses: the
    /// callback must hop to the Slint event loop itself.
    pub type OnAction = Arc<dyn Fn(MenuAction) + Send + Sync>;

    pub struct Tray {
        icon: TrayIcon,
        shown: Option<IconId>,
        open: MenuItem,
        pause: Submenu,
        pause_5: MenuItem,
        pause_1h: MenuItem,
        pause_until: MenuItem,
        resume: MenuItem,
        private: CheckMenuItem,
        settings: MenuItem,
        quit: MenuItem,
    }

    fn icon_for(id: IconId) -> Option<Icon> {
        let size = u32::try_from(ICON_SIZE).ok()?;
        Icon::from_rgba(icon_rgba(id), size, size).ok()
    }

    impl Tray {
        /// Builds the tray with its menu. `None` when the shell refuses the icon.
        pub fn new(first: TrayUpdate, on_action: OnAction) -> Option<Tray> {
            let l = first.lang;
            let open = MenuItem::new(t(l, Msg::TrayOpen), true, None);
            let pause_5 = MenuItem::new(t(l, Msg::TrayPause5Min), true, None);
            let pause_1h = MenuItem::new(t(l, Msg::TrayPause1Hour), true, None);
            let pause_until = MenuItem::new(t(l, Msg::TrayPauseUntilResumed), true, None);
            let pause = Submenu::with_items(
                t(l, Msg::TrayPause),
                true,
                &[&pause_5, &pause_1h, &pause_until],
            )
            .ok()?;
            let resume = MenuItem::new(t(l, Msg::TrayResume), false, None);
            let private = CheckMenuItem::new(t(l, Msg::TrayPrivate), true, false, None);
            let settings = MenuItem::new(t(l, Msg::TraySettings), true, None);
            let quit = MenuItem::new(t(l, Msg::TrayQuit), true, None);
            let sep1 = tray_icon::menu::PredefinedMenuItem::separator();
            let sep2 = tray_icon::menu::PredefinedMenuItem::separator();
            let menu = Menu::with_items(&[
                &open, &sep1, &pause, &resume, &private, &sep2, &settings, &quit,
            ])
            .ok()?;

            let routes: Vec<(MenuId, MenuAction)> = vec![
                (open.id().clone(), MenuAction::Open),
                (pause_5.id().clone(), MenuAction::Pause5Min),
                (pause_1h.id().clone(), MenuAction::Pause1Hour),
                (pause_until.id().clone(), MenuAction::PauseUntilResumed),
                (resume.id().clone(), MenuAction::Resume),
                (private.id().clone(), MenuAction::TogglePrivate),
                (settings.id().clone(), MenuAction::Settings),
                (quit.id().clone(), MenuAction::Quit),
            ];
            let id = icon_id(first.state);
            let built = TrayIconBuilder::new()
                .with_menu(Box::new(menu))
                .with_menu_on_left_click(false)
                .with_tooltip(tooltip_for(&first))
                .with_icon(icon_for(id)?)
                .build()
                .ok()?;
            // Handlers are installed only now that the icon exists, so a failed build leaves
            // none behind.
            let menu_cb = Arc::clone(&on_action);
            MenuEvent::set_event_handler(Some(move |e: MenuEvent| {
                if let Some((_, action)) = routes.iter().find(|(id, _)| *id == e.id) {
                    menu_cb(*action);
                }
            }));
            let click_cb = on_action;
            TrayIconEvent::set_event_handler(Some(move |e: TrayIconEvent| {
                if let TrayIconEvent::Click {
                    button: MouseButton::Left,
                    button_state: MouseButtonState::Up,
                    ..
                } = e
                {
                    click_cb(MenuAction::Open);
                }
            }));
            let mut tray = Tray {
                icon: built,
                shown: Some(id),
                open,
                pause,
                pause_5,
                pause_1h,
                pause_until,
                resume,
                private,
                settings,
                quit,
            };
            tray.update(first);
            Some(tray)
        }

        /// Applies a new state: icon (only when it changes), tooltip, menu texts and states.
        pub fn update(&mut self, u: TrayUpdate) {
            let l = u.lang;
            let id = icon_id(u.state);
            if self.shown != Some(id) {
                if let Some(icon) = icon_for(id) {
                    if self.icon.set_icon(Some(icon)).is_ok() {
                        self.shown = Some(id);
                    }
                }
            }
            let _ = self.icon.set_tooltip(Some(tooltip_for(&u)));
            self.open.set_text(t(l, Msg::TrayOpen));
            self.pause.set_text(t(l, Msg::TrayPause));
            self.pause_5.set_text(t(l, Msg::TrayPause5Min));
            self.pause_1h.set_text(t(l, Msg::TrayPause1Hour));
            self.pause_until.set_text(t(l, Msg::TrayPauseUntilResumed));
            self.resume.set_text(t(l, Msg::TrayResume));
            self.private.set_text(t(l, Msg::TrayPrivate));
            self.settings.set_text(t(l, Msg::TraySettings));
            self.quit.set_text(t(l, Msg::TrayQuit));
            let m = menu_model(u.state);
            self.pause.set_enabled(m.pause_enabled);
            self.resume.set_enabled(m.resume_enabled);
            self.private.set_enabled(m.private_enabled);
            self.private.set_checked(m.private_checked);
        }
    }

    impl Drop for Tray {
        fn drop(&mut self) {
            // The handlers hold the action callback; release it with the icon.
            MenuEvent::set_event_handler(None::<fn(MenuEvent)>);
            TrayIconEvent::set_event_handler(None::<fn(TrayIconEvent)>);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOW: Timestamp = Timestamp(1_000_000_000);
    const LATER: Timestamp = Timestamp(1_000_000_000 + 600_000);

    fn state(failed: bool, enabled: bool, mode: CaptureMode) -> TrayState {
        tray_state(failed, enabled, mode, NOW)
    }

    fn flags(m: MenuModel) -> (bool, bool, bool, bool) {
        (
            m.pause_enabled,
            m.resume_enabled,
            m.private_enabled,
            m.private_checked,
        )
    }

    #[test]
    fn state_follows_mode_and_health() {
        assert_eq!(state(false, true, CaptureMode::Normal), TrayState::Active);
        assert_eq!(
            state(false, true, CaptureMode::Paused { until: LATER }),
            TrayState::Paused(LATER)
        );
        assert_eq!(state(false, true, CaptureMode::Private), TrayState::Private);
        assert_eq!(state(false, false, CaptureMode::Normal), TrayState::Off);
        assert_eq!(state(true, true, CaptureMode::Normal), TrayState::Failed);
        // Failure wins over everything; "off" wins over the mode.
        assert_eq!(state(true, false, CaptureMode::Private), TrayState::Failed);
        assert_eq!(state(false, false, CaptureMode::Private), TrayState::Off);
    }

    #[test]
    fn an_expired_pause_is_active() {
        for until in [NOW, Timestamp(NOW.0 - 1)] {
            assert_eq!(
                state(false, true, CaptureMode::Paused { until }),
                TrayState::Active
            );
        }
    }

    #[test]
    fn every_state_has_the_expected_icon() {
        assert_eq!(icon_id(TrayState::Active), IconId::Normal);
        assert_eq!(icon_id(TrayState::Paused(LATER)), IconId::Paused);
        assert_eq!(icon_id(TrayState::Private), IconId::Private);
        assert_eq!(icon_id(TrayState::Off), IconId::Unavailable);
        assert_eq!(icon_id(TrayState::Failed), IconId::Unavailable);
    }

    #[test]
    fn icons_are_distinct_monochrome_and_not_empty() {
        let ids = [
            IconId::Normal,
            IconId::Paused,
            IconId::Private,
            IconId::Unavailable,
        ];
        let icons: Vec<Vec<u8>> = ids.iter().map(|i| icon_rgba(*i)).collect();
        for (i, a) in icons.iter().enumerate() {
            assert_eq!(a.len(), ICON_SIZE * ICON_SIZE * 4);
            let visible: Vec<&[u8]> = a.chunks(4).filter(|p| p[3] != 0).collect();
            assert!(visible.len() > 50, "icon {i} is nearly empty");
            assert!(
                visible.len() < ICON_SIZE * ICON_SIZE / 2,
                "icon {i} is a blob"
            );
            assert!(visible.iter().all(|p| p[..3] == GLYPH && p[3] == 0xFF));
            for b in icons.iter().skip(i + 1) {
                assert_ne!(a, b, "two states share one icon");
            }
        }
    }

    #[test]
    fn menu_states_per_mode() {
        assert_eq!(
            flags(menu_model(TrayState::Active)),
            (true, false, true, false)
        );
        assert_eq!(
            flags(menu_model(TrayState::Paused(LATER))),
            (true, true, true, false)
        );
        assert_eq!(
            flags(menu_model(TrayState::Private)),
            (true, false, true, true)
        );
        for s in [TrayState::Off, TrayState::Failed] {
            assert_eq!(flags(menu_model(s)), (false, false, false, false));
        }
    }

    #[test]
    fn actions_map_to_modes() {
        let go = |a, s| mode_for(a, s, NOW);
        assert_eq!(
            go(MenuAction::Pause5Min, TrayState::Active),
            Some(CaptureMode::Paused {
                until: Timestamp(NOW.0 + 300_000)
            })
        );
        assert_eq!(
            go(MenuAction::Pause1Hour, TrayState::Active),
            Some(CaptureMode::Paused {
                until: Timestamp(NOW.0 + 3_600_000)
            })
        );
        assert_eq!(
            go(MenuAction::PauseUntilResumed, TrayState::Active),
            Some(CaptureMode::Paused {
                until: UNTIL_RESUMED
            })
        );
        assert_eq!(
            go(MenuAction::Resume, TrayState::Paused(LATER)),
            Some(CaptureMode::Normal)
        );
        assert_eq!(
            go(MenuAction::TogglePrivate, TrayState::Active),
            Some(CaptureMode::Private)
        );
        assert_eq!(
            go(MenuAction::TogglePrivate, TrayState::Paused(LATER)),
            Some(CaptureMode::Private)
        );
        assert_eq!(
            go(MenuAction::TogglePrivate, TrayState::Private),
            Some(CaptureMode::Normal)
        );
        for a in [MenuAction::Open, MenuAction::Settings, MenuAction::Quit] {
            assert_eq!(go(a, TrayState::Active), None);
        }
    }

    #[test]
    fn until_resumed_can_be_saved() {
        // Settings store the time as RFC 3339; the value must be one that can be read back.
        assert_eq!(UNTIL_RESUMED, darkclip_core::FAR_FUTURE);
        assert_eq!(
            Timestamp::parse_rfc3339(&UNTIL_RESUMED.to_rfc3339()),
            Ok(UNTIL_RESUMED)
        );
    }

    #[test]
    fn pause_end_never_overflows() {
        let late = Timestamp(i64::MAX - 1);
        assert_eq!(
            mode_for(MenuAction::Pause1Hour, TrayState::Active, late),
            Some(CaptureMode::Paused {
                until: darkclip_core::FAR_FUTURE
            })
        );
    }

    #[test]
    fn clock_text_uses_the_offset() {
        // 1970-01-01 14:05 UTC.
        let ts = Timestamp((14 * 60 + 5) * 60_000);
        assert_eq!(format_clock(ts, 0), "14:05");
        assert_eq!(format_clock(ts, 420), "21:05"); // UTC+7
        assert_eq!(format_clock(ts, -(14 * 60 + 5)), "00:00");
        assert_eq!(format_clock(ts, 600), "00:05"); // wraps past midnight
        assert_eq!(format_clock(Timestamp(-60_000), 0), "23:59"); // before the epoch
    }

    #[test]
    fn a_warning_goes_on_a_second_tooltip_line() {
        let mut u = TrayUpdate {
            state: TrayState::Active,
            lang: Lang::En,
            utc_offset_min: 0,
            warning: None,
        };
        assert_eq!(tooltip_for(&u), "Dark Clipboard: capturing");
        u.warning = Some(Msg::ShortcutInUse);
        assert_eq!(
            tooltip_for(&u),
            "Dark Clipboard: capturing
Shortcut in use by another app"
        );
        u.lang = Lang::Vi;
        assert!(tooltip_for(&u).ends_with("Phím tắt đang được ứng dụng khác dùng"));
    }

    #[test]
    fn every_tooltip_fits_the_windows_limit() {
        let warnings = [
            None,
            Some(Msg::ShortcutInUse),
            Some(Msg::ShortcutRegisterFailed),
            Some(Msg::ShortcutErrEmpty),
            Some(Msg::ShortcutErrNoKey),
            Some(Msg::ShortcutErrManyKeys),
            Some(Msg::ShortcutErrNoModifier),
            Some(Msg::ShortcutErrDuplicate),
            Some(Msg::ShortcutErrUnknownKey),
        ];
        let states = [
            TrayState::Active,
            TrayState::Paused(Timestamp(60_000)),
            TrayState::Paused(UNTIL_RESUMED),
            TrayState::Private,
            TrayState::Off,
            TrayState::Failed,
        ];
        for lang in [Lang::En, Lang::Vi] {
            for state in states {
                for warning in warnings {
                    let u = TrayUpdate {
                        state,
                        lang,
                        utc_offset_min: 0,
                        warning,
                    };
                    let n = tooltip_for(&u).encode_utf16().count();
                    assert!(n <= 127, "{lang:?} {state:?} {warning:?}: {n}");
                }
            }
        }
    }

    #[test]
    fn tooltips_name_the_state_in_both_languages() {
        let until = Timestamp((14 * 60 + 5) * 60_000);
        assert_eq!(
            tooltip(Lang::En, TrayState::Active, 0),
            "Dark Clipboard: capturing"
        );
        assert_eq!(
            tooltip(Lang::En, TrayState::Paused(until), 0),
            "Dark Clipboard: paused until 14:05"
        );
        assert_eq!(
            tooltip(Lang::Vi, TrayState::Paused(until), 420),
            "Dark Clipboard: tạm dừng đến 21:05"
        );
        assert_eq!(
            tooltip(Lang::En, TrayState::Paused(UNTIL_RESUMED), 0),
            "Dark Clipboard: paused"
        );
        assert_eq!(
            tooltip(Lang::En, TrayState::Private, 0),
            "Dark Clipboard: private"
        );
        let all = [
            TrayState::Active,
            TrayState::Paused(until),
            TrayState::Private,
            TrayState::Off,
            TrayState::Failed,
        ];
        for lang in [Lang::En, Lang::Vi] {
            for (i, a) in all.iter().enumerate() {
                for b in all.iter().skip(i + 1) {
                    assert_ne!(tooltip(lang, *a, 0), tooltip(lang, *b, 0));
                }
            }
        }
    }
}
