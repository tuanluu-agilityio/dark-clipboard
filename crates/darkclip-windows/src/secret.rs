//! The Windows [`SecretStore`] (Task 602, D-094): one generic Credential Manager credential per
//! target (`CRED_TYPE_GENERIC`, `CRED_PERSIST_LOCAL_MACHINE`: this user on this machine, never
//! roaming). `UserName` is `"anthropic"`; the blob is the UTF-8 secret bytes. The secret lives
//! only in Credential Manager: never in settings, logs or backups, and nothing is cached here.
//! Errors carry the Win32 code only (GC-06). All `unsafe` is in `win.rs`.

use crate::win;
use darkclip_core::Redacted;
use darkclip_platform::{PlatformError, SecretStore};

/// The `UserName` written with every credential (Credential Manager ignores it for generic
/// credentials; it only labels the entry).
const USER_NAME: &str = "anthropic";

/// Credential Manager secret store. Holds no state.
#[derive(Debug, Default, Clone, Copy)]
pub struct WinSecretStore;

impl SecretStore for WinSecretStore {
    fn get(&self, target: &str) -> Result<Option<Redacted<String>>, PlatformError> {
        let Some(blob) = win::cred_read(target)? else {
            return Ok(None);
        };
        // Borrow-checked first so a bad blob is never copied into an error value.
        let text = std::str::from_utf8(&blob).map_err(|_| PlatformError::Backend { code: 0 })?;
        Ok(Some(Redacted(text.to_owned())))
    }

    fn set(&self, target: &str, secret: &Redacted<String>) -> Result<(), PlatformError> {
        win::cred_write(target, USER_NAME, secret.0.as_bytes())
    }

    fn delete(&self, target: &str) -> Result<(), PlatformError> {
        win::cred_delete(target)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use darkclip_platform::ANTHROPIC_KEY_TARGET;

    /// Deletes the test credential however the test ends.
    struct Cleanup<'a>(&'a str);

    impl Drop for Cleanup<'_> {
        fn drop(&mut self) {
            let _ = WinSecretStore.delete(self.0);
        }
    }

    /// A synthetic secret assembled at runtime (never a key-shaped literal).
    fn synthetic(tag: &str) -> Redacted<String> {
        Redacted(["syn", tag, &std::process::id().to_string()].join("-"))
    }

    #[test]
    #[ignore = "writes a test-only credential to Windows Credential Manager, then deletes it"]
    fn win_secret_store_round_trip() {
        let target = format!("dark-clipboard/test-602-{}", std::process::id());
        assert_ne!(target, ANTHROPIC_KEY_TARGET);
        let _cleanup = Cleanup(&target);
        let store = WinSecretStore;
        let first = synthetic("first");
        let second = synthetic("second");

        store.set(&target, &first).unwrap();
        assert_eq!(store.get(&target).unwrap(), Some(first));
        store.set(&target, &second).unwrap();
        assert_eq!(store.get(&target).unwrap(), Some(second));
        store.delete(&target).unwrap();
        assert_eq!(store.get(&target).unwrap(), None);
        assert_eq!(store.delete(&target), Ok(()));
    }
}
