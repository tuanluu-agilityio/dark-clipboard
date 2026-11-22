//! Windows platform adapter (ADR-0002, ADR-0004): Win32 clipboard monitor and access.
//!
//! On other operating systems this crate compiles to an empty library, so the workspace
//! builds on all three OSes. All `unsafe` lives in `win.rs`. No logging, no window titles,
//! no clipboard content in this crate (GC-06); adapters report markers and never decide
//! (GC-04).

#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used, clippy::panic))]

#[cfg(windows)]
mod access;
#[cfg(windows)]
mod monitor;
#[cfg(windows)]
mod win;

#[cfg(windows)]
pub use access::WinAccess;
#[cfg(windows)]
pub use monitor::{SignalSourceFn, WinMonitor};

#[cfg(windows)]
use darkclip_platform::{
    Capabilities, PlatformBundle, PlatformError, ShortcutSupport, SourceResolver, SourceSupport,
    Support,
};

/// Source resolver placeholder until Task 105: always `None`.
#[cfg(windows)]
struct NoSource;

#[cfg(windows)]
impl SourceResolver for NoSource {
    fn resolve(&self) -> Option<darkclip_core::ClipboardSource> {
        None
    }
}

#[cfg(windows)]
const NOT_YET: Support = Support::Unsupported { reason: "not yet" };

/// What exists in this build: the monitor. Everything else is reported as not available.
#[cfg(windows)]
fn capabilities() -> Capabilities {
    Capabilities {
        monitor: Support::Native,
        read_without_focus: NOT_YET,
        write: NOT_YET,
        concealed_markers: NOT_YET,
        source_app: SourceSupport::None,
        // No hotkey is registered by this adapter yet; `UserBound` claims nothing native.
        global_shortcut: ShortcutSupport::UserBound,
        auto_paste: NOT_YET,
        tray: NOT_YET,
        autostart: NOT_YET,
        backend: "win32",
    }
}

/// The Windows platform bundle. The monitor is not started here; the capture actor starts it.
#[cfg(windows)]
pub fn create() -> Result<PlatformBundle, PlatformError> {
    Ok(PlatformBundle {
        monitor: Box::new(WinMonitor::new(Box::new(|| None))),
        access: Box::new(WinAccess::new()),
        source: Box::new(NoSource),
        paste: None,
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
        assert_eq!(b.caps.source_app, SourceSupport::None);
        assert!(matches!(b.caps.write, Support::Unsupported { .. }));
        assert!(b.source.resolve().is_none());
        assert!(b.paste.is_none());
        assert!(b.access.sequence().is_ok());
    }
}
