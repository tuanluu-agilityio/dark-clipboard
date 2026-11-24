//! Task 707: accessibility scans over the `.slint` sources (A3, A6, tooltip on focus). No
//! window is created (GC-19); the parsing is the same plain text scanning as `tests/kit.rs`.

use std::fs;
use std::path::{Path, PathBuf};

fn manifest() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

/// Every `ui/**/*.slint` except the kit gallery (a developer page, never shipped in a window).
fn slint_files() -> Vec<PathBuf> {
    fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
        let Ok(entries) = fs::read_dir(dir) else {
            return;
        };
        for e in entries.flatten() {
            let p = e.path();
            if p.is_dir() {
                walk(&p, out);
            } else if p.extension().is_some_and(|x| x == "slint")
                && !p.ends_with("kit/gallery.slint")
            {
                out.push(p);
            }
        }
    }
    let mut out = Vec::new();
    walk(&manifest().join("ui"), &mut out);
    out.sort();
    out
}

fn read(p: &Path) -> String {
    fs::read_to_string(p).unwrap_or_default()
}

fn is_ident(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_' || c == '-'
}

/// One element instance: file, 1-based line, and its top-level `name: value;` bindings.
struct Instance {
    file: String,
    line: usize,
    props: Vec<(String, String)>,
}

impl Instance {
    fn prop(&self, name: &str) -> Option<&str> {
        self.props
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, v)| v.as_str())
    }
    fn at(&self) -> String {
        format!("{}:{}", self.file, self.line)
    }
}

/// Top-level bindings of a block body (depth 0 of `body`), `a: expr;` only.
fn top_level_props(body: &str) -> Vec<(String, String)> {
    let mut props = Vec::new();
    let mut depth = 0i32;
    let mut stmt = String::new();
    let mut in_str = false;
    let mut prev = ' ';
    for c in body.chars() {
        if c == '"' && prev != '\\' {
            in_str = !in_str;
        }
        if !in_str {
            match c {
                '{' => depth += 1,
                '}' => {
                    depth -= 1;
                    if depth == 0 {
                        stmt.clear();
                        prev = c;
                        continue;
                    }
                }
                _ => {}
            }
        }
        if depth == 0 && c == ';' && !in_str {
            if let Some((n, v)) = stmt.split_once(':') {
                let n = n.trim();
                if !n.is_empty() && n.chars().all(is_ident) {
                    props.push((n.to_string(), v.trim().to_string()));
                }
            }
            stmt.clear();
        } else if depth == 0 {
            stmt.push(c);
        }
        prev = c;
    }
    props
}

/// Strip `//` comments (no `//` appears inside our strings).
fn strip_comments(src: &str) -> String {
    src.lines()
        .map(|l| l.split("//").next().unwrap_or_default())
        .collect::<Vec<_>>()
        .join("\n")
}

/// Instances of component `name` in one source text (comments already stripped).
fn instances_in(src: &str, file: &str, name: &str) -> Vec<Instance> {
    let mut out = Vec::new();
    let mut from = 0;
    while let Some(rel) = src[from..].find(name) {
        let start = from + rel;
        from = start + name.len();
        let before = src[..start].chars().next_back().unwrap_or(' ');
        let after = &src[start + name.len()..];
        if is_ident(before) || !after.trim_start().starts_with('{') {
            continue;
        }
        // `component Name inherits ... {` and `inherits Name {` are definitions.
        let line_start = src[..start].rfind('\n').map_or(0, |i| i + 1);
        let head = &src[line_start..start];
        if head.contains("component ") || head.trim_end().ends_with("inherits") {
            continue;
        }
        let open = start + name.len() + (after.len() - after.trim_start().len());
        let mut depth = 0i32;
        let mut end = src.len();
        for (i, c) in src[open..].char_indices() {
            match c {
                '{' => depth += 1,
                '}' => {
                    depth -= 1;
                    if depth == 0 {
                        end = open + i;
                        break;
                    }
                }
                _ => {}
            }
        }
        out.push(Instance {
            file: file.to_string(),
            line: src[..start].matches('\n').count() + 1,
            props: top_level_props(&src[open + 1..end]),
        });
    }
    out
}

/// All instances of component `name` in `ui/**/*.slint` (`Name {`, `id := Name {`, `if c: Name {`).
fn instances(name: &str) -> Vec<Instance> {
    let mut out = Vec::new();
    for f in slint_files() {
        let src = strip_comments(&read(&f));
        let file = f
            .strip_prefix(manifest())
            .unwrap_or(&f)
            .to_string_lossy()
            .replace('\\', "/");
        out.extend(instances_in(&src, &file, name));
    }
    out
}

/// The text of `component Name inherits ... { ... }` in any file: (file, body).
fn definition(name: &str) -> Option<(String, String)> {
    for f in slint_files() {
        let src = format!("\n{}", strip_comments(&read(&f)));
        for key in [
            format!("export component {name} inherits"),
            format!("\ncomponent {name} inherits"),
        ] {
            if let Some(p) = src.find(&key) {
                let Some(open) = src[p..].find('{') else {
                    continue;
                };
                let open = p + open;
                let mut depth = 0i32;
                for (i, c) in src[open..].char_indices() {
                    match c {
                        '{' => depth += 1,
                        '}' => {
                            depth -= 1;
                            if depth == 0 {
                                return Some((
                                    f.display().to_string(),
                                    src[open..open + i].to_string(),
                                ));
                            }
                        }
                        _ => {}
                    }
                }
            }
        }
    }
    None
}

fn def_or_fail(name: &str) -> (String, String) {
    let d = definition(name);
    assert!(d.is_some(), "definition of {name} not found");
    d.unwrap_or_default()
}

/// A label is bound to a model, property or i18n global, never to a literal or left empty.
fn is_bound_label(v: &str) -> bool {
    let v = v.trim();
    !v.is_empty() && !v.starts_with('"')
}

/// (component, property that is its accessible name)
const NAMED: &[(&str, &str)] = &[
    ("IconButton", "label"),
    ("Button", "text"),
    ("NavItem", "text"),
    ("Switch", "label"),
    ("Select", "label"),
    ("Field", "label"),
    ("ModeCard", "label"),
];

/// Instances allowed to have no bound name, each with its reason. Empty on purpose: every
/// control in the app is named. Add an entry (`"ui/file.slint:LINE"`, reason) only with a reason.
const ALLOWED_UNNAMED: &[(&str, &str)] = &[];

#[test]
fn every_control_instance_binds_an_accessible_name() {
    let mut checked = 0;
    for (comp, prop) in NAMED {
        for i in instances(comp) {
            checked += 1;
            let allowed = ALLOWED_UNNAMED.iter().any(|(at, _)| *at == i.at());
            let ok = match i.prop(prop) {
                Some(v) => is_bound_label(v),
                // A `Field` can be named by its placeholder instead of a visible label.
                None if *comp == "Field" => i.prop("placeholder").is_some_and(is_bound_label),
                None => false,
            };
            assert!(
                ok || allowed,
                "{}: {comp} has no bound `{prop}` (accessible name)",
                i.at()
            );
        }
    }
    assert!(checked >= 70, "scan found only {checked} controls");
}

#[test]
fn every_control_definition_has_an_accessible_role() {
    for comp in NAMED
        .iter()
        .map(|(c, _)| *c)
        .chain(["MenuItem", "ConfirmDialog"])
    {
        let (file, body) = def_or_fail(comp);
        assert!(
            body.contains("accessible-role:"),
            "{file}: {comp} has no accessible-role"
        );
    }
}

/// Every component that takes the keyboard focus (`inherits FocusScope`) declares a role, so a
/// screen reader never meets an unnamed focus stop.
#[test]
fn every_focusable_component_has_a_role() {
    let mut seen = 0;
    for f in slint_files() {
        let src = strip_comments(&read(&f));
        for (i, line) in src.lines().enumerate() {
            let t = line.trim();
            let Some(rest) = t
                .strip_prefix("export component ")
                .or_else(|| t.strip_prefix("component "))
            else {
                continue;
            };
            if !rest.contains("inherits FocusScope") {
                continue;
            }
            let name = rest.split_whitespace().next().unwrap_or_default();
            let (_, body) = def_or_fail(name);
            seen += 1;
            assert!(
                body.contains("accessible-role:"),
                "{}:{}: FocusScope component {name} has no accessible-role",
                f.display(),
                i + 1
            );
        }
    }
    assert!(seen >= 8, "scan found only {seen} focusable components");
}

// ---- A6: hit targets --------------------------------------------------------------------------

/// A pixel literal such as `28px` from a binding value, if that is all it is.
fn px(v: &str) -> Option<f64> {
    v.trim().strip_suffix("px")?.trim().parse().ok()
}

#[test]
fn icon_buttons_are_at_least_28_px_at_every_instance() {
    let mut n = 0;
    for i in instances("IconButton") {
        n += 1;
        for dim in ["width", "height"] {
            // A computed size is covered by the kit's own 28 px default; a literal override
            // must not be smaller.
            if let Some(p) = i.prop(dim).and_then(px) {
                assert!(p >= 28.0, "{}: IconButton {dim} {p}px < 28px", i.at());
            }
        }
    }
    assert!(n >= 15, "found only {n} IconButtons");
    let (_, body) = def_or_fail("IconButton");
    assert!(body.contains("width: 28px;") && body.contains("height: 28px;"));
}

/// Nothing interactive is below 24 px: instance overrides by literal, definitions by their own
/// `height` / `min-height` literals.
#[test]
fn no_interactive_control_is_below_24_px() {
    for (comp, _) in NAMED {
        for i in instances(comp) {
            for dim in ["width", "height", "min-width", "min-height"] {
                if let Some(p) = i.prop(dim).and_then(px) {
                    assert!(p >= 24.0, "{}: {comp} {dim} {p}px < 24px", i.at());
                }
            }
        }
    }
    for comp in [
        "Button", "NavItem", "ModeCard", "Switch", "Select", "Field", "MenuItem",
    ] {
        let (file, body) = def_or_fail(comp);
        for (n, v) in top_level_props(&body) {
            if matches!(n.as_str(), "height" | "min-height" | "width" | "min-width") {
                if let Some(p) = px(&v) {
                    assert!(p >= 24.0, "{file}: {comp} {n} {p}px < 24px");
                }
            }
        }
    }
}

// ---- Tooltip on keyboard focus ----------------------------------------------------------------

#[test]
fn the_icon_button_raises_the_tooltip_hub_on_keyboard_focus_like_on_hover() {
    let (_, body) = def_or_fail("IconButton");
    assert!(
        body.contains("active: touch.has-hover || (root.focusable && root.has-focus);"),
        "IconButton's Tooltip must be active on hover or keyboard focus"
    );
}

/// Every IconButton shows something: its tooltip, or (empty tooltip) its label, which is
/// already required to be bound above.
#[test]
fn every_icon_button_has_a_tooltip_text() {
    for i in instances("IconButton") {
        let tip = i.prop("tooltip").is_some_and(is_bound_label);
        let label = i.prop("label").is_some_and(is_bound_label);
        assert!(
            tip || label,
            "{}: IconButton has neither tooltip nor label",
            i.at()
        );
    }
}

#[test]
fn every_window_has_one_tooltip_layer_as_its_last_child() {
    for name in ["app.slint", "history.slint"] {
        let src = strip_comments(&read(&manifest().join("ui").join(name)));
        assert_eq!(
            src.matches("TooltipLayer {").count(),
            1,
            "{name}: exactly one TooltipLayer"
        );
        let tip = src.rfind("TooltipLayer {").unwrap_or(0);
        let tail = src[tip..].replacen("TooltipLayer { }", "", 1);
        assert_eq!(
            tail.trim(),
            "}",
            "{name}: TooltipLayer must be the last root child"
        );
    }
    // The settings and welcome windows have no icon buttons, hence no tooltip to show.
    for name in ["settings.slint", "welcome.slint"] {
        let src = strip_comments(&read(&manifest().join("ui").join(name)));
        assert!(
            !src.contains("IconButton {"),
            "{name} has an IconButton but no TooltipLayer rule"
        );
    }
}

/// The scan itself: it finds a missing name and a literal one (so a pass means something).
#[test]
fn the_scan_detects_a_missing_or_literal_name() {
    let src = "IconButton { name: x; }\nif c: IconButton { label: \"Close\"; }\n               b := IconButton { label: Strings.close; tooltip: \"\"; }
               component IconButton inherits FocusScope { }";
    let v = instances_in(src, "t.slint", "IconButton");
    assert_eq!(v.len(), 3);
    assert!(v[0].prop("label").is_none());
    assert!(!is_bound_label(v[1].prop("label").unwrap_or_default()));
    assert!(is_bound_label(v[2].prop("label").unwrap_or_default()));
    assert_eq!(v[1].line, 2);
}

// ---- Focus return (D-082) ---------------------------------------------------------------------

/// Every control of the settings window that opens a dialog or the picker records itself in
/// `opener` right before it asks, and closing the layer hands the focus back through
/// `return-to` (each opener has a `changed` handler that focuses it). Slint 1.19 cannot name the
/// element that had the focus, so the openers say who they are.
#[test]
fn settings_openers_record_themselves_and_get_the_focus_back() {
    let src = strip_comments(&read(&manifest().join("ui/settings.slint")));
    let lines: Vec<&str> = src.lines().map(str::trim).collect();
    for call in [
        "root.open-picker();",
        "root.orphans-ask();",
        "root.compact-ask();",
        "root.clear-ask();",
        "root.restore-ask();",
        "root.remove-app(i);",
        "root.save();",
    ] {
        let at = lines.iter().position(|l| *l == call).unwrap_or(0);
        assert!(
            lines.get(at) == Some(&call),
            "settings.slint: {call} not found"
        );
        assert!(
            lines[at - 1].starts_with("root.opener = "),
            "settings.slint: {call} must be preceded by `root.opener = ...;`"
        );
    }
    let closed = src
        .find("changed layer-open")
        .map(|p| &src[p..p + 400])
        .unwrap_or_default();
    assert!(
        closed.contains("root.return-to = root.opener;"),
        "closing a layer must hand the focus back through return-to"
    );
    // One `changed` handler per opener kind that focuses itself.
    assert!(
        src.matches("root.return-to == ").count() >= 7,
        "each opener needs a `want` property on return-to"
    );
}
