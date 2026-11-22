//! Start at login (Task 208): one `REG_SZ` value under
//! `HKCU\Software\Microsoft\Windows\CurrentVersion\Run` holding the quoted path of the current
//! executable and no arguments. Per-user only; nothing is written outside that key. The
//! command string is built by a pure function so its tests run on every OS.

use std::path::Path;

/// The `Run` key, relative to `HKEY_CURRENT_USER`.
pub const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
/// The value name this app uses inside the `Run` key.
pub const VALUE_NAME: &str = "DarkClipboard";
/// Where the shell (Task Manager > Startup, Settings > Apps > Startup) records that the user
/// turned an entry off. A value with the same name as the `Run` entry holds a flag byte.
pub const APPROVED_KEY: &str =
    r"Software\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\Run";

/// True when the first byte of a `StartupApproved` value marks the entry as disabled (odd,
/// `0x03`; enabled entries are even, `0x02`/`0x06`). A missing value means enabled.
pub fn approved_disabled(first_byte: Option<u8>) -> bool {
    first_byte.is_some_and(|b| b & 1 == 1)
}

/// The command the shell runs at login: the executable path in double quotes, no arguments.
/// Quoting keeps a path with spaces (`C:\Program Files\...`) in one piece.
pub fn run_command(exe: &Path) -> String {
    format!("\"{}\"", exe.display())
}

/// True when a stored `Run` value is exactly this app's command (Windows paths compare
/// case-insensitively).
pub fn is_our_command(stored: &str, exe: &Path) -> bool {
    stored.trim().eq_ignore_ascii_case(&run_command(exe))
}

#[cfg(windows)]
pub use registry::RegistryAutostart;

#[cfg(windows)]
mod registry {
    use super::{
        approved_disabled, is_our_command, run_command, APPROVED_KEY, RUN_KEY, VALUE_NAME,
    };
    use crate::win;
    use darkclip_platform::{Autostart, PlatformError};

    /// The real registry registration. `with_value_name` exists so the ignored test can use a
    /// throwaway value name; the app itself uses `new`.
    pub struct RegistryAutostart {
        value_name: String,
    }

    impl RegistryAutostart {
        pub fn new() -> Self {
            Self::with_value_name(VALUE_NAME)
        }

        pub fn with_value_name(name: &str) -> Self {
            Self {
                value_name: name.to_string(),
            }
        }
    }

    impl Default for RegistryAutostart {
        fn default() -> Self {
            Self::new()
        }
    }

    fn current_exe() -> Result<std::path::PathBuf, PlatformError> {
        std::env::current_exe().map_err(|_| PlatformError::Unsupported("current executable"))
    }

    impl Autostart for RegistryAutostart {
        fn is_enabled(&self) -> Result<bool, PlatformError> {
            let exe = current_exe()?;
            let stored = win::reg_get_string(RUN_KEY, &self.value_name)?;
            if !stored.is_some_and(|s| is_our_command(&s, &exe)) {
                return Ok(false);
            }
            // Turned off by the user in Task Manager or Windows Settings: not enabled.
            let flag = win::reg_get_first_byte(APPROVED_KEY, &self.value_name)?;
            Ok(!approved_disabled(flag))
        }

        fn set_enabled(&self, enabled: bool) -> Result<(), PlatformError> {
            if enabled {
                let exe = current_exe()?;
                win::reg_set_string(RUN_KEY, &self.value_name, &run_command(&exe))?;
                // Drop a "disabled" flag left by the user, so the entry runs again.
                win::reg_delete_value(APPROVED_KEY, &self.value_name)
            } else {
                win::reg_delete_value(RUN_KEY, &self.value_name)
            }
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        /// Changes the owner's login items (a throwaway value name, removed at the end), so
        /// agents never run it: `cargo test -p darkclip-windows -- --ignored autostart`.
        #[test]
        #[ignore = "changes login items"]
        fn autostart_real_registry_round_trip() {
            let a = RegistryAutostart::with_value_name("DarkClipboardTest");
            a.set_enabled(true).unwrap();
            assert!(a.is_enabled().unwrap());
            a.set_enabled(false).unwrap();
            assert!(!a.is_enabled().unwrap());
            // Removing what is not there is fine.
            a.set_enabled(false).unwrap();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn autostart_command_quotes_the_path_and_has_no_arguments() {
        let exe = PathBuf::from(r"C:\Program Files\Dark Clipboard\dark-clipboard.exe");
        assert_eq!(
            run_command(&exe),
            r#""C:\Program Files\Dark Clipboard\dark-clipboard.exe""#
        );
    }

    #[test]
    fn autostart_recognises_only_its_own_command() {
        let exe = PathBuf::from(r"C:\Apps\dark-clipboard.exe");
        assert!(is_our_command(r#""C:\Apps\dark-clipboard.exe""#, &exe));
        assert!(is_our_command(r#""c:\apps\DARK-CLIPBOARD.exe""#, &exe));
        // A moved executable or one with arguments is not "enabled" for this path.
        assert!(!is_our_command(r#""C:\Other\dark-clipboard.exe""#, &exe));
        assert!(!is_our_command(r#""C:\Apps\dark-clipboard.exe" --x"#, &exe));
        assert!(!is_our_command("", &exe));
    }

    #[test]
    fn a_disabled_startup_approved_flag_means_not_enabled() {
        assert!(approved_disabled(Some(0x03)));
        assert!(!approved_disabled(Some(0x02)));
        assert!(!approved_disabled(Some(0x06)));
        assert!(!approved_disabled(None));
        assert_eq!(
            APPROVED_KEY,
            r"Software\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\Run"
        );
    }

    #[test]
    fn run_key_is_the_current_user_run_key() {
        assert_eq!(RUN_KEY, r"Software\Microsoft\Windows\CurrentVersion\Run");
    }
}
