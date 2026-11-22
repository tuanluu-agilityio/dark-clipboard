//! Platform traits, capability types and a `FakePlatform` for tests (ADR-0004).
//!
//! Must not depend on: OS crates (only traits, capability types, a fake for tests).

#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used, clippy::panic))]

pub mod caps;
pub mod error;
#[cfg(any(test, feature = "fake"))]
pub mod fake;
pub mod traits;

pub use caps::{Capabilities, ShortcutSupport, SourceSupport, Support};
pub use error::PlatformError;
pub use traits::{
    Autostart, ChangeSignal, ClipboardAccess, ClipboardMeta, ClipboardMonitor, PasteInjector,
    PasteTarget, PlatformBundle, PlatformFormat, SourceResolver,
};
