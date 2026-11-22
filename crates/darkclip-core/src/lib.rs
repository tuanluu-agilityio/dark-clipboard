//! Domain models, policy, dedupe and sensitive-content detection.
//!
//! Must not depend on: any OS crate, `rusqlite`, `tauri`, any async runtime, the network.

#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used, clippy::panic))]

pub mod error;
pub mod i18n;
pub mod ids;
pub mod markers;
pub mod model;
pub mod policy;
pub mod redact;
pub mod time;

pub use error::CoreError;
pub use i18n::{t, Lang, Msg};
pub use ids::{ClipId, CollectionId, TagId};
pub use model::*;
pub use redact::Redacted;
pub use time::{Clock, Timestamp};
