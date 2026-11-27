//! The OS-independent traits adapters implement (ADR-0004).

use crate::caps::Capabilities;
use crate::error::PlatformError;
use darkclip_core::markers::PrivacyMarker;
use darkclip_core::policy::ReadPlan;
use darkclip_core::{ClipboardSource, Mime, RawRepresentation, Redacted, Timestamp};
use std::sync::mpsc::Sender;

/// One change notification from the platform event source.
///
/// `source` is the app that owned the clipboard when the change was observed, resolved on
/// the monitor thread at notification time (it is more reliable then than at dequeue).
/// `None` when the backend cannot resolve at signal time; the actor then falls back to
/// `SourceResolver::resolve()` (D-031). Sources are app ids, never content.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChangeSignal {
    pub seq: u64,
    pub at: Timestamp,
    pub source: Option<ClipboardSource>,
}

/// An offered format: platform name, normalized MIME if known, size if known.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlatformFormat {
    pub name: String,
    pub mime: Option<Mime>,
    pub size_hint: Option<u64>,
}

/// Format names, markers and source of the current clipboard. Never content.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClipboardMeta {
    pub seq: u64,
    pub formats: Vec<PlatformFormat>,
    pub markers: Vec<PrivacyMarker>,
    pub source: Option<ClipboardSource>,
}

/// Where to paste. Opaque to the core. Recorded by the UI when the popup shows (Task 205).
///
/// `window` and `pid` identify the window that had the focus then (Windows: the `HWND` value
/// and its process id). An injector must refuse a target it cannot identify: it never pastes
/// into "whatever is focused now" (never paste blind, D-044). No title is ever kept (SR-11).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PasteTarget {
    pub app_id: Option<String>,
    /// Native window handle value; opaque outside the adapter.
    pub window: Option<u64>,
    /// Process id that owned `window` when it was recorded.
    pub pid: Option<u32>,
}

pub trait ClipboardMonitor: Send {
    /// Starts the platform event source; sends a signal for every observed change.
    fn start(&mut self, tx: Sender<ChangeSignal>) -> Result<(), PlatformError>;
    fn stop(&mut self);
}

/// All methods are called only from the CaptureActor thread (not Sync on purpose).
pub trait ClipboardAccess: Send {
    fn sequence(&self) -> Result<u64, PlatformError>;

    /// Format names, markers and source. Must not read content.
    fn snapshot_meta(&mut self) -> Result<ClipboardMeta, PlatformError>;

    /// Reads only the formats in the plan (never a format absent from it), enforcing caps:
    /// every non-image format (plain, HTML, RTF, uri-list) is capped at
    /// `plan.truncate_text_at` bytes and every image at `plan.image_max_bytes`. The image cap
    /// applies to the PNG that is returned (D-043): an adapter that converts another image
    /// format bounds that input by the decode caps (`darkclip_core::limits`, checked from its
    /// header and size before allocating) and the PNG it produces by `image_max_bytes`.
    ///
    /// Over-cap text is truncated on a UTF-8 boundary and the adapter must set
    /// `RawRepresentation::truncated`. An over-cap image returns `PlatformError::TooLarge`,
    /// with or without a size hint; that fails the whole read, so the text formats of the same
    /// clip are lost. For UTF-16 sources the cap applies to the UTF-8 output, but the raw read
    /// must still be bounded (never allocate an unbounded source buffer).
    fn read(&mut self, plan: &ReadPlan) -> Result<Vec<RawRepresentation>, PlatformError>;

    /// Writes representations; returns the sequence number after the write (self-write suppression).
    fn write(&mut self, reps: &[RawRepresentation]) -> Result<u64, PlatformError>;

    /// Drops anything `write` kept for a retry of the same representations (for example
    /// converted blocks kept across `Busy`). The actor calls it when a copy-back ends with an
    /// error after its retries, so no prepared content outlives the failed copy-back. Must
    /// not touch the clipboard. Default: nothing is kept.
    fn discard_prepared(&mut self) {}
}

/// Focus return and auto-paste (Task 205). Called off the UI thread; may wait briefly (bounded)
/// for the target to take the focus.
///
/// Errors: `PermissionDenied` when the target runs with higher privileges than the app (UIPI;
/// the clip stays copied), `Unsupported` when the target cannot be identified safely or did not
/// take the focus, `Backend` when the OS did not accept the keystrokes. Reasons are static text.
pub trait PasteInjector: Send {
    /// Returns the focus to `target`, then sends the paste shortcut, only after every safety
    /// check passed.
    fn paste(&mut self, target: &PasteTarget) -> Result<(), PlatformError>;

    /// Returns the focus to `target` without sending any input. Default: not supported.
    fn focus(&mut self, target: &PasteTarget) -> Result<(), PlatformError> {
        let _ = target;
        Err(PlatformError::Unsupported("focus return"))
    }
}

pub trait SourceResolver: Send {
    fn resolve(&self) -> Option<ClipboardSource>;
}

/// What an adapter's `create()` returns.
pub struct PlatformBundle {
    pub monitor: Box<dyn ClipboardMonitor>,
    pub access: Box<dyn ClipboardAccess>,
    pub source: Box<dyn SourceResolver>,
    pub paste: Option<Box<dyn PasteInjector>>,
    pub caps: Capabilities,
}

/// Registers the app to start when the user logs in (Task 208). Windows: a value under
/// `HKCU\Software\Microsoft\Windows\CurrentVersion\Run` that points to the current
/// executable and passes no arguments. Methods may block briefly (registry, files): call them
/// off the UI thread. Errors carry static reasons only.
pub trait Autostart: Send + Sync {
    /// True when this app is registered to start at login.
    fn is_enabled(&self) -> Result<bool, PlatformError>;
    /// Registers (`true`) or removes (`false`) the entry. Removing a missing entry succeeds.
    fn set_enabled(&self, enabled: bool) -> Result<(), PlatformError>;
}

/// The Credential Manager target (or keychain item) that holds the Anthropic API key
/// (Task 602, D-094). Fixed; never a setting.
pub const ANTHROPIC_KEY_TARGET: &str = "dark-clipboard/anthropic";

/// Per-user secret storage outside settings, logs and backups (Task 602; Windows: Credential
/// Manager). Secrets travel only inside [`Redacted`], so they never reach `Debug` output; errors
/// carry codes only, never the secret (GC-06). Implementations cache nothing. Calls may block
/// briefly: call them off the UI thread.
pub trait SecretStore: Send + Sync {
    /// The secret stored under `target`; `Ok(None)` when there is none.
    fn get(&self, target: &str) -> Result<Option<Redacted<String>>, PlatformError>;
    /// Stores `secret` under `target`, replacing any earlier one.
    fn set(&self, target: &str, secret: &Redacted<String>) -> Result<(), PlatformError>;
    /// Removes the secret under `target`; `Ok(())` when there is none.
    fn delete(&self, target: &str) -> Result<(), PlatformError>;
}
