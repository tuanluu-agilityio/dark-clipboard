//! The About section (Task 207): version, the capability report as rows, and the licences.
//!
//! The third-party list is STATIC: the direct dependencies of this app with the licence text
//! of their `Cargo.toml`, copied from `cargo metadata` when this task was written. A test
//! checks that every direct dependency of `apps/desktop/Cargo.toml` is listed, so the list
//! cannot silently fall behind. The full list of every dependency is `Cargo.lock`
//! (`cargo deny` checks all licences in CI).

use darkclip_core::i18n::Msg;
use darkclip_platform::{Capabilities, ShortcutSupport, SourceSupport, Support};

/// (crate, version, licence expression). Slint is used under its GPL-3.0-only option.
pub const THIRD_PARTY: &[(&str, &str, &str)] = &[
    ("ctrlc", "3.5.3", "MIT/Apache-2.0"),
    ("embed-resource", "3.0.12", "MIT"),
    ("flate2", "1.1.11", "MIT OR Apache-2.0"),
    ("global-hotkey", "0.10.0", "Apache-2.0 OR MIT"),
    ("png", "0.18.1", "MIT OR Apache-2.0"),
    ("rusqlite", "0.43.0", "MIT"),
    ("serde", "1.0.229", "MIT OR Apache-2.0"),
    ("serde_json", "1.0.154", "MIT OR Apache-2.0"),
    ("sha2", "0.11.1", "MIT OR Apache-2.0"),
    (
        "slint",
        "1.19.0",
        "GPL-3.0-only OR LicenseRef-Slint-Royalty-free-2.0 OR LicenseRef-Slint-Software-3.0",
    ),
    (
        "slint-build",
        "1.19.0",
        "GPL-3.0-only OR LicenseRef-Slint-Royalty-free-2.0 OR LicenseRef-Slint-Software-3.0",
    ),
    ("thiserror", "2.0.21", "MIT OR Apache-2.0"),
    ("tray-icon", "0.26.1", "MIT OR Apache-2.0"),
    ("ureq", "3.5.2", "MIT OR Apache-2.0"),
    ("rustls", "0.23.46", "Apache-2.0 OR ISC OR MIT"),
    ("ring", "0.17.14", "Apache-2.0 AND ISC"),
    ("webpki-roots", "1.0.12", "CDLA-Permissive-2.0"),
    ("uuid", "1.29.3", "Apache-2.0 OR MIT"),
    ("windows", "0.62.2", "MIT OR Apache-2.0"),
    ("zeroize", "1.9.2", "Apache-2.0 OR MIT"),
];

/// Notice for the root certificates the update check trusts (Task 412).
pub const ROOTS_NOTICE: &str = "Root certificates: Mozilla CA data from webpki-roots, CDLA-Permissive-2.0, https://cdla.dev/permissive-2-0/";

/// The app's own licence.
pub const OWN_LICENCE: &str = "GPL-3.0-only";

pub fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

/// The third-party list as text, one component per line.
pub fn third_party_text() -> String {
    let own = format!(
        "Dark Clipboard {}  -  {OWN_LICENCE}
",
        version()
    );
    own + &THIRD_PARTY
        .iter()
        .map(|(n, v, l)| format!("{n} {v}  -  {l}"))
        .collect::<Vec<_>>()
        .join("\n")
        + "\n"
        + ROOTS_NOTICE
}

/// A value in the capability list: a translated message, or the adapter's own short name.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CapValue {
    Msg(Msg),
    Raw(&'static str),
}

fn support(s: Support) -> CapValue {
    CapValue::Msg(match s {
        Support::Native => Msg::CapNative,
        Support::Polling { .. } => Msg::CapPolling,
        Support::Unsupported { .. } => Msg::CapNo,
    })
}

/// The capability report as (label, value) rows. Only what the platform adapter reports;
/// nothing is claimed beyond it.
pub fn capability_rows(c: &Capabilities) -> Vec<(Msg, CapValue)> {
    vec![
        (Msg::CapBackend, CapValue::Raw(c.backend)),
        (Msg::CapMonitor, support(c.monitor)),
        (Msg::CapReadWithoutFocus, support(c.read_without_focus)),
        (Msg::CapWrite, support(c.write)),
        (Msg::CapMarkers, support(c.concealed_markers)),
        (
            Msg::CapSourceApp,
            CapValue::Msg(match c.source_app {
                SourceSupport::Owner => Msg::CapSourceOwner,
                SourceSupport::Frontmost => Msg::CapSourceFront,
                SourceSupport::None => Msg::CapSourceNone,
            }),
        ),
        (
            Msg::CapShortcut,
            CapValue::Msg(match c.global_shortcut {
                ShortcutSupport::Native => Msg::CapNative,
                ShortcutSupport::Portal => Msg::CapPortal,
                ShortcutSupport::UserBound => Msg::CapUserBound,
            }),
        ),
        (Msg::CapAutoPaste, support(c.auto_paste)),
        (Msg::CapTray, support(c.tray)),
        (Msg::CapAutostart, support(c.autostart)),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn caps() -> Capabilities {
        Capabilities {
            monitor: Support::Native,
            read_without_focus: Support::Polling { interval_ms: 250 },
            write: Support::Native,
            concealed_markers: Support::Native,
            source_app: SourceSupport::Frontmost,
            global_shortcut: ShortcutSupport::Native,
            auto_paste: Support::Unsupported { reason: "x" },
            tray: Support::Native,
            autostart: Support::Unsupported { reason: "y" },
            backend: "fake",
        }
    }

    #[test]
    fn the_report_shows_what_the_adapter_says_and_nothing_more() {
        let rows = capability_rows(&caps());
        assert_eq!(rows.len(), 10);
        let get = |m: Msg| rows.iter().find(|r| r.0 == m).map(|r| r.1);
        assert_eq!(get(Msg::CapBackend), Some(CapValue::Raw("fake")));
        assert_eq!(get(Msg::CapMonitor), Some(CapValue::Msg(Msg::CapNative)));
        assert_eq!(
            get(Msg::CapReadWithoutFocus),
            Some(CapValue::Msg(Msg::CapPolling))
        );
        assert_eq!(get(Msg::CapAutoPaste), Some(CapValue::Msg(Msg::CapNo)));
        assert_eq!(get(Msg::CapAutostart), Some(CapValue::Msg(Msg::CapNo)));
        assert_eq!(
            get(Msg::CapSourceApp),
            Some(CapValue::Msg(Msg::CapSourceFront))
        );
    }

    #[test]
    fn the_licence_is_gpl3_and_slint_is_listed_under_its_gpl_option() {
        assert_eq!(OWN_LICENCE, "GPL-3.0-only");
        let slint = THIRD_PARTY.iter().find(|d| d.0 == "slint").unwrap();
        assert!(slint.2.contains("GPL-3.0-only"));
        assert!(third_party_text().contains("rusqlite 0.43.0"));
        let text = third_party_text();
        for needle in [
            "webpki-roots 1.0.12",
            "CDLA-Permissive-2.0",
            "https://cdla.dev/permissive-2-0/",
            "rustls 0.23.46",
            "ring 0.17.14",
        ] {
            assert!(text.contains(needle), "{needle} missing from the notices");
        }
        assert!(!version().is_empty());
    }

    #[test]
    fn every_direct_dependency_of_the_app_is_listed() {
        let manifest = include_str!("../../Cargo.toml");
        let mut in_deps = false;
        let mut seen = 0;
        for line in manifest.lines() {
            let l = line.trim();
            if l.starts_with('[') {
                in_deps = l.contains("dependencies") && !l.contains("dev-");
                continue;
            }
            let Some((name, _)) = l.split_once('=') else {
                continue;
            };
            let name = name.trim();
            if !in_deps || name.starts_with("darkclip-") {
                continue;
            }
            seen += 1;
            assert!(
                THIRD_PARTY.iter().any(|d| d.0 == name),
                "{name} is missing from THIRD_PARTY"
            );
        }
        assert!(seen >= 5, "the manifest was not read");
    }

    #[test]
    fn listed_versions_match_cargo_lock() {
        let lock = include_str!("../../../../Cargo.lock");
        let mut found: Vec<(String, String)> = Vec::new();
        let mut name = None;
        for line in lock.lines() {
            if let Some(n) = line.strip_prefix("name = \"") {
                name = Some(n.trim_end_matches('"').to_string());
            } else if let (Some(v), Some(n)) = (line.strip_prefix("version = \""), name.take()) {
                found.push((n, v.trim_end_matches('"').to_string()));
            }
        }
        for (n, v, _) in THIRD_PARTY {
            assert!(
                found.iter().any(|(fn_, fv)| fn_ == n && fv == v),
                "{n} {v} is not in Cargo.lock"
            );
        }
    }
}
