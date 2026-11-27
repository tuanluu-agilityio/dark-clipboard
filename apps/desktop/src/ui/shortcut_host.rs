//! Holds the OS registration of the global shortcuts on the Slint main thread (the
//! `global-hotkey` manager owns a window handle, so it lives in a thread-local like the tray).
//! Every function must be called on the main thread. On Windows presses arrive on the main thread
//! too (the manager's hidden window procedure); one handler tells them apart by the hotkey id
//! (Task 605: the popup, translate in place, and Esc while a translation runs), debounces each
//! shortcut on its own and calls its `OnPress`, which hops to the event loop itself.
//! Non-Windows builds get no-ops (Task 203 is Windows first).

use crate::shortcut::Status;
use std::sync::Arc;

/// Called (on any thread) for each press that passed the debounce; it must hop to the event
/// loop itself.
pub type OnPress = Arc<dyn Fn() + Send + Sync>;

/// What each shortcut does.
pub struct Handlers {
    pub popup: OnPress,
    pub translate: OnPress,
    /// Esc, only while a translation runs ([`set_cancel`]). Called without a debounce.
    pub cancel: OnPress,
}

/// The status of each shortcut after an [`apply`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Statuses {
    pub popup: Status,
    pub translate: Status,
}

#[cfg(windows)]
mod imp {
    use super::{Handlers, Statuses};
    use crate::shortcut::{
        dispatch, Backend, Debounce, Hotkeys, RegError, Shortcut, ShortcutId, Status,
    };
    use global_hotkey::hotkey::{Code, HotKey, Modifiers};
    use global_hotkey::{GlobalHotKeyEvent, GlobalHotKeyManager, HotKeyState};
    use std::cell::RefCell;
    use std::str::FromStr;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::{Arc, Mutex, PoisonError};
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

    /// Esc with no modifier: registered only while a translation runs.
    fn cancel_hotkey() -> HotKey {
        HotKey::new(None, Code::Escape)
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

        fn press_id(&self, s: &Shortcut) -> u32 {
            hotkey_for(s).map_or_else(|| crate::shortcut::press_id(s), |hk| hk.id())
        }
    }

    struct State {
        keys: Hotkeys<Os>,
        /// The press table the handler reads (it may not borrow `keys`).
        table: Arc<Mutex<Vec<(ShortcutId, u32)>>>,
    }

    thread_local! {
        static STATE: RefCell<Option<State>> = const { RefCell::new(None) };
    }

    /// True while Esc is registered for a running translation.
    static CANCEL_ARMED: AtomicBool = AtomicBool::new(false);

    pub fn init(h: Handlers) -> bool {
        let Ok(manager) = GlobalHotKeyManager::new() else {
            return false;
        };
        let table: Arc<Mutex<Vec<(ShortcutId, u32)>>> = Arc::default();
        let handler_table = Arc::clone(&table);
        let cancel_id = cancel_hotkey().id();
        // The handler is installed once per process and stays for its lifetime (the crate does
        // not allow removing it); `release` only unregisters the shortcuts.
        let origin = Instant::now();
        let debounce = Mutex::new((Debounce::default(), Debounce::default()));
        GlobalHotKeyEvent::set_event_handler(Some(move |e: GlobalHotKeyEvent| {
            // Only the key going down counts; the release is ignored.
            if e.state() != HotKeyState::Pressed {
                return;
            }
            if e.id() == cancel_id {
                if CANCEL_ARMED.load(Ordering::SeqCst) {
                    (h.cancel)();
                }
                return;
            }
            let which = {
                let t = handler_table.lock().unwrap_or_else(PoisonError::into_inner);
                dispatch(&t, e.id())
            };
            let Some(which) = which else {
                return;
            };
            let now = u64::try_from(origin.elapsed().as_millis()).unwrap_or(u64::MAX);
            let counted = {
                let mut d = debounce.lock().unwrap_or_else(PoisonError::into_inner);
                match which {
                    ShortcutId::Popup => d.0.press(now),
                    ShortcutId::Translate => d.1.press(now),
                }
            };
            if counted {
                match which {
                    ShortcutId::Popup => (h.popup)(),
                    ShortcutId::Translate => (h.translate)(),
                }
            }
        }));
        STATE.with(|s| {
            *s.borrow_mut() = Some(State {
                keys: Hotkeys::new(Os(manager)),
                table,
            });
        });
        true
    }

    pub fn apply(popup: &str, translate: Option<&str>) -> Statuses {
        STATE.with(|s| match s.borrow_mut().as_mut() {
            Some(st) => {
                // Translate first, and again if the popup freed its combination.
                let (popup, translate) = st.keys.apply_all(popup, translate);
                *st.table.lock().unwrap_or_else(PoisonError::into_inner) = st.keys.press_table();
                Statuses { popup, translate }
            }
            None => Statuses {
                popup: Status::Off,
                translate: Status::Off,
            },
        })
    }

    pub fn set_cancel(on: bool) -> bool {
        STATE.with(|s| {
            let mut guard = s.borrow_mut();
            let Some(st) = guard.as_mut() else {
                CANCEL_ARMED.store(false, Ordering::SeqCst);
                return false;
            };
            let os = st.keys.backend_mut();
            if on {
                if CANCEL_ARMED.load(Ordering::SeqCst) {
                    return true;
                }
                let ok = os.0.register(cancel_hotkey()).is_ok();
                CANCEL_ARMED.store(ok, Ordering::SeqCst);
                ok
            } else {
                if CANCEL_ARMED.swap(false, Ordering::SeqCst) {
                    let _ = os.0.unregister(cancel_hotkey());
                }
                true
            }
        })
    }

    pub fn release() {
        let _ = set_cancel(false);
        STATE.with(|s| {
            if let Some(mut st) = s.borrow_mut().take() {
                st.keys.release();
                st.table
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .clear();
            }
        });
    }
}

#[cfg(not(windows))]
mod imp {
    use super::{Handlers, Status, Statuses};

    pub fn init(_h: Handlers) -> bool {
        false
    }
    pub fn apply(_popup: &str, _translate: Option<&str>) -> Statuses {
        Statuses {
            popup: Status::Off,
            translate: Status::Off,
        }
    }
    pub fn set_cancel(_on: bool) -> bool {
        false
    }
    pub fn release() {}
}

/// Creates the OS manager and the press handler. `false` when the OS refuses (the app goes on
/// without global shortcuts; `apply` then reports `Off`).
pub fn init(h: Handlers) -> bool {
    imp::init(h)
}

/// Registers or changes the popup shortcut to `popup` and the translate shortcut to
/// `translate` (`None` while the feature is not active: unregistered).
pub fn apply(popup: &str, translate: Option<&str>) -> Statuses {
    imp::apply(popup, translate)
}

/// Registers Esc (`on`) while a translation runs, or unregisters it. `false` when Esc could
/// not be registered (the translation then cannot be cancelled with Esc).
pub fn set_cancel(on: bool) -> bool {
    imp::set_cancel(on)
}

/// Unregisters every shortcut, Esc included (the press handler stays for the process lifetime).
pub fn release() {
    imp::release();
}
