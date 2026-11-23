//! Application layer: services, capture actor, retention and permission guard.
//!
//! Must not depend on: `tauri`, OS crates (adapters are injected).

#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used, clippy::panic))]

pub mod api;
pub mod capture;
pub mod config;
pub mod envelope;
pub mod events;
pub mod image;
pub mod permission;
pub mod settings;

pub use api::{
    App, AppStatus, Caller, CaptureStatus, ClipApi, ClipDetailView, ClipSummaryView, ClipView,
    CopyMode, ExportResult, ExportSelection, GetOptions, ImageDetailView, ImportReport, PixelsView,
    StorageStatsView, TransferReply, TransferWait, UiTransfer,
};
pub use capture::{ActorCommand, CaptureHandle};
pub use config::CaptureConfig;
pub use envelope::{to_envelope, Page};
pub use events::{ApiError, AppEvent, SkipCounters};
pub use permission::{Operation, PermissionGuard};
pub use settings::{LoadOutcome, Settings};
