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

/// Which tray image is shown (the same icon, plus a state badge except for `Normal`).
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
    /// "History...": the history window (Task 211).
    History,
    Pause5Min,
    Pause1Hour,
    PauseUntilResumed,
    Resume,
    TogglePrivate,
    Settings,
    /// "Start capturing...": opens the welcome (the consent path); only while capture is off.
    StartCapturing,
    Quit,
}

/// Enabled and checked state of the menu entries that change.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MenuModel {
    pub pause_enabled: bool,
    pub resume_enabled: bool,
    pub private_enabled: bool,
    pub private_checked: bool,
    /// The "Start capturing..." entry is in the menu (capture is off).
    pub start_visible: bool,
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
        TrayState::Off => IconId::Paused,
        TrayState::Failed => IconId::Unavailable,
    }
}

pub fn menu_model(s: TrayState) -> MenuModel {
    let usable = !matches!(s, TrayState::Off | TrayState::Failed);
    MenuModel {
        pause_enabled: usable,
        resume_enabled: matches!(s, TrayState::Paused(_)),
        private_enabled: usable,
        private_checked: s == TrayState::Private,
        start_visible: s == TrayState::Off,
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
        MenuAction::Open
        | MenuAction::History
        | MenuAction::Settings
        | MenuAction::StartCapturing
        | MenuAction::Quit => None,
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

/// Sizes of the pre-rendered tray images (`res/tray/`, made by `res/make_icon.py`).
pub const TRAY_SIZES: [u32; 4] = [16, 20, 24, 32];

/// The pre-rendered size closest to the system tray size `px` (`SM_CXSMICON`): a tie goes to
/// the larger size; below 16 and above 32 clamp.
pub fn nearest_tray_size(px: u32) -> u32 {
    let mut best = TRAY_SIZES[0];
    for size in TRAY_SIZES {
        // `<=`: on a tie the later (larger) size wins.
        if size.abs_diff(px) <= best.abs_diff(px) {
            best = size;
        }
    }
    best
}

/// One tray image: straight RGBA, `size` x `size` pixels.
#[derive(Debug, Clone, Copy)]
pub struct TrayImage {
    pub size: u32,
    pub rgba: &'static [u8],
}

macro_rules! tray_rgba {
    ($state:literal, $size:literal) => {
        include_bytes!(concat!("../res/tray/", $state, "-", $size, ".rgba")).as_slice()
    };
}

/// Raw RGBA made by the generator, embedded as is: no decoder and no extra crate needed, and a
/// bad image cannot appear at run time (the length is checked in `tray_image`).
const NORMAL: [&[u8]; 4] = [
    tray_rgba!("normal", 16),
    tray_rgba!("normal", 20),
    tray_rgba!("normal", 24),
    tray_rgba!("normal", 32),
];
const PAUSED: [&[u8]; 4] = [
    tray_rgba!("paused", 16),
    tray_rgba!("paused", 20),
    tray_rgba!("paused", 24),
    tray_rgba!("paused", 32),
];
const PRIVATE: [&[u8]; 4] = [
    tray_rgba!("private", 16),
    tray_rgba!("private", 20),
    tray_rgba!("private", 24),
    tray_rgba!("private", 32),
];
const UNAVAILABLE: [&[u8]; 4] = [
    tray_rgba!("unavailable", 16),
    tray_rgba!("unavailable", 20),
    tray_rgba!("unavailable", 24),
    tray_rgba!("unavailable", 32),
];

fn table(id: IconId) -> &'static [&'static [u8]; 4] {
    match id {
        IconId::Normal => &NORMAL,
        IconId::Paused => &PAUSED,
        IconId::Private => &PRIVATE,
        IconId::Unavailable => &UNAVAILABLE,
    }
}

fn checked(id: IconId, slot: usize) -> Option<TrayImage> {
    let size = *TRAY_SIZES.get(slot)?;
    let rgba = *table(id).get(slot)?;
    (rgba.len() == (size as usize) * (size as usize) * 4).then_some(TrayImage { size, rgba })
}

/// The image for a state at the pre-rendered size nearest to `px`. A missing or wrong-sized
/// image falls back to the plain icon at the same size; `None` only if that is bad too.
pub fn tray_image(id: IconId, px: u32) -> Option<TrayImage> {
    let size = nearest_tray_size(px);
    let slot = TRAY_SIZES.iter().position(|s| *s == size)?;
    checked(id, slot).or_else(|| checked(IconId::Normal, slot))
}

#[cfg(windows)]
pub use win::Tray;

/// The tray icon itself (Windows). Created and used only on the Slint main thread.
#[cfg(windows)]
mod win {
    use super::{icon_id, menu_model, tooltip_for, tray_image, IconId, MenuAction, TrayUpdate};
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
        menu: Menu,
        start: MenuItem,
        start_shown: bool,
        icon: TrayIcon,
        shown: Option<IconId>,
        open: MenuItem,
        history: MenuItem,
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
        let img = tray_image(id, crate::winsys::small_icon_px())?;
        Icon::from_rgba(img.rgba.to_vec(), img.size, img.size).ok()
    }

    impl Tray {
        /// Builds the tray with its menu. `None` when the shell refuses the icon.
        pub fn new(first: TrayUpdate, on_action: OnAction) -> Option<Tray> {
            let l = first.lang;
            let open = MenuItem::new(t(l, Msg::TrayOpen), true, None);
            let history = MenuItem::new(t(l, Msg::TrayHistory), true, None);
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
            let start = MenuItem::new(t(l, Msg::TrayStartCapturing), true, None);
            let sep1 = tray_icon::menu::PredefinedMenuItem::separator();
            let sep2 = tray_icon::menu::PredefinedMenuItem::separator();
            let menu = Menu::with_items(&[
                &open, &history, &sep1, &pause, &resume, &private, &sep2, &settings, &quit,
            ])
            .ok()?;

            let routes: Vec<(MenuId, MenuAction)> = vec![
                (open.id().clone(), MenuAction::Open),
                (history.id().clone(), MenuAction::History),
                (pause_5.id().clone(), MenuAction::Pause5Min),
                (pause_1h.id().clone(), MenuAction::Pause1Hour),
                (pause_until.id().clone(), MenuAction::PauseUntilResumed),
                (resume.id().clone(), MenuAction::Resume),
                (private.id().clone(), MenuAction::TogglePrivate),
                (settings.id().clone(), MenuAction::Settings),
                (quit.id().clone(), MenuAction::Quit),
                (start.id().clone(), MenuAction::StartCapturing),
            ];
            let id = icon_id(first.state);
            let built = TrayIconBuilder::new()
                .with_menu(Box::new(menu.clone()))
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
                menu,
                start,
                start_shown: false,
                icon: built,
                shown: Some(id),
                open,
                history,
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
            self.history.set_text(t(l, Msg::TrayHistory));
            self.pause.set_text(t(l, Msg::TrayPause));
            self.pause_5.set_text(t(l, Msg::TrayPause5Min));
            self.pause_1h.set_text(t(l, Msg::TrayPause1Hour));
            self.pause_until.set_text(t(l, Msg::TrayPauseUntilResumed));
            self.resume.set_text(t(l, Msg::TrayResume));
            self.private.set_text(t(l, Msg::TrayPrivate));
            self.settings.set_text(t(l, Msg::TraySettings));
            self.quit.set_text(t(l, Msg::TrayQuit));
            self.start.set_text(t(l, Msg::TrayStartCapturing));
            let m = menu_model(u.state);
            // Shown only while capture is off: the first entry, above Open.
            if m.start_visible != self.start_shown {
                let done = if m.start_visible {
                    self.menu.insert(&self.start, 0).is_ok()
                } else {
                    self.menu.remove(&self.start).is_ok()
                };
                if done {
                    self.start_shown = m.start_visible;
                }
            }
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
        // Capture switched off has its own glyph, not the failure one.
        assert_eq!(icon_id(TrayState::Off), IconId::Paused);
        assert_ne!(icon_id(TrayState::Off), icon_id(TrayState::Failed));
        assert_eq!(icon_id(TrayState::Failed), IconId::Unavailable);
    }

    const ALL_IDS: [IconId; 4] = [
        IconId::Normal,
        IconId::Paused,
        IconId::Private,
        IconId::Unavailable,
    ];

    #[test]
    fn nearest_tray_size_picks_the_closest_and_ties_go_up() {
        for exact in TRAY_SIZES {
            assert_eq!(nearest_tray_size(exact), exact);
        }
        assert_eq!(nearest_tray_size(17), 16);
        assert_eq!(nearest_tray_size(18), 20); // tie 16/20 goes up
        assert_eq!(nearest_tray_size(19), 20);
        assert_eq!(nearest_tray_size(22), 24); // tie 20/24 goes up
        assert_eq!(nearest_tray_size(26), 24);
        assert_eq!(nearest_tray_size(28), 32); // tie 24/32 goes up
        assert_eq!(nearest_tray_size(30), 32);
    }

    #[test]
    fn nearest_tray_size_clamps() {
        assert_eq!(nearest_tray_size(0), 16);
        assert_eq!(nearest_tray_size(8), 16);
        assert_eq!(nearest_tray_size(15), 16);
        assert_eq!(nearest_tray_size(33), 32);
        assert_eq!(nearest_tray_size(48), 32);
        assert_eq!(nearest_tray_size(u32::MAX), 32);
    }

    #[test]
    fn every_state_has_an_image_of_every_size() {
        for id in ALL_IDS {
            for size in TRAY_SIZES {
                let img = tray_image(id, size).expect("embedded image");
                assert_eq!(img.size, size);
                assert_eq!(img.rgba.len(), (size * size * 4) as usize, "{id:?} {size}");
                let visible = img.rgba.chunks(4).filter(|p| p[3] != 0).count();
                assert!(
                    visible > (size * size / 2) as usize,
                    "{id:?} {size} is empty"
                );
            }
        }
    }

    #[test]
    fn states_look_different_and_normal_has_no_badge() {
        for size in TRAY_SIZES {
            for (i, a) in ALL_IDS.iter().enumerate() {
                for b in ALL_IDS.iter().skip(i + 1) {
                    let (x, y) = (tray_image(*a, size), tray_image(*b, size));
                    assert_ne!(
                        x.map(|v| v.rgba),
                        y.map(|v| v.rgba),
                        "{a:?} = {b:?} at {size}"
                    );
                }
            }
        }
    }

    #[test]
    fn an_unknown_size_falls_back_to_the_nearest_image() {
        let img = tray_image(IconId::Private, 21).expect("image");
        assert_eq!(img.size, 20);
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
    fn start_capturing_is_offered_only_while_capture_is_off() {
        assert!(menu_model(TrayState::Off).start_visible);
        for s in [
            TrayState::Active,
            TrayState::Paused(LATER),
            TrayState::Private,
            TrayState::Failed,
        ] {
            assert!(!menu_model(s).start_visible, "{s:?}");
        }
        // It opens the welcome (the consent path); it never changes the capture mode itself.
        assert_eq!(
            mode_for(MenuAction::StartCapturing, TrayState::Off, NOW),
            None
        );
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
        for a in [
            MenuAction::Open,
            MenuAction::History,
            MenuAction::Settings,
            MenuAction::Quit,
        ] {
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
