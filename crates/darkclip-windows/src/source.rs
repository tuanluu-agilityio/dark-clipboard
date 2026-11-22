//! `WinSourceResolver`: the clipboard owner window's process image file name (GC-06).
//!
//! Only the file name is kept; the full path can contain the user name and is dropped inside
//! this module. No window title is ever read (SR-11).

use crate::win;
use darkclip_core::{ClipboardSource, Platform, SourceConfidence};
use darkclip_platform::SourceResolver;

/// The part after the last `\` or `/`, as UTF-8. `None` for an empty name or a name that is
/// not valid UTF-16 (unpaired surrogates are not repaired: the source is then unknown).
pub(crate) fn file_name_from_path(path: &[u16]) -> Option<String> {
    let start = path
        .iter()
        .rposition(|&c| c == u16::from(b'\\') || c == u16::from(b'/'))
        .map_or(0, |i| i + 1);
    let name = String::from_utf16(&path[start..]).ok()?;
    if name.is_empty() {
        None
    } else {
        Some(name)
    }
}

/// Resolves the source from `GetClipboardOwner` (confidence `Owner`).
pub struct WinSourceResolver;

impl SourceResolver for WinSourceResolver {
    fn resolve(&self) -> Option<ClipboardSource> {
        let path = win::clipboard_owner_image_path()?;
        let app_id = file_name_from_path(&path)?;
        Some(ClipboardSource {
            app_id,
            app_name: None,
            platform: Platform::Windows,
            confidence: SourceConfidence::Owner,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn w(s: &str) -> Vec<u16> {
        s.encode_utf16().collect()
    }

    #[test]
    fn file_name_from_full_path() {
        assert_eq!(
            file_name_from_path(&w(r"C:\Program Files\KeePassXC\KeePassXC.exe")).as_deref(),
            Some("KeePassXC.exe")
        );
    }

    #[test]
    fn file_name_without_separator_is_kept() {
        assert_eq!(file_name_from_path(&w("a.exe")).as_deref(), Some("a.exe"));
    }

    #[test]
    fn file_name_forward_slash() {
        assert_eq!(
            file_name_from_path(&w("C:/x/b.exe")).as_deref(),
            Some("b.exe")
        );
    }

    #[test]
    fn file_name_empty_or_trailing_separator_is_none() {
        assert_eq!(file_name_from_path(&[]), None);
        assert_eq!(file_name_from_path(&w(r"C:\dir\")), None);
    }

    #[test]
    fn file_name_invalid_utf16_is_none() {
        let mut p = w(r"C:\x\");
        p.push(0xD800); // lone high surrogate
        p.extend(w(".exe"));
        assert_eq!(file_name_from_path(&p), None);
    }

    #[test]
    fn file_name_non_ascii_is_lossless() {
        assert_eq!(
            file_name_from_path(&w(r"C:\Tệp\Ứng dụng.exe")).as_deref(),
            Some("Ứng dụng.exe")
        );
    }

    #[test]
    fn resolve_does_not_leak_handles() {
        let before = crate::win::test_support::process_handle_count();
        let mut resolved = false;
        for _ in 0..1000 {
            resolved |= WinSourceResolver.resolve().is_some();
        }
        let after = crate::win::test_support::process_handle_count();
        // Slack: tests in parallel threads open handles too; a leak of one handle per call
        // would grow by 1000.
        // No owner right now: nothing was opened, so there is nothing to assert.
        if resolved {
            assert!(
                after <= before + 200,
                "handles grew from {before} to {after}"
            );
        }
    }

    #[test]
    fn resolve_does_not_panic() {
        // Whatever owns the clipboard now (read-only; the clipboard is never opened).
        if let Some(s) = WinSourceResolver.resolve() {
            assert!(!s.app_id.is_empty());
            assert!(!s.app_id.contains('\\') && !s.app_id.contains('/'));
            assert_eq!(s.app_name, None);
            assert_eq!(s.platform, Platform::Windows);
            assert_eq!(s.confidence, SourceConfidence::Owner);
        }
    }

    #[test]
    #[ignore = "writes the real clipboard"]
    fn real_clipboard_write_owner_is_powershell() {
        // The owner window dies with its process, so resolve while the child is alive.
        let mut child = std::process::Command::new("powershell")
            .args([
                "-NoProfile",
                "-Command",
                "Set-Clipboard -Value x; Start-Sleep -Seconds 4",
            ])
            .spawn()
            .unwrap();
        let mut found = None;
        for _ in 0..30 {
            std::thread::sleep(std::time::Duration::from_millis(100));
            if let Some(s) = WinSourceResolver.resolve() {
                found = Some(s);
                break;
            }
        }
        let _ = child.kill();
        let _ = child.wait();
        let s = found.expect("no owner resolved");
        assert!(
            s.app_id.eq_ignore_ascii_case("powershell.exe"),
            "{}",
            s.app_id
        );
    }
}
