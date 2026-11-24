//! Task 705 tests for the settings window: nothing was lost in the rebuild, every setting sits
//! in exactly one section, "Always protected" has no control (GC-07), no std-widgets form
//! control is left and the five confirmations share one `ConfirmDialog`. Source scans only: no
//! window is created (GC-19).
//!
//! The three lists below were copied from `ui/settings.slint` at the base commit of Task 705
//! (the window before the redesign): 50 properties and 34 callbacks of `SettingsWindow`, and the
//! 86 `SText` strings. They are frozen on purpose: removing or renaming one is a failure here.

use std::collections::BTreeSet;
use std::fs;
use std::path::PathBuf;

fn manifest() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn read(rel: &str) -> String {
    fs::read_to_string(manifest().join(rel)).unwrap_or_default()
}

/// A property not written or read by Rust: the days field of "Delete old clips" is passed as the
/// argument of `older-ask`.
const NOT_BOUND_IN_RUST: &[&str] = &["older-days"];

const WINDOW_PROPERTIES: &[&str] = &[
    "section",
    "can-save",
    "capture-off",
    "login-enabled",
    "shortcut",
    "lang-index",
    "launch-at-login",
    "position-index",
    "close-on-focus-loss",
    "enter-pastes",
    "sensitive-index",
    "kinds",
    "max-items",
    "max-age",
    "max-storage",
    "new-app",
    "apps",
    "err-shortcut",
    "err-items",
    "err-age",
    "err-storage",
    "err-kinds",
    "err-app",
    "status",
    "status-error",
    "mode",
    "confirm-remove-text",
    "picker-open",
    "picker-loading",
    "picker",
    "clear-open",
    "limits-open",
    "clear-keep-pinned",
    "clear-question",
    "cleanup-open",
    "transfer-busy",
    "restore-open",
    "restore-question",
    "cleanup-question",
    "older-days",
    "stats-text",
    "skip-text",
    "caps-text",
    "version-text",
    "update-status",
    "update-busy",
    "update-found",
    "auto-update",
    "auto-update-enabled",
    "data-path",
];

const WINDOW_CALLBACKS: &[&str] = &[
    "save",
    "revert",
    "edited",
    "set-mode",
    "start-capturing",
    "add-app",
    "remove-app",
    "confirm-remove",
    "cancel-remove",
    "open-picker",
    "close-picker",
    "pick-app",
    "limits-confirm",
    "limits-cancel",
    "clear-ask",
    "clear-confirm",
    "clear-cancel",
    "clear-toggled",
    "older-ask",
    "orphans-ask",
    "compact-ask",
    "cleanup-confirm",
    "cleanup-cancel",
    "backup-ask",
    "restore-ask",
    "import-ask",
    "restore-confirm",
    "restore-cancel",
    "open-folder",
    "show-welcome",
    "check-updates",
    "auto-update-toggled",
    "open-release",
    "escape",
];

const STEXT_PROPERTIES: &[&str] = &[
    "title",
    "general",
    "privacy",
    "history",
    "about",
    "banner",
    "save",
    "revert",
    "cancel",
    "close",
    "loading",
    "notices",
    "limits-confirm",
    "shortcut-label",
    "shortcut-hint",
    "shortcut-warning",
    "lang-label",
    "lang-en",
    "lang-vi",
    "login",
    "login-note",
    "login-drift",
    "pos-label",
    "pos-cursor",
    "pos-center",
    "close-focus",
    "enter-pastes",
    "capture-label",
    "mode-normal",
    "mode-private",
    "mode-paused",
    "mode-current",
    "capture-off-text",
    "start-capturing",
    "markers-label",
    "markers-note",
    "sensitive-label",
    "sens-drop",
    "sens-masked",
    "sens-store",
    "sens-warn",
    "kinds-label",
    "excluded-label",
    "excluded-note",
    "builtin-badge",
    "remove",
    "add",
    "app-placeholder",
    "pick-running",
    "pick-none",
    "pick-note",
    "confirm-builtin",
    "max-items-label",
    "max-age-label",
    "max-storage-label",
    "retention-note",
    "stats-label",
    "skipped-label",
    "clear",
    "clear-confirm",
    "keep-pinned",
    "clear-do",
    "cleanup-heading",
    "older-label",
    "older-do",
    "orphans-do",
    "compact-do",
    "version",
    "caps-label",
    "licences-label",
    "licence-text",
    "third-party",
    "folder-label",
    "open-folder",
    "show-welcome",
    "transfer-heading",
    "transfer-note",
    "backup-do",
    "restore-do",
    "import-do",
    "restore-yes",
    "updates-heading",
    "update-check",
    "update-auto",
    "update-note",
    "update-open",
];

fn under(name: &str) -> String {
    name.replace('-', "_")
}

/// True when `src` declares `kind` (`property` or `callback`) named `name`.
fn declares(src: &str, kind: &str, name: &str) -> bool {
    src.lines().any(|l| {
        let l = l.trim_start();
        if kind == "callback" {
            l.starts_with(&format!("callback {name}("))
        } else {
            l.contains("property <") && {
                let after = l.rsplit('>').next().unwrap_or_default().trim_start();
                after.starts_with(&format!("{name};")) || after.starts_with(&format!("{name}:"))
            }
        }
    })
}

#[test]
fn every_old_window_property_and_callback_still_exists_and_is_bound() {
    let slint = read("ui/settings.slint");
    let host = read("src/ui/settings_host.rs");
    assert_eq!(WINDOW_PROPERTIES.len(), 50);
    assert_eq!(WINDOW_CALLBACKS.len(), 34);
    for p in WINDOW_PROPERTIES {
        assert!(declares(&slint, "property", p), "property {p} is gone");
        if !NOT_BOUND_IN_RUST.contains(p) {
            let n = under(p);
            assert!(
                host.contains(&format!("set_{n}(")) || host.contains(&format!("get_{n}(")),
                "property {p} is not used by settings_host.rs"
            );
        }
    }
    for c in WINDOW_CALLBACKS {
        assert!(declares(&slint, "callback", c), "callback {c} is gone");
        assert!(
            host.contains(&format!("on_{}", under(c))),
            "callback {c} is not connected in settings_host.rs"
        );
    }
}

#[test]
fn every_old_text_still_exists_and_is_filled_by_set_strings() {
    let slint = read("ui/settings.slint");
    let host = read("src/ui/settings_host.rs");
    assert_eq!(STEXT_PROPERTIES.len(), 86);
    for p in STEXT_PROPERTIES {
        assert!(
            slint.contains(&format!("in property <string> {p};")),
            "SText.{p} is gone"
        );
        assert!(
            host.contains(&format!("s.set_{}(", under(p))),
            "SText.{p} is not filled in set_strings"
        );
    }
}

/// The `(section, id)` pairs of `src/prefs/sections.rs`.
fn table() -> Vec<(u8, String)> {
    let src = read("src/prefs/sections.rs");
    src.lines()
        .filter_map(|l| {
            let l = l.trim();
            let inner = l.strip_prefix('(')?.strip_suffix("),")?;
            let (n, id) = inner.split_once(", \"")?;
            Some((n.parse().ok()?, id.strip_suffix('"')?.to_string()))
        })
        .collect()
}

/// The `// section:N id` markers of `ui/settings.slint`, with the section block they sit in.
fn markers() -> Vec<(u8, String, Option<u8>)> {
    let slint = read("ui/settings.slint");
    let mut block: Option<u8> = None;
    let mut out = Vec::new();
    for line in slint.lines() {
        let l = line.trim();
        if let Some(rest) = l.split("if root.section == ").nth(1) {
            if let Some(n) = rest.split(':').next().and_then(|n| n.trim().parse().ok()) {
                block = Some(n);
            }
        }
        if let Some(rest) = l.strip_prefix("// section:") {
            let mut it = rest.split_whitespace();
            if let (Some(n), Some(id)) = (it.next(), it.next()) {
                if let Ok(n) = n.parse() {
                    out.push((n, id.to_string(), block));
                }
            }
        }
    }
    out
}

#[test]
fn every_setting_is_in_the_table_exactly_once() {
    let t = table();
    assert!(t.len() >= 25, "the table has only {} entries", t.len());
    let ids: BTreeSet<&str> = t.iter().map(|(_, id)| id.as_str()).collect();
    assert_eq!(
        ids.len(),
        t.len(),
        "an id is listed twice in SETTING_SECTIONS"
    );
    for (n, id) in &t {
        assert!(*n <= 4, "{id}: section {n} does not exist");
    }
    // The settings of the old window must all be placed.
    for id in [
        "shortcut",
        "language",
        "start-at-login",
        "popup-position",
        "close-on-focus-loss",
        "enter-pastes",
        "show-welcome",
        "capture-mode",
        "always-protected",
        "sensitive-content",
        "excluded-apps",
        "kinds",
        "skip-summary",
        "max-items",
        "max-age",
        "max-storage",
        "storage-stats",
        "cleanup",
        "clear-history",
        "backup",
        "restore",
        "import",
        "data-folder",
        "version",
        "auto-update",
        "check-updates",
        "capabilities",
        "licences",
    ] {
        assert!(ids.contains(id), "setting {id} is missing from the table");
    }
}

#[test]
fn the_markers_in_the_window_match_the_table() {
    let mut want = table();
    want.sort();
    let found = markers();
    let mut got: Vec<(u8, String)> = found.iter().map(|(n, id, _)| (*n, id.clone())).collect();
    got.sort();
    assert_eq!(
        got, want,
        "the `// section:N id` markers in settings.slint and SETTING_SECTIONS differ"
    );
    for (n, id, block) in found {
        assert_eq!(
            block,
            Some(n),
            "{id} is marked section {n} but sits in another section block"
        );
    }
}

#[test]
fn history_has_no_transfer_and_data_and_transfer_has_them() {
    let t = table();
    for id in ["backup", "restore", "import", "export", "data-folder"] {
        let places: Vec<u8> = t.iter().filter(|(_, i)| i == id).map(|(n, _)| *n).collect();
        assert!(
            places.iter().all(|n| *n == 3),
            "{id} must only be in Data and Transfer (3), found {places:?}"
        );
    }
    for id in ["backup", "restore", "import", "data-folder"] {
        assert!(
            t.iter().any(|(n, i)| *n == 3 && i == id),
            "{id} missing in 3"
        );
    }
    for (n, id) in &t {
        if *n == 2 {
            for bad in ["backup", "restore", "import", "export", "transfer"] {
                assert!(!id.contains(bad), "History and Storage holds {id}");
            }
        }
    }
}

const FORBIDDEN_IN_PROTECTED: &[&str] = &[
    "Switch",
    "Button",
    "TouchArea",
    "FocusScope",
    "callback",
    "set-",
    "clicked",
    "key-pressed",
];

#[test]
fn always_protected_has_no_control_no_callback_and_no_writable_property() {
    let slint = read("ui/settings.slint");
    let begin = slint.find("// always-protected:begin");
    let end = slint.find("// always-protected:end");
    let (Some(b), Some(e)) = (begin, end) else {
        panic!("the always-protected markers are missing from settings.slint");
    };
    assert!(b < e, "always-protected:end comes before :begin");
    let slice = &slint[b..e];
    assert!(slice.contains("ProtectedRow"), "the card shows no rows");
    for word in FORBIDDEN_IN_PROTECTED {
        assert!(
            !slice.contains(word),
            "the Always protected card contains `{word}`"
        );
    }
    // The row component is decoration too.
    let parts = read("ui/settings_parts.slint");
    let start = parts
        .find("component ProtectedRow")
        .unwrap_or_else(|| panic!("ProtectedRow is missing from settings_parts.slint"));
    let body = &parts[start..];
    let end = body[1..].find("\ncomponent ").map_or(body.len(), |i| i + 1);
    let body = &body[..end];
    for word in [
        "Switch",
        "Button",
        "TouchArea",
        "FocusScope",
        "callback",
        "clicked",
        "key-pressed",
        "in-out",
        "focus",
    ] {
        assert!(
            !body.contains(word),
            "ProtectedRow contains `{word}`: it must be decoration"
        );
    }
    // No property on the window itself that Rust could write for the markers (the `SText` labels
    // above it are fixed texts that existed before the redesign).
    let window_src = &slint[slint.find("export component SettingsWindow").unwrap_or(0)..];
    for line in window_src.lines().filter(|l| l.contains("property <")) {
        let l = line.to_ascii_lowercase();
        assert!(
            !(l.contains("protect") || l.contains("marker")),
            "a window property is about the markers: {line}"
        );
    }
    // Nothing in the host writes a marker setting to the window either (the labels are fixed
    // texts set on the `SText` global, `s.set_protected_os(..)`).
    let host = read("src/ui/settings_host.rs");
    for w in [
        "w.set_protected",
        "win.set_protected",
        "w.set_marker",
        "win.set_marker",
        "w.get_protected",
        "win.get_protected",
    ] {
        assert!(!host.contains(w), "settings_host.rs uses `{w}`");
    }
}

/// The names imported from `std-widgets.slint` in `src`.
fn std_widget_imports(src: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = src;
    while let Some(i) = rest.find("import {") {
        let after = &rest[i + "import {".len()..];
        let Some(close) = after.find('}') else { break };
        let names = &after[..close];
        let tail = &after[close..];
        if tail
            .lines()
            .next()
            .is_some_and(|l| l.contains("std-widgets.slint"))
        {
            out.extend(names.split(',').map(|n| n.trim().to_string()));
        }
        rest = &after[close..];
    }
    out.into_iter().filter(|n| !n.is_empty()).collect()
}

#[test]
fn no_std_widgets_form_control_is_left() {
    for file in ["ui/settings.slint", "ui/settings_parts.slint"] {
        let src = read(file);
        assert!(!src.is_empty(), "{file} is missing");
        for name in std_widget_imports(&src) {
            assert!(
                matches!(name.as_str(), "Palette" | "ScrollView"),
                "{file} imports the std-widgets form control {name}"
            );
        }
    }
}

#[test]
fn std_widget_import_scanner_works() {
    let src = "import { Button, Palette } from \"std-widgets.slint\";\nimport { Theme } from \"theme.slint\";";
    assert_eq!(std_widget_imports(src), vec!["Button", "Palette"]);
}

#[test]
fn one_confirm_dialog_per_former_overlay_and_no_overlay_component() {
    let slint = read("ui/settings.slint");
    assert!(
        !slint.contains("component Overlay"),
        "the old Overlay component is still defined"
    );
    assert!(!slint.contains("Overlay {"), "an Overlay is still used");
    assert_eq!(
        slint.matches("ConfirmDialog {").count(),
        5,
        "expected one ConfirmDialog per former overlay"
    );
    for open in [
        "root.confirm-remove-text != \"\"",
        "root.limits-open",
        "root.clear-open",
        "root.cleanup-open",
        "root.restore-open",
    ] {
        assert!(
            slint.contains(&format!("open: {open};")),
            "no ConfirmDialog opens on {open}"
        );
    }
    for cb in [
        "root.confirm-remove()",
        "root.cancel-remove()",
        "root.limits-confirm()",
        "root.limits-cancel()",
        "root.clear-confirm()",
        "root.clear-cancel()",
        "root.cleanup-confirm()",
        "root.cleanup-cancel()",
        "root.restore-confirm()",
        "root.restore-cancel()",
    ] {
        assert!(slint.contains(cb), "{cb} is no longer called");
    }
}

#[test]
fn the_save_bar_is_driven_by_rust_through_dirty() {
    let slint = read("ui/settings.slint");
    assert!(slint.contains("in property <bool> dirty;"));
    assert!(slint.contains("root.dirty"));
    assert!(slint.contains("callback key(string, bool, bool, bool, bool, bool) -> bool;"));
    assert!(slint.contains("capture-key-pressed"));
    let host = read("src/ui/settings_host.rs");
    assert!(host.contains("set_dirty("));
    assert!(host.contains("on_key("));
}
