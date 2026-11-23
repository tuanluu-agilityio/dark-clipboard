//! Organisation view-models (Task 403): collections, tags, notes, favorites and the multi-select
//! actions of the history window and the popup. Pure: no window, no I/O. The hosts
//! (`ui/history_host.rs`, `ui/popup_host.rs`) call `App` and mirror these types into Slint; no
//! business rule lives in the markup. Collection and tag names and notes are user metadata:
//! shown in the UI, never logged (GC-06).

// The history window runs on Windows first; other platforms compile it but do not show it yet.
#![cfg_attr(not(windows), allow(dead_code))]

pub mod bulk;
pub mod collections;
pub mod dialog;
pub mod nav;
pub mod note;
pub mod tags;

/// `template` with `{name}` and `{n}` replaced.
pub fn fill_name(template: &str, name: &str, n: u64) -> String {
    template
        .replace("{name}", name)
        .replace("{n}", &n.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_name_containing_a_placeholder_stays_literal() {
        assert_eq!(
            fill_name("Add {n} clips to \"{name}\"?", "Top {n}", 3),
            "Add 3 clips to \"Top {n}\"?"
        );
        assert_eq!(fill_name("{name} {n}", "{name}", 2), "{name} 2");
    }
}
