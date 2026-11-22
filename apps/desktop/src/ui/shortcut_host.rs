//! Holds the OS registration of the global popup shortcut on the Slint main thread (the
//! `global-hotkey` manager owns a window handle, so it lives in a thread-local like the tray).
//! Every function must be called on the main thread. Presses arrive on another thread; they are
//! debounced there and handed to the event loop with `slint::invoke_from_event_loop`. Non-Windows
//! builds get no-ops (Task 203 is Windows first).

use crate::shortcut::Status;
use std::sync::Arc;

/// Called (on any thread) for each press that passed the debounce; it must hop to the event
/// loop itself.
pub type OnPress = Arc<dyn Fn() + Send + Sync>;

#[cfg(windows)]
mod imp {
    use super::OnPress;
    use crate::shortcut::{Backend, Debounce, RegError, Registrar, Shortcut, Status};
    use global_hotkey::hotkey::{Code, HotKey, Modifiers};
    use global_hotkey::{GlobalHotKeyEvent, GlobalHotKeyManager, HotKeyState};
    use std::cell::RefCell;
    use std::str::FromStr;
    use std::sync::Mutex;
    use std::time::Instant;

    struct Os(GlobalHotKeyManager);

    fn hotkey_for(s: &Shortcut) -> Option<HotKey> {
        let mut mods = Modifiers::empty();
        for (on, m) in [
            (s.ctrl, Modifiers::CONTROL),
            (s.alt, Modifiers::ALT),
            (s.shift, Modifiers::SHIFT),
            (s.win, Modifiers::META),
        ] {
            if on {
                mods |= m;
            }
        }
        let code = Code::from_str(&s.code).ok()?;
        Some(HotKey::new(Some(mods), code))
    }

    impl Backend for Os {
        fn register(&mut self, s: &Shortcut) -> Result<(), RegError> {
            let hk = hotkey_for(s).ok_or(RegError::Other)?;
            match self.0.register(hk) {
                Ok(()) => Ok(()),
                Err(global_hotkey::Error::AlreadyRegistered(_)) => Err(RegError::InUse),
                Err(_) => Err(RegError::Other),
            }
        }

        fn unregister(&mut self, s: &Shortcut) {
            if let Some(hk) = hotkey_for(s) {
                let _ = self.0.unregister(hk);
            }
        }
    }

    thread_local! {
        static REGISTRAR: RefCell<Option<Registrar<Os>>> = const { RefCell::new(None) };
    }

    pub fn init(on_press: OnPress) -> bool {
        let Ok(manager) = GlobalHotKeyManager::new() else {
            return false;
        };
        let origin = Instant::now();
        let debounce = Mutex::new(Debounce::default());
        GlobalHotKeyEvent::set_event_handler(Some(move |e: GlobalHotKeyEvent| {
            // Only the key going down counts; the release is ignored.
            if e.state() != HotKeyState::Pressed {
                return;
            }
            let now = u64::try_from(origin.elapsed().as_millis()).unwrap_or(u64::MAX);
            let counted = debounce.lock().is_ok_and(|mut d| d.press(now));
            if counted {
                on_press();
            }
        }));
        REGISTRAR.with(|r| *r.borrow_mut() = Some(Registrar::new(Os(manager))));
        true
    }

    pub fn apply(wanted: &str) -> Status {
        REGISTRAR.with(|r| match r.borrow_mut().as_mut() {
            Some(reg) => reg.apply(wanted),
            None => Status::Off,
        })
    }

    pub fn release() {
        REGISTRAR.with(|r| {
            if let Some(mut reg) = r.borrow_mut().take() {
                reg.release();
            }
        });
        GlobalHotKeyEvent::set_event_handler(None::<fn(GlobalHotKeyEvent)>);
    }
}

#[cfg(not(windows))]
mod imp {
    use super::{OnPress, Status};

    pub fn init(_on_press: OnPress) -> bool {
        false
    }
    pub fn apply(_wanted: &str) -> Status {
        Status::Off
    }
    pub fn release() {}
}

/// Creates the OS manager and the press handler. `false` when the OS refuses (the app goes on
/// without a global shortcut; `apply` then reports `Off`).
pub use imp::init;
/// Registers or changes the shortcut to `wanted` (see `Registrar::apply`).
pub fn apply(wanted: &str) -> Status {
    imp::apply(wanted)
}
/// Unregisters the shortcut and removes the press handler.
pub fn release() {
    imp::release();
}
