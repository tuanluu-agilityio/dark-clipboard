//! Events the capture actor publishes, skip counters, and the application error type.
//!
//! Nothing here carries clipboard content, previews, file paths or app names (GC-06).

use darkclip_core::policy::SkipReason;
use darkclip_core::{CaptureMode, ClipId};
use std::collections::HashMap;

/// Published by the capture actor. Ids, reasons and static text only.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AppEvent {
    ClipAdded(ClipId),
    ClipBumped(ClipId),
    Skipped(SkipReason),
    ModeChanged(CaptureMode),
    /// A platform or storage facility failed; static text only, never content.
    Unavailable(&'static str),
}

/// Application error. Task 011 owns the full API surface and adds `code()`; the variant set
/// already matches the Task 011 interface. No variant carries content (GC-06).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ApiError {
    NotFound,
    InvalidArgument(&'static str),
    PermissionDenied(&'static str),
    UnsupportedPlatform,
    ClipboardBusy,
    Storage,
    AppNotRunning,
    SchemaTooNew,
    Internal,
}

impl std::fmt::Display for ApiError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotFound => f.write_str("not found"),
            Self::InvalidArgument(what) => write!(f, "invalid argument: {what}"),
            Self::PermissionDenied(what) => write!(f, "permission denied: {what}"),
            Self::UnsupportedPlatform => f.write_str("unsupported on this platform"),
            Self::ClipboardBusy => f.write_str("clipboard busy"),
            Self::Storage => f.write_str("storage error"),
            Self::AppNotRunning => f.write_str("app not running"),
            Self::SchemaTooNew => f.write_str("database schema too new"),
            Self::Internal => f.write_str("internal error"),
        }
    }
}

impl std::error::Error for ApiError {}

/// How many changes were skipped, per reason. In memory only; never persisted or logged.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SkipCounters {
    counts: HashMap<SkipReason, u64>,
}

impl SkipCounters {
    pub fn increment(&mut self, reason: SkipReason) {
        let n = self.counts.entry(reason).or_insert(0);
        *n = n.saturating_add(1);
    }

    pub fn get(&self, reason: SkipReason) -> u64 {
        self.counts.get(&reason).copied().unwrap_or(0)
    }

    pub fn total(&self) -> u64 {
        self.counts.values().fold(0u64, |a, b| a.saturating_add(*b))
    }

    /// Non-zero counters, in no particular order.
    pub fn iter(&self) -> impl Iterator<Item = (SkipReason, u64)> + '_ {
        self.counts.iter().map(|(r, n)| (*r, *n))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn skip_counters_count_per_reason() {
        let mut c = SkipCounters::default();
        c.increment(SkipReason::Busy);
        c.increment(SkipReason::Busy);
        c.increment(SkipReason::Sensitive);
        assert_eq!(c.get(SkipReason::Busy), 2);
        assert_eq!(c.get(SkipReason::Sensitive), 1);
        assert_eq!(c.get(SkipReason::Paused), 0);
        assert_eq!(c.total(), 3);
        assert_eq!(c.iter().count(), 2);
    }

    #[test]
    fn api_error_display_is_static() {
        assert_eq!(
            ApiError::InvalidArgument("clip has no plain text").to_string(),
            "invalid argument: clip has no plain text"
        );
        assert_eq!(ApiError::ClipboardBusy.to_string(), "clipboard busy");
    }
}
