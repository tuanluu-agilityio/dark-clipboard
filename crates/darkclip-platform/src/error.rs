//! Platform errors. Carry no clipboard content (GC-06): only static text, a format name,
//! a byte count and an OS error code.

use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PlatformError {
    Unsupported(&'static str),
    PermissionDenied(&'static str),
    /// The clipboard is held by another process; retry shortly.
    Busy,
    TooLarge {
        format: String,
        bytes: u64,
    },
    Backend {
        code: i64,
    },
}

impl fmt::Display for PlatformError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unsupported(what) => write!(f, "unsupported: {what}"),
            Self::PermissionDenied(what) => write!(f, "permission denied: {what}"),
            Self::Busy => f.write_str("clipboard busy"),
            Self::TooLarge { format, bytes } => write!(f, "{format} too large ({bytes} bytes)"),
            Self::Backend { code } => write!(f, "backend error {code}"),
        }
    }
}

impl std::error::Error for PlatformError {}
