//! Holds the tray on the Slint main thread (the tray is not `Send`, so it lives in a
//! thread-local). Every function here must be called on the main thread; other threads reach it
//! with `slint::invoke_from_event_loop`. Non-Windows builds get no-ops (Task 202 is Windows
//! first).

use crate::tray::{MenuAction, TrayState, TrayUpdate};
use std::sync::Arc;

/// Called for each tray menu choice or click, on any thread.
pub type OnAction = Arc<dyn Fn(MenuAction) + Send + Sync>;

/// The current local UTC offset in minutes (0 where unknown).
pub fn local_utc_offset_minutes() -> i32 {
    #[cfg(windows)]
    {
        crate::winsys::local_utc_offset_minutes()
    }
    #[cfg(not(windows))]
    {
        0
    }
}

#[cfg(windows)]
mod imp {
    use super::{OnAction, TrayState, TrayUpdate};
    use crate::tray::Tray;
    use std::cell::RefCell;

    struct Held {
        tray: Tray,
        state: TrayState,
    }

    thread_local! {
        static TRAY: RefCell<Option<Held>> = const { RefCell::new(None) };
    }

    pub fn create(first: TrayUpdate, on_action: OnAction) -> bool {
        match Tray::new(first, on_action) {
            Some(tray) => {
                TRAY.with(|t| {
                    *t.borrow_mut() = Some(Held {
                        tray,
                        state: first.state,
                    });
                });
                true
            }
            None => false,
        }
    }

    pub fn update(u: TrayUpdate) {
        TRAY.with(|t| {
            if let Some(h) = t.borrow_mut().as_mut() {
                h.tray.update(u);
                h.state = u.state;
            }
        });
    }

    pub fn current_state() -> Option<TrayState> {
        TRAY.with(|t| t.borrow().as_ref().map(|h| h.state))
    }

    /// Removes the icon from the notification area.
    pub fn destroy() {
        TRAY.with(|t| *t.borrow_mut() = None);
    }
}

#[cfg(not(windows))]
mod imp {
    use super::{OnAction, TrayState, TrayUpdate};

    pub fn create(_first: TrayUpdate, _on_action: OnAction) -> bool {
        false
    }
    pub fn update(_u: TrayUpdate) {}
    pub fn current_state() -> Option<TrayState> {
        None
    }
    pub fn destroy() {}
}

pub use imp::{create, current_state, destroy, update};
