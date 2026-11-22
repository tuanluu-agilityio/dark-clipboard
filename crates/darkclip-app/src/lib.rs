//! Application layer: services, capture actor, retention and permission guard.
//!
//! Must not depend on: `tauri`, OS crates (adapters are injected).

#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used, clippy::panic))]

pub mod capture;
pub mod config;
pub mod events;

pub use capture::{ActorCommand, CaptureHandle};
pub use config::CaptureConfig;
pub use events::{ApiError, AppEvent, SkipCounters};
