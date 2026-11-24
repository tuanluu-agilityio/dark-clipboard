//! Regression test (Task 414): the UI must not draw symbols that the Windows UI font and the
//! software renderer cannot show (they render as empty space, as the gear and cross did in
//! 0.1.0). Icons are vector `Path`s in `ui/icons.slint`.
//!
//! Scans `ui/*.slint`, `src/**/*.rs` and `crates/darkclip-core/src/i18n.rs` for any character in
//! U+2190..=U+2BFF or U+1F000 and above, written literally or as a `\u{...}` escape. Comments
//! are excluded. Allow-list: `→` (U+2192) is in Segoe UI and used in key hints; `×`, `·` are
//! outside the scanned range.

use std::fs;
use std::path::{Path, PathBuf};

const ALLOWED: &[char] = &['\u{2192}'];
const BACKSLASH: char = '\u{5C}';
const ESCAPE_OPEN: &str = "\u{5C}u{";

fn risky(c: char) -> bool {
    let n = c as u32;
    ((0x2190..=0x2BFF).contains(&n) || n >= 0x1F000) && !ALLOWED.contains(&c)
}

fn collect(dir: &Path, ext: &str, out: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for e in entries.flatten() {
        let p = e.path();
        if p.is_dir() {
            collect(&p, ext, out);
        } else if p.extension().is_some_and(|x| x == ext) {
            out.push(p);
        }
    }
}

/// Removes `//` and `/* */` comments, keeping line structure. Tracks double-quoted strings.
fn strip_comments(src: &str) -> String {
    let chars: Vec<char> = src.chars().collect();
    let mut out = String::new();
    let (mut i, mut in_str, mut in_block) = (0, false, false);
    while i < chars.len() {
        let c = chars[i];
        let next = chars.get(i + 1).copied();
        if in_block {
            if c == '*' && next == Some('/') {
                in_block = false;
                i += 1;
            } else if c == '\n' {
                out.push('\n');
            }
        } else if in_str {
            out.push(c);
            if c == BACKSLASH {
                if let Some(n) = next {
                    out.push(n);
                    i += 1;
                }
            } else if c == '"' {
                in_str = false;
            }
        } else if c == '"' {
            in_str = true;
            out.push(c);
        } else if c == '/' && next == Some('/') {
            while i < chars.len() && chars[i] != '\n' {
                i += 1;
            }
            continue;
        } else if c == '/' && next == Some('*') {
            in_block = true;
            i += 1;
        } else {
            out.push(c);
        }
        i += 1;
    }
    out
}

/// Findings (line, description) for one source text.
fn scan(src: &str) -> Vec<(usize, String)> {
    let mut found = Vec::new();
    for (idx, line) in strip_comments(src).lines().enumerate() {
        for c in line.chars().filter(|c| risky(*c)) {
            found.push((idx + 1, format!("U+{:04X}", c as u32)));
        }
        let mut rest = line;
        while let Some(p) = rest.find(ESCAPE_OPEN) {
            let after = &rest[p + 3..];
            if let Some(end) = after.find('}') {
                if let Ok(n) = u32::from_str_radix(&after[..end], 16) {
                    if char::from_u32(n).is_some_and(risky) {
                        found.push((idx + 1, format!("escape U+{n:04X}")));
                    }
                }
            }
            rest = after;
        }
    }
    found
}

#[test]
fn scanner_finds_literals_and_escapes_and_skips_comments_and_allowed() {
    assert_eq!(scan("text: \"⚙\";").len(), 1);
    let star = format!("let s = \"{ESCAPE_OPEN}2605}}\";");
    let lock = format!("let s = \"{ESCAPE_OPEN}1F512}}\";");
    assert_eq!(scan(&star).len(), 1);
    assert_eq!(scan(&lock).len(), 1);
    assert!(scan(&format!("// ⚙ star {ESCAPE_OPEN}2605}}")).is_empty());
    assert!(scan("/* ✕\n ★ */ let a = 1;").is_empty());
    assert!(scan("\"a → b × c · d\"").is_empty());
    assert_eq!(scan("/* x */\n\"★\"")[0].0, 2);
}

#[test]
fn no_ui_source_uses_symbols_the_ui_font_cannot_draw() {
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut files = Vec::new();
    collect(&manifest.join("ui"), "slint", &mut files);
    collect(&manifest.join("src"), "rs", &mut files);
    files.push(manifest.join("../../crates/darkclip-core/src/i18n.rs"));
    assert!(
        files.len() > 10,
        "scan found too few files: {}",
        files.len()
    );
    let mut problems = Vec::new();
    for f in &files {
        let src = fs::read_to_string(f).unwrap_or_else(|e| panic!("read {}: {e}", f.display()));
        for (line, what) in scan(&src) {
            problems.push(format!("{}:{line}: {what}", f.display()));
        }
    }
    assert!(
        problems.is_empty(),
        "glyphs the UI font cannot draw (use a Path icon from ui/icons.slint):\n{}",
        problems.join("\n")
    );
}
