//! Windows platform adapter (ADR-0002, ADR-0004): Win32 clipboard monitor and access, and the
//! paste injector (focus return + `SendInput` Ctrl+V, Task 205).
//!
//! On other operating systems this crate compiles to an empty library, so the workspace
//! builds on all three OSes. All `unsafe` lives in `win.rs`. No logging, no window titles,
//! no clipboard content in this crate (GC-06); adapters report markers and never decide
//! (GC-04).

#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used, clippy::panic))]

#[cfg(windows)]
mod access;
// The command string is pure (tests run everywhere); the registry part is Windows only.
mod autostart;
// Pure format/marker mapping and read conversions: compiled on every OS so their tests run
// everywhere.
mod dib;
mod formats;
// Fuzz entry points (Task 406): tests and the `fuzzing` feature only.
#[cfg(any(test, feature = "fuzzing"))]
#[doc(hidden)]
pub mod fuzz_entry;
#[cfg(windows)]
mod monitor;
// Pure focus/paste decisions; compiled everywhere so the fake-seam tests run on every OS.
mod paste;
#[cfg(windows)]
mod source;
mod text;
#[cfg(windows)]
mod win;

#[cfg(windows)]
pub use access::WinAccess;
#[cfg(windows)]
pub use autostart::RegistryAutostart;
pub use autostart::{run_command, RUN_KEY, VALUE_NAME};
#[cfg(windows)]
pub use monitor::{SignalSourceFn, WinMonitor};
#[cfg(windows)]
pub use paste::WinPasteInjector;
#[cfg(windows)]
pub use source::WinSourceResolver;

#[cfg(windows)]
use darkclip_platform::{
    Capabilities, PlatformBundle, PlatformError, ShortcutSupport, SourceResolver, SourceSupport,
    Support,
};

#[cfg(windows)]
const NOT_YET: Support = Support::Unsupported { reason: "not yet" };

/// What exists in this build: monitor, read, write and marker detection (Tasks 101-105; the
/// real-clipboard tests ran in Task 107, `docs/testing/platform-checklist.md`) and auto-paste
/// with focus return (Task 205; the real paste is an owner manual row). The rest is reported as
/// not available.
#[cfg(windows)]
fn capabilities() -> Capabilities {
    Capabilities {
        monitor: Support::Native,
        read_without_focus: Support::Native,
        write: Support::Native,
        concealed_markers: Support::Native,
        source_app: SourceSupport::Owner,
        // No hotkey is registered by this adapter yet; `UserBound` claims nothing native.
        global_shortcut: ShortcutSupport::UserBound,
        auto_paste: Support::Native,
        tray: NOT_YET,
        autostart: Support::Native,
        backend: "win32",
    }
}

/// The Windows platform bundle. The monitor is not started here; the capture actor starts it.
#[cfg(windows)]
pub fn create() -> Result<PlatformBundle, PlatformError> {
    Ok(PlatformBundle {
        monitor: Box::new(WinMonitor::new(Box::new(|| WinSourceResolver.resolve()))),
        access: Box::new(WinAccess::new()),
        source: Box::new(WinSourceResolver),
        paste: Some(Box::new(WinPasteInjector)),
        caps: capabilities(),
    })
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;

    #[test]
    fn create_reports_win32_native_monitor() {
        let b = create().unwrap();
        assert_eq!(b.caps.backend, "win32");
        assert_eq!(b.caps.monitor, Support::Native);
        assert_eq!(b.caps.source_app, SourceSupport::Owner);
        assert_eq!(b.caps.read_without_focus, Support::Native);
        assert_eq!(b.caps.write, Support::Native);
        assert_eq!(b.caps.concealed_markers, Support::Native);
        assert_eq!(b.caps.auto_paste, Support::Native);
        assert_eq!(b.caps.autostart, Support::Native);
        // Read-only: whatever owns the clipboard now is `None` or a bare file name.
        assert!(b.source.resolve().is_none_or(|s| !s.app_id.contains('\\')));
        assert!(b.paste.is_some());
        assert!(b.access.sequence().is_ok());
    }
}
