//! The file picker for backup, restore, export and import (Task 405).
//!
//! Windows: the shell's own save and open dialogs (`winsys::pick_file`), each on a short
//! thread of its own, so the Slint event loop never waits. One picker at a time across the
//! app. The answer comes back on the event loop. Elsewhere the picker is not available yet
//! (macOS and Linux are previews, Q-04) and the caller says so.
//!
//! The chosen path is handed to the caller only: never shown in a log line (GC-06).

use std::path::PathBuf;
#[cfg(windows)]
use std::sync::atomic::{AtomicBool, Ordering};

/// What to ask for.
pub struct Pick {
    pub save: bool,
    pub title: String,
    pub filter_name: String,
    /// File type without the dot (`dcbackup`).
    pub ext: &'static str,
    /// Suggested file name for a save dialog.
    pub default_name: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Started {
    Yes,
    /// Another picker is open: nothing was started.
    AlreadyOpen,
    /// No picker on this platform, or its thread could not start.
    Unavailable,
}

#[cfg(windows)]
static OPEN: AtomicBool = AtomicBool::new(false);

/// Shows the picker; `done` runs on the Slint event loop with the chosen file, or `None` when
/// the user cancelled. Not called when the result is not `Started::Yes`.
pub fn pick(p: Pick, done: impl FnOnce(Option<PathBuf>) + Send + 'static) -> Started {
    #[cfg(windows)]
    {
        if OPEN.swap(true, Ordering::SeqCst) {
            return Started::AlreadyOpen;
        }
        let spawned = std::thread::Builder::new()
            .name("darkclip-file-dialog".into())
            .spawn(move || {
                let path = crate::winsys::pick_file(
                    p.save,
                    &p.title,
                    &p.filter_name,
                    p.ext,
                    p.default_name.as_deref(),
                );
                OPEN.store(false, Ordering::SeqCst);
                let _ = slint::invoke_from_event_loop(move || done(path));
            });
        if spawned.is_err() {
            OPEN.store(false, Ordering::SeqCst);
            return Started::Unavailable;
        }
        Started::Yes
    }
    #[cfg(not(windows))]
    {
        let _ = (p, done);
        Started::Unavailable
    }
}
