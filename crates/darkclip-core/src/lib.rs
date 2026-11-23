//! Domain models, policy, dedupe and sensitive-content detection.
//!
//! Must not depend on: any OS crate, `rusqlite`, `tauri`, any async runtime, the network.

#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
// The only unsafe code is the volatile zeroing in `sensitive/wipe.rs`, which opts in
// explicitly (F407-11); `forbid` would not allow that module's `allow`.
#![deny(unsafe_code)]
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used, clippy::panic))]

pub mod classify;
pub mod error;
#[cfg(any(test, feature = "fuzzing"))]
#[doc(hidden)]
pub mod fuzz_entry;
pub mod hash;
pub mod i18n;
pub mod ids;
pub mod limits;
pub mod markers;
pub mod model;
pub mod policy;
pub mod preview;
pub mod redact;
pub mod sensitive;
pub mod time;
pub mod transform;

pub use classify::{classify, Classified, RawRepresentation};
pub use error::CoreError;
pub use hash::canonical_hash;
pub use i18n::{t, Lang, Msg};
pub use ids::{ClipId, CollectionId, TagId};
pub use model::*;
pub use preview::{image_preview, make_preview};
pub use redact::Redacted;
pub use time::{Clock, Timestamp, FAR_FUTURE};
