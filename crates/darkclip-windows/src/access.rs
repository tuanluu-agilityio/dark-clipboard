//! `WinAccess`: clipboard access from the capture actor's thread.
//!
//! Task 101: only `sequence()` works. `snapshot_meta`, `read` and `write` arrive in Tasks
//! 102-104. Adapter rule (S-5): a failed `OpenClipboard` returns `PlatformError::Busy` once;
//! the actor owns retries. No policy here (GC-04), no content in errors (GC-06).

use crate::win;
use darkclip_core::policy::ReadPlan;
use darkclip_core::RawRepresentation;
use darkclip_platform::{ClipboardAccess, ClipboardMeta, PlatformError};

const NOT_YET: PlatformError = PlatformError::Unsupported("not yet");

/// Win32 clipboard access.
#[derive(Default)]
pub struct WinAccess {
    _private: (),
}

impl WinAccess {
    pub fn new() -> Self {
        Self::default()
    }
}

impl ClipboardAccess for WinAccess {
    /// `GetClipboardSequenceNumber`; never fails (F-03).
    fn sequence(&self) -> Result<u64, PlatformError> {
        Ok(u64::from(win::clipboard_sequence()))
    }

    fn snapshot_meta(&mut self) -> Result<ClipboardMeta, PlatformError> {
        Err(NOT_YET)
    }

    fn read(&mut self, _plan: &ReadPlan) -> Result<Vec<RawRepresentation>, PlatformError> {
        Err(NOT_YET)
    }

    fn write(&mut self, _reps: &[RawRepresentation]) -> Result<u64, PlatformError> {
        Err(NOT_YET)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sequence_is_ok() {
        let a = WinAccess::new();
        let first = a.sequence().unwrap();
        // Never fails, and is stable or increasing (another app may copy meanwhile).
        let second = a.sequence().unwrap();
        assert!(second >= first);
    }

    #[test]
    fn unimplemented_methods_are_unsupported() {
        let mut a = WinAccess::new();
        assert_eq!(a.snapshot_meta().unwrap_err(), NOT_YET);
        assert_eq!(a.write(&[]).unwrap_err(), NOT_YET);
    }
}
