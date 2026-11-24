//! Task 711 source scans for two Slint layout rules. No window is created (GC-19).
//!
//! 1. A kit `Menu` is a 0x0 anchor and `open(x, y)` takes window positions, so every instance
//!    must sit at the window origin (`x: 0px; y: 0px;`); Slint otherwise centres a child of the
//!    `Window` and every menu opens shifted by half the window size.
//! 2. A fixed-size child (an `Icon`) that is a direct child of a `VerticalLayout` caps the
//!    layout's cross-axis maximum width, so a stretched card renders content-sized. The
//!    `ModeCard` icon must be wrapped.

use std::fs;
use std::path::{Path, PathBuf};

fn manifest() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn slint_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for e in entries.flatten() {
        let p = e.path();
        if p.is_dir() {
            slint_files(&p, out);
        } else if p.extension().is_some_and(|x| x == "slint") {
            out.push(p);
        }
    }
}

fn indent(line: &str) -> usize {
    line.len() - line.trim_start().len()
}

/// The lines of the block opened at `lines[open]` (exclusive of the braces), found by indent:
/// the block ends at the first later line that is a `}` at the same indent.
fn block_end(lines: &[&str], open: usize) -> usize {
    let base = indent(lines[open]);
    for (i, l) in lines.iter().enumerate().skip(open + 1) {
        if indent(l) == base && l.trim_start().starts_with('}') {
            return i;
        }
    }
    lines.len()
}

#[test]
fn every_menu_instance_sits_at_the_window_origin() {
    let mut files = Vec::new();
    slint_files(&manifest().join("ui"), &mut files);
    assert!(!files.is_empty(), "no .slint files found under ui/");
    let mut instances = 0;
    let mut failures = Vec::new();
    for f in &files {
        let text = fs::read_to_string(f).unwrap_or_default();
        let lines: Vec<&str> = text.lines().collect();
        for (i, l) in lines.iter().enumerate() {
            let t = l.trim_start();
            // An instance is `name := Menu {`; the definition is `export component Menu`.
            if !t.contains(":= Menu {") {
                continue;
            }
            instances += 1;
            let end = block_end(&lines, i);
            let body = &lines[i + 1..end.min(lines.len())];
            let inner = indent(l) + 4;
            let has = |prop: &str| {
                body.iter()
                    .any(|b| indent(b) == inner && b.trim() == format!("{prop}: 0px;"))
            };
            if !(has("x") && has("y")) {
                failures.push(format!(
                    "{}:{}: Menu needs `x: 0px;` and `y: 0px;`",
                    f.display(),
                    i + 1
                ));
            }
        }
    }
    assert!(
        instances >= 4,
        "expected at least 4 Menu instances, found {instances}"
    );
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn mode_card_icon_is_not_a_direct_child_of_a_vertical_layout() {
    let path = manifest().join("ui/settings_parts.slint");
    let text = fs::read_to_string(&path).unwrap_or_default();
    let lines: Vec<&str> = text.lines().collect();
    let start = lines
        .iter()
        .position(|l| l.contains("export component ModeCard"));
    assert!(
        start.is_some(),
        "ModeCard not found in settings_parts.slint"
    );
    let start = start.unwrap_or(0);
    let end = block_end(&lines, start);
    let mut failures = Vec::new();
    let mut i = start;
    while i < end {
        if lines[i].trim_start().starts_with("VerticalLayout {") {
            let vend = block_end(&lines, i);
            let child = indent(lines[i]) + 4;
            for (j, l) in lines.iter().enumerate().take(vend).skip(i + 1) {
                if indent(l) == child && l.trim_start().starts_with("Icon {") {
                    failures.push(format!(
                        "{}:{}: Icon is a direct child of a VerticalLayout in ModeCard",
                        path.display(),
                        j + 1
                    ));
                }
            }
        }
        i += 1;
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
