//! Task 700 tests for the shared UI kit (`ui/kit`, `ui/icons`). No window is created (GC-19):
//! these are source scans and pure key maps.
//!
//! The tooltip rule below is the specification that `ui/kit/tooltip.slint` mirrors (Slint cannot
//! call Rust from a pure expression without a callback). The menu's key map moved to Rust in
//! Task 701 (F-31): `src/popup/menu.rs` owns it with its tests, and `ui/kit/menu.slint` only
//! forwards keys; a scan test below checks that.

use std::fs;
use std::path::{Path, PathBuf};

fn manifest() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn files_with_ext(dir: &Path, ext: &str) -> Vec<PathBuf> {
    // A missing folder gives an empty list; the tests that need files assert on the count.
    let Ok(entries) = fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut out: Vec<PathBuf> = entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == ext))
        .collect();
    out.sort();
    out
}

fn kit_sources() -> String {
    let mut all = String::new();
    for f in files_with_ext(&manifest().join("ui/kit"), "slint") {
        all.push_str(&fs::read_to_string(&f).unwrap_or_default());
        all.push('\n');
    }
    all
}

/// Every `@image-url("...")` path written in the kit, as found in the source.
fn referenced_svgs() -> Vec<String> {
    let src = kit_sources();
    let key = "@image-url(\"";
    let mut out = Vec::new();
    let mut rest = src.as_str();
    while let Some(p) = rest.find(key) {
        let after = &rest[p + key.len()..];
        let Some(end) = after.find('"') else { break };
        out.push(after[..end].to_string());
        rest = &after[end..];
    }
    out
}

#[test]
fn every_svg_referenced_from_the_kit_exists() {
    let refs = referenced_svgs();
    assert!(
        refs.len() >= 40,
        "found only {} icon references",
        refs.len()
    );
    for r in refs {
        let path = manifest().join("ui/kit").join(&r);
        assert!(path.is_file(), "kit references a missing file: {r}");
    }
}

#[test]
fn every_vendored_icon_is_referenced_by_the_kit() {
    let refs = referenced_svgs();
    let icons = files_with_ext(&manifest().join("ui/icons"), "svg");
    assert!(icons.len() >= 40, "found only {} icons", icons.len());
    for f in icons {
        let name = f.file_name().and_then(|n| n.to_str()).unwrap_or_default();
        assert!(
            refs.iter().any(|r| r.ends_with(&format!("icons/{name}"))),
            "dead icon (not referenced from ui/kit): {name}"
        );
    }
}

#[test]
fn icons_are_vendored_with_licence_and_readme() {
    let dir = manifest().join("ui/icons");
    let lic = fs::read_to_string(dir.join("LICENSE")).unwrap_or_default();
    assert!(lic.contains("ISC License"), "ISC text missing");
    let readme = fs::read_to_string(dir.join("README.md")).unwrap_or_default();
    assert!(readme.contains("lucide-static"), "version source missing");
    assert!(readme.contains("1.79.0"), "pinned version missing");
}

#[test]
fn lucide_attribution_is_in_the_third_party_notices_template() {
    let hbs = fs::read_to_string(manifest().join("../../about.hbs")).unwrap_or_default();
    assert!(hbs.contains("Lucide"), "about.hbs lacks the Lucide notice");
    assert!(hbs.contains("ISC License"), "about.hbs lacks the ISC text");
}

#[test]
fn old_path_icons_are_gone_and_not_imported() {
    assert!(
        !manifest().join("ui/icons.slint").exists(),
        "ui/icons.slint should be deleted"
    );
    for f in files_with_ext(&manifest().join("ui"), "slint") {
        let src = fs::read_to_string(&f).unwrap_or_default();
        assert!(
            !src.contains("icons.slint") && !src.contains("IconKind"),
            "{} still uses the Task 414 Path icons",
            f.display()
        );
    }
}

/// True when `line` holds a `#rgb`, `#rrggbb` or `#rrggbbaa` colour literal.
fn has_hex_literal(line: &str) -> bool {
    let b = line.as_bytes();
    (0..b.len()).any(|i| {
        b[i] == b'#' && {
            let n = b[i + 1..]
                .iter()
                .take_while(|c| c.is_ascii_hexdigit())
                .count();
            matches!(n, 3 | 4 | 6 | 8)
        }
    })
}

#[test]
fn hex_literal_detector_works() {
    assert!(has_hex_literal("background: #1C1E22;"));
    assert!(has_hex_literal("color: #fff;"));
    assert!(!has_hex_literal("// a # comment"));
    assert!(!has_hex_literal("background: Theme.canvas;"));
}

#[test]
fn kit_colours_come_from_the_theme_only() {
    let files = files_with_ext(&manifest().join("ui/kit"), "slint");
    assert!(files.len() >= 8, "found only {} kit files", files.len());
    for f in files {
        let src = fs::read_to_string(&f).unwrap_or_default();
        for (i, line) in src.lines().enumerate() {
            let code = line.split("//").next().unwrap_or_default();
            assert!(
                !has_hex_literal(code),
                "{}:{}: hex colour literal outside Theme: {line}",
                f.display(),
                i + 1
            );
        }
    }
}

#[test]
fn icon_buttons_are_at_least_28_px() {
    let src = fs::read_to_string(manifest().join("ui/kit/icon_button.slint")).unwrap_or_default();
    assert!(src.contains("width: 28px;"), "IconButton default width");
    assert!(src.contains("height: 28px;"), "IconButton default height");
}

// ---- Pure key maps (specification mirrored by the Slint kit) ----------------------------------

/// A tooltip is shown after 500 ms of hover or keyboard focus.
fn tooltip_visible(hovered: bool, focused: bool, held_ms: u32) -> bool {
    (hovered || focused) && held_ms >= 500
}

#[test]
fn tooltip_shows_after_500_ms_on_hover_or_keyboard_focus() {
    assert!(!tooltip_visible(true, false, 499));
    assert!(tooltip_visible(true, false, 500));
    assert!(tooltip_visible(false, true, 500));
    assert!(!tooltip_visible(false, false, 5000));
}

#[test]
fn the_slint_menu_forwards_its_keys_to_the_host_and_does_not_decide_them() {
    let menu = fs::read_to_string(manifest().join("ui/kit/menu.slint")).unwrap_or_default();
    assert!(
        menu.contains("root.key("),
        "menu.slint must forward keys through key(...)"
    );
    assert!(
        menu.contains("callback key(string) -> bool;"),
        "menu.slint must declare key(string) -> bool"
    );
    for decided in [
        "Key.DownArrow",
        "Key.UpArrow",
        "Key.Home",
        "Key.End",
        "Key.Return",
        "Key.Escape",
    ] {
        assert!(
            !menu.contains(decided),
            "menu.slint decides {decided}: that is `popup::menu::menu_key`'s job"
        );
    }
    // The host drives the highlight: it is an `in` property, not `in-out`.
    assert!(
        menu.contains("in property <int> highlighted;"),
        "the highlight is driven by the host"
    );
    assert!(!menu.contains("in-out property <int> highlighted"));
    let tip = fs::read_to_string(manifest().join("ui/kit/tooltip.slint")).unwrap_or_default();
    assert!(tip.contains("500ms"), "tooltip delay is not 500 ms");
}

#[test]
fn every_menu_key_is_in_the_rust_key_map() {
    // The keys the design asks of a menu (`04` section 1.7) exist as `MenuKey` variants.
    let rust = fs::read_to_string(manifest().join("src/popup/menu.rs")).unwrap_or_default();
    for variant in [
        "Up", "Down", "Home", "End", "Enter", "Escape", "Left", "Right", "Letter",
    ] {
        assert!(
            rust.contains(&format!("    {variant}")),
            "MenuKey::{variant} is missing"
        );
    }
}

#[test]
fn a_switch_swallows_enter_so_a_dialog_is_never_confirmed_from_a_toggle() {
    let sw = fs::read_to_string(manifest().join("ui/kit/switch.slint")).unwrap_or_default();
    assert!(
        sw.contains("e.text == Key.Return"),
        "switch.slint must accept Enter itself"
    );
    // ConfirmDialog confirms on Enter when the key reaches it, which is why the Switch above
    // must not let it through.
    let dlg =
        fs::read_to_string(manifest().join("ui/kit/confirm_dialog.slint")).unwrap_or_default();
    assert!(dlg.contains("e.text == Key.Return"));
}

// ---- Task 706 ----

#[test]
fn the_empty_state_is_used_by_the_real_windows() {
    // Slint compiles only what a window instantiates, so a real window must use it.
    for name in ["ui/app.slint", "ui/history.slint"] {
        let src = fs::read_to_string(manifest().join(name)).unwrap_or_default();
        assert!(
            src.contains("import { EmptyState }"),
            "{name}: EmptyState import"
        );
        assert!(src.contains("EmptyState {"), "{name}: EmptyState instance");
    }
    let es = fs::read_to_string(manifest().join("ui/kit/empty_state.slint")).unwrap_or_default();
    // 32 px icon in text-3, 13 px line in text-2, 12 px hint in text-3, one accessible label.
    assert!(es.contains("size: 32px;") && es.contains("tint: Theme.text-3;"));
    assert!(es.contains("font-size: 13px;") && es.contains("font-size: 12px;"));
    assert!(es.contains("accessible-label: root.hint != \"\" ? root.text + \". \" + root.hint"));
}

/// The windows' secondary surfaces (Task 706) are in-tree overlays, never `PopupWindow`s, and
/// take their colours from the theme and their text from i18n.
#[test]
fn the_secondary_surfaces_are_overlays_with_theme_colours_and_no_literal_text() {
    for name in [
        "transform.slint",
        "organize.slint",
        "welcome.slint",
        "kit/empty_state.slint",
    ] {
        let src = fs::read_to_string(manifest().join("ui").join(name)).unwrap_or_default();
        assert!(!src.is_empty(), "{name} is missing");
        for (i, line) in src.lines().enumerate() {
            let code = line.split("//").next().unwrap_or_default();
            assert!(
                !has_hex_literal(code),
                "{name}:{}: hex colour literal: {line}",
                i + 1
            );
            assert!(
                !code.contains("PopupWindow"),
                "{name}:{}: a PopupWindow would take the keys from the host (D-079)",
                i + 1
            );
            for key in ["text: \"", "label: \"", "placeholder: \""] {
                assert!(
                    !code.contains(key) || code.contains("text: \"\""),
                    "{name}:{}: literal user-visible string: {line}",
                    i + 1
                );
            }
        }
    }
    // The sheet and the dialogs sit on the kit scrim.
    let tf = fs::read_to_string(manifest().join("ui/transform.slint")).unwrap_or_default();
    assert!(tf.contains("export component ShortcutSheet inherits DialogScrim"));
    assert!(tf.contains("export component TransformPanel inherits DialogScrim"));
    let org = fs::read_to_string(manifest().join("ui/organize.slint")).unwrap_or_default();
    assert!(org.contains("export component OrgDialog inherits DialogScrim"));
}

/// The sheet sits before the tooltip layer, which stays the last child of each window root.
#[test]
fn the_tooltip_layer_stays_last_and_the_sheet_comes_just_before_it() {
    for name in ["app.slint", "history.slint"] {
        let src = fs::read_to_string(manifest().join("ui").join(name)).unwrap_or_default();
        let sheet = src.find("ShortcutSheet {").unwrap_or(usize::MAX);
        let tip = src.rfind("TooltipLayer { }").unwrap_or(0);
        assert!(
            sheet < tip,
            "{name}: the sheet must come before TooltipLayer"
        );
        assert!(
            src[tip..].trim_end().ends_with("}\n}") || src[tip + 16..].trim() == "}",
            "{name}: TooltipLayer must be the last child"
        );
    }
}

/// Welcome: Esc is "Not now", Enter presses the focused button and Start has the focus, the
/// switch is hidden without `can-login`; nothing but `begin` is wired to the host.
#[test]
fn the_welcome_keeps_its_keys_and_its_callbacks() {
    let w = fs::read_to_string(manifest().join("ui/welcome.slint")).unwrap_or_default();
    assert!(w.contains("e.text == Key.Escape") && w.contains("root.dismiss();"));
    assert!(w.contains("if root.can-login && !root.finished: HorizontalLayout"));
    assert!(w.contains("root.begin();") && w.contains("root.not-now();"));
    // Start takes the focus when the window opens, so Enter starts; Not now never does.
    let start = w.find("text: WText.start;").unwrap_or(0);
    assert!(w[start..].contains("self.focus();"));
    for callback in [
        "callback begin();",
        "callback not-now();",
        "callback dismiss();",
    ] {
        assert!(w.contains(callback), "{callback}");
    }
    // The old widgets are gone (the kit has the switch and the buttons).
    assert!(!w.contains("CheckBox") && !w.contains("std-widgets.slint\";\nimport { Button"));
}
