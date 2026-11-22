//! The OS-independent traits adapters implement (ADR-0004).

use crate::caps::Capabilities;
use crate::error::PlatformError;
use darkclip_core::markers::PrivacyMarker;
use darkclip_core::policy::ReadPlan;
use darkclip_core::{ClipboardSource, Mime, RawRepresentation, Timestamp};
use std::sync::mpsc::Sender;

/// One change notification from the platform event source.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ChangeSignal {
    pub seq: u64,
    pub at: Timestamp,
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

/// Where to paste. Opaque to the core; `None` means the currently focused window.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PasteTarget {
    pub app_id: Option<String>,
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
    /// `plan.truncate_text_at` bytes and every image at `plan.image_max_bytes`.
    ///
    /// Over-cap text is truncated on a UTF-8 boundary and the adapter must set
    /// `RawRepresentation::truncated`. An over-cap image returns `PlatformError::TooLarge`,
    /// with or without a size hint; that fails the whole read, so the text formats of the same
    /// clip are lost. For UTF-16 sources the cap applies to the UTF-8 output, but the raw read
    /// must still be bounded (never allocate an unbounded source buffer).
    fn read(&mut self, plan: &ReadPlan) -> Result<Vec<RawRepresentation>, PlatformError>;

    /// Writes representations; returns the sequence number after the write (self-write suppression).
    fn write(&mut self, reps: &[RawRepresentation]) -> Result<u64, PlatformError>;
}

pub trait PasteInjector: Send {
    fn paste(&mut self, target: &PasteTarget) -> Result<(), PlatformError>;
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
