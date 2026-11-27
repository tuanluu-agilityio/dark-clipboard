//! Task 606: Settings → Translate, headless (GC-19): software-rendered `MinimalSoftwareWindow`s,
//! no OS window, no real input, a fake key store. Pointer clicks (`dispatch_event`) reach the
//! real section components of `ui/settings_parts.slint`, wired to the real rules of
//! `src/prefs/translate.rs` the way `src/ui/settings_host.rs` wires them:
//! - clicking the Enable switch with no consent opens the consent dialog and leaves it off;
//! - Cancel keeps it off; Turn on sets it on and the consent to 1;
//! - Save with an empty or too-long key shows the error and stores nothing;
//! - Save with a key stores it, clears the field and shows only the mask; Remove clears it;
//! - in the real settings window, Ctrl+6 picks About (the key map of `src/ui/settings_keys.rs`).
//!
//! `set_platform` works once per process, so every case lives in the one `#[test]` below.

#![allow(dead_code)]

// `#[path]` only at the top level (rustfmt on Linux/macOS; see tests/shortcuts.rs, F-37).
#[path = "../src/popup/keys.rs"]
pub mod popup_keys;
#[path = "../src/prefs/translate.rs"]
mod translate_prefs;
mod popup {
    pub use super::popup_keys as keys;
}
#[path = "../src/ui/settings_keys.rs"]
mod settings_keys;

use darkclip_core::i18n::{t, Lang, Msg};
use darkclip_platform::fake::FakeSecretStore;
use darkclip_platform::{SecretStore, ANTHROPIC_KEY_TARGET};
use popup::keys::Mods;
use settings_keys::SettingsKey;
use slint::platform::software_renderer::{MinimalSoftwareWindow, RepaintBufferType};
use slint::platform::{Platform, PointerEventButton, WindowAdapter, WindowEvent};
use slint::{ComponentHandle, LogicalPosition, PhysicalSize, SharedString};
use std::cell::{Cell, RefCell};
use std::rc::Rc;
use translate_prefs as tr;

slint::slint! {
    #[edition = "1.19"]
    import { TrEnableCard, TrKeyCard, TrConsentDialog } from "ui/settings_parts.slint";
    export { SettingsWindow } from "ui/settings.slint";

    export component Harness inherits Window {
        width: 600px;
        height: 520px;
        in-out property <bool> enabled;
        in property <bool> on;
        in property <bool> editing: true;
        in property <string> key-status;
        in property <string> key-error;
        in-out property <string> key-input;
        in property <bool> consent-open;
        out property <length> enable-y: e.y;
        out property <length> enable-h: e.height;
        out property <length> key-y: k.y;
        out property <length> key-h: k.height;
        callback toggled(bool);
        callback save-key(string);
        callback replace-key();
        callback remove-key();
        callback test();
        callback confirmed();
        callback cancelled();

        e := TrEnableCard {
            x: 0px;
            y: 0px;
            width: 600px;
            checked <=> root.enabled;
            toggled(v) => {
                root.toggled(v);
            }
        }
        k := TrKeyCard {
            x: 0px;
            y: 140px;
            width: 600px;
            on: root.on;
            editing: root.editing;
            key-status: root.key-status;
            key-error: root.key-error;
            key-input <=> root.key-input;
            save-key(v) => {
                root.save-key(v);
            }
            replace-key => {
                root.replace-key();
            }
            remove-key => {
                root.remove-key();
            }
            test => {
                root.test();
            }
        }
        TrConsentDialog {
            x: 0px;
            y: 0px;
            width: 600px;
            height: 520px;
            open: root.consent-open;
            confirmed => {
                root.confirmed();
            }
            cancelled => {
                root.cancelled();
            }
        }
    }
}

/// A new software window per component (the harness, then the settings window).
#[derive(Default)]
struct TestPlatform {
    windows: RefCell<Vec<Rc<MinimalSoftwareWindow>>>,
}

impl Platform for TestPlatform {
    fn create_window_adapter(&self) -> Result<Rc<dyn WindowAdapter>, slint::PlatformError> {
        let w = MinimalSoftwareWindow::new(RepaintBufferType::NewBuffer);
        w.set_size(PhysicalSize::new(600, 520));
        self.windows.borrow_mut().push(w.clone());
        Ok(w)
    }
}

fn click(w: &Rc<MinimalSoftwareWindow>, x: f32, y: f32) {
    let pos = LogicalPosition::new(x, y);
    w.window()
        .dispatch_event(WindowEvent::PointerMoved { position: pos });
    w.window().dispatch_event(WindowEvent::PointerPressed {
        position: pos,
        button: PointerEventButton::Left,
    });
    w.window().dispatch_event(WindowEvent::PointerReleased {
        position: pos,
        button: PointerEventButton::Left,
    });
    slint::platform::update_timers_and_animations();
}

/// Clicks column by column (from the right when `from_right`), top to bottom, inside the band
/// `y0..y1`, until `hit()` turns true. Returns where it hit.
fn scan(
    w: &Rc<MinimalSoftwareWindow>,
    y0: f32,
    y1: f32,
    from_right: bool,
    hit: impl Fn() -> bool,
) -> Option<(f32, f32)> {
    let xs: Vec<i32> = if from_right {
        (0..600).rev().step_by(3).collect()
    } else {
        (0..600).step_by(3).collect()
    };
    for x in xs {
        let mut y = y0;
        while y < y1 {
            click(w, x as f32, y);
            if hit() {
                return Some((x as f32, y));
            }
            y += 3.0;
        }
    }
    None
}

/// What the harness callbacks did, wired like `settings_host.rs`.
#[derive(Default)]
struct State {
    consent: Cell<u32>,
    dirty: Cell<u32>,
    toggles: Cell<u32>,
    saves: Cell<u32>,
    removes: Cell<u32>,
    replaces: Cell<u32>,
    cancels: Cell<u32>,
    confirms: Cell<u32>,
}

fn synthetic_key() -> String {
    // Runtime-assembled (GC-09): never a key-shaped literal.
    ["sk-", "ant-", "api03-", &"q".repeat(40), "Zk9w"].concat()
}

fn key_down(w: &Rc<MinimalSoftwareWindow>, text: SharedString) {
    w.window()
        .dispatch_event(WindowEvent::KeyPressed { text: text.clone() });
}

fn key_up(w: &Rc<MinimalSoftwareWindow>, text: SharedString) {
    w.window().dispatch_event(WindowEvent::KeyReleased { text });
}

#[test]
fn translate_section_clicks_consent_key_entry_and_ctrl_6() {
    let platform = Rc::new(TestPlatform::default());
    struct Shared(Rc<TestPlatform>);
    impl Platform for Shared {
        fn create_window_adapter(&self) -> Result<Rc<dyn WindowAdapter>, slint::PlatformError> {
            self.0.create_window_adapter()
        }
    }
    slint::platform::set_platform(Box::new(Shared(platform.clone()))).expect("platform set once");

    let lang = Lang::En;
    let store = Rc::new(FakeSecretStore::default());
    let st = Rc::new(State::default());
    let ui = Harness::new().unwrap();
    let window = platform.windows.borrow()[0].clone();
    ui.set_key_status(tr::key_status(lang, None).into());

    {
        let (s, weak) = (st.clone(), ui.as_weak());
        ui.on_toggled(move |on| {
            s.toggles.set(s.toggles.get() + 1);
            let Some(ui) = weak.upgrade() else { return };
            match tr::switch_step(on, s.consent.get()) {
                tr::SwitchStep::AskConsent => {
                    ui.set_enabled(false);
                    ui.set_consent_open(true);
                }
                tr::SwitchStep::On | tr::SwitchStep::Off => s.dirty.set(s.dirty.get() + 1),
            }
        });
        let (s, weak) = (st.clone(), ui.as_weak());
        ui.on_confirmed(move || {
            s.confirms.set(s.confirms.get() + 1);
            let Some(ui) = weak.upgrade() else { return };
            s.consent.set(s.consent.get().max(tr::CONSENT_VERSION));
            ui.set_consent_open(false);
            ui.set_enabled(true);
            s.dirty.set(s.dirty.get() + 1);
        });
        let (s, weak) = (st.clone(), ui.as_weak());
        ui.on_cancelled(move || {
            s.cancels.set(s.cancels.get() + 1);
            let Some(ui) = weak.upgrade() else { return };
            ui.set_consent_open(false);
            ui.set_enabled(false);
        });
        let (s, weak, keys) = (st.clone(), ui.as_weak(), store.clone());
        ui.on_save_key(move |typed| {
            s.saves.set(s.saves.get() + 1);
            let Some(ui) = weak.upgrade() else { return };
            ui.set_key_input(SharedString::new());
            match tr::save_key(keys.as_ref(), typed.to_string()) {
                Ok(mask) => {
                    ui.set_key_error(SharedString::new());
                    ui.set_key_status(tr::key_status(lang, Some(&mask)).into());
                    ui.set_editing(false);
                }
                Err(e) => ui.set_key_error(tr::key_error_text(lang, e).into()),
            }
        });
        let (s, weak, keys) = (st.clone(), ui.as_weak(), store.clone());
        ui.on_remove_key(move || {
            s.removes.set(s.removes.get() + 1);
            let Some(ui) = weak.upgrade() else { return };
            if tr::remove_key(keys.as_ref()).is_ok() {
                ui.set_key_status(tr::key_status(lang, None).into());
                ui.set_editing(true);
            }
        });
        let s = st.clone();
        ui.on_replace_key(move || s.replaces.set(s.replaces.get() + 1));
    }
    ui.show().unwrap();
    slint::platform::update_timers_and_animations();

    let (ey, eh) = (ui.get_enable_y(), ui.get_enable_h());
    assert!(eh > 20.0, "enable card height {eh}");

    // 1. The Enable switch, no consent yet: the dialog opens, the switch stays off.
    let hit = scan(&window, ey, ey + eh, true, || st.toggles.get() > 0);
    assert!(hit.is_some(), "no click reached the Enable switch");
    let (sx, sy) = hit.unwrap();
    assert_eq!(st.toggles.get(), 1);
    assert!(ui.get_consent_open(), "the consent dialog did not open");
    assert!(!ui.get_enabled());
    assert_eq!(st.consent.get(), 0);
    assert_eq!(st.dirty.get(), 0);
    // The dialog is modal: the switch behind it cannot be clicked.
    click(&window, sx, sy);
    assert_eq!(st.toggles.get(), 1);

    // 2. Cancel (the left button) keeps it off.
    assert!(scan(&window, 0.0, 520.0, false, || st.cancels.get() > 0).is_some());
    assert_eq!((st.cancels.get(), st.confirms.get()), (1, 0));
    assert!(!ui.get_consent_open());
    assert!(!ui.get_enabled());
    assert_eq!(st.consent.get(), 0);

    // 3. Again, then Turn on (the right button): on, consent 1.
    click(&window, sx, sy);
    assert_eq!(st.toggles.get(), 2);
    assert!(ui.get_consent_open());
    assert!(scan(&window, 0.0, 520.0, true, || st.confirms.get() > 0).is_some());
    assert_eq!((st.cancels.get(), st.confirms.get()), (1, 1));
    assert!(ui.get_enabled());
    assert!(!ui.get_consent_open());
    assert_eq!(st.consent.get(), 1);
    assert_eq!(st.dirty.get(), 1);
    // Turning it off and on again needs no second dialog.
    click(&window, sx, sy);
    assert!(!ui.get_enabled() && !ui.get_consent_open());
    click(&window, sx, sy);
    assert!(ui.get_enabled() && !ui.get_consent_open());
    assert_eq!(st.confirms.get(), 1);

    // The key card, now enabled.
    ui.set_on(true);
    slint::platform::update_timers_and_animations();
    let (ky, kh) = (ui.get_key_y(), ui.get_key_h());
    assert!(kh > 40.0, "key card height {kh}");

    // 4. Save with an empty key: the error, nothing stored.
    let save_hit = scan(&window, ky, ky + kh, true, || st.saves.get() > 0);
    assert!(save_hit.is_some(), "no click reached Save");
    let (kx, kyy) = save_hit.unwrap();
    assert_eq!((st.saves.get(), st.removes.get()), (1, 0));
    assert_eq!(ui.get_key_error(), t(lang, Msg::SettingsTrKeyEmpty));
    assert_eq!(store.get(ANTHROPIC_KEY_TARGET).unwrap(), None);

    // 5. A key over 2560 bytes: the error, nothing stored, the field cleared.
    ui.set_key_input("k".repeat(tr::MAX_KEY_BYTES + 1).into());
    click(&window, kx, kyy);
    assert_eq!(st.saves.get(), 2);
    assert_eq!(ui.get_key_error(), t(lang, Msg::SettingsTrKeyTooLong));
    assert_eq!(store.get(ANTHROPIC_KEY_TARGET).unwrap(), None);
    assert_eq!(ui.get_key_input(), "");

    // 6. A key: stored, the field cleared, only the mask shown, the buttons swap.
    let key = synthetic_key();
    ui.set_key_input(key.as_str().into());
    click(&window, kx, kyy);
    assert_eq!(st.saves.get(), 3);
    assert_eq!(ui.get_key_error(), "");
    assert_eq!(ui.get_key_input(), "");
    assert_eq!(store.get(ANTHROPIC_KEY_TARGET).unwrap().unwrap().0, key);
    let status = ui.get_key_status().to_string();
    assert!(status.ends_with("(sk-ant-\u{2026}Zk9w)"), "{status}");
    assert!(!status.contains("qqqq"), "{status}");
    assert!(!ui.get_editing());

    // 7. Remove (the rightmost button of the key row now) clears the stored key.
    slint::platform::update_timers_and_animations();
    let hit = scan(&window, ky, ky + kh, true, || {
        st.removes.get() > 0 || st.replaces.get() > 0
    });
    assert!(hit.is_some(), "no click reached Remove");
    assert_eq!((st.removes.get(), st.replaces.get()), (1, 0));
    assert_eq!(store.get(ANTHROPIC_KEY_TARGET).unwrap(), None);
    assert!(ui.get_editing());
    assert_eq!(ui.get_key_status(), t(lang, Msg::SettingsTrKeyNone));

    // 8. The real settings window: Ctrl+6 picks About (section index 5).
    let sw = SettingsWindow::new().unwrap();
    let swin = platform.windows.borrow()[1].clone();
    {
        let weak = sw.as_weak();
        sw.on_key(move |text, ctrl, shift, alt, meta, nav| {
            let Some(w) = weak.upgrade() else {
                return false;
            };
            let mods = Mods {
                ctrl,
                shift,
                alt,
                meta,
            };
            let ctx = settings_keys::KeyCtx {
                nav_focused: nav,
                ..Default::default()
            };
            match settings_keys::map_text(text.as_str(), mods, ctx) {
                Some(SettingsKey::Section(n)) => {
                    w.set_section(i32::from(n));
                    true
                }
                Some(_) => true,
                None => false,
            }
        });
    }
    sw.show().unwrap();
    slint::platform::update_timers_and_animations();
    assert_eq!(sw.get_section(), 0);
    let ctrl: SharedString = slint::platform::Key::Control.into();
    // A key typed but not saved is cleared when the section changes.
    sw.set_tr_key_input("typed-not-saved".into());
    for (digit, want) in [("5", 4), ("6", 5), ("1", 0)] {
        key_down(&swin, ctrl.clone());
        key_down(&swin, digit.into());
        key_up(&swin, digit.into());
        key_up(&swin, ctrl.clone());
        slint::platform::update_timers_and_animations();
        assert_eq!(sw.get_section(), want, "Ctrl+{digit}");
        assert_eq!(sw.get_tr_key_input(), "", "Ctrl+{digit}");
    }
}
