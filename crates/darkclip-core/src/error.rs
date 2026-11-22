//! Core error type. Messages never contain clip content (GC-06).

use thiserror::Error;

/// Errors produced by the core model. No variant carries user content.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum CoreError {
    #[error("invalid id")]
    InvalidId,
    #[error("invalid timestamp")]
    InvalidTimestamp,
    #[error("invalid tag")]
    InvalidTag,
    #[error("field `{field}` is longer than {max} characters")]
    TooLong { field: &'static str, max: usize },
}
