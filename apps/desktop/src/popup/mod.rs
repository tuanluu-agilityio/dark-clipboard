//! Popup logic that does not need a window (Task 204): highlight ranges, the keyboard map, the
//! list view-model and placement. The Slint wiring is `ui/popup_host.rs`.

// The popup runs on Windows first; other platforms compile it but do not show it yet.
#![cfg_attr(not(windows), allow(dead_code))]

pub mod highlight;
pub mod keys;
pub mod list;
pub mod place;

/// How to bring a model from `old` to `new` without replacing rows needlessly (a replaced
/// chip loses the keyboard focus). `None`: the set changed (length or `key` per slot), rebuild.
/// `Some(indexes)`: same set, only these slots changed; update them in place.
pub fn in_place_changes<T: PartialEq, K: PartialEq>(
    old: &[T],
    new: &[T],
    key: impl Fn(&T) -> K,
) -> Option<Vec<usize>> {
    if old.len() != new.len() || old.iter().zip(new).any(|(a, b)| key(a) != key(b)) {
        return None;
    }
    Some(
        old.iter()
            .zip(new)
            .enumerate()
            .filter(|(_, (a, b))| a != b)
            .map(|(i, _)| i)
            .collect(),
    )
}

#[cfg(test)]
mod tests {
    use super::in_place_changes;

    /// (id, label, on)
    type C = (i32, &'static str, bool);

    #[test]
    fn toggling_a_chip_updates_only_that_slot() {
        let old: Vec<C> = vec![
            (0, "Text", false),
            (1, "Link", false),
            (100, "Pinned", false),
        ];
        let mut new = old.clone();
        new[1].2 = true;
        assert_eq!(in_place_changes(&old, &new, |c| c.0), Some(vec![1]));
        // Nothing changed: nothing to do.
        assert_eq!(in_place_changes(&old, &old, |c| c.0), Some(vec![]));
        // A relabel (language change) is in place too.
        let relabeled: Vec<C> = old.iter().map(|c| (c.0, "x", c.2)).collect();
        assert_eq!(
            in_place_changes(&old, &relabeled, |c| c.0),
            Some(vec![0, 1, 2])
        );
    }

    #[test]
    fn a_different_chip_set_is_rebuilt() {
        let old: Vec<C> = vec![(0, "Text", false), (200, "Editor", false)];
        let longer: Vec<C> = vec![
            (0, "Text", false),
            (200, "Editor", false),
            (201, "Web", false),
        ];
        assert_eq!(in_place_changes(&old, &longer, |c| c.0), None);
        let other: Vec<C> = vec![(0, "Text", false), (201, "Web", false)];
        assert_eq!(in_place_changes(&old, &other, |c| c.0), None);
        // Same ids, but a source chip now names another app: in place (same slot, same id).
        let renamed: Vec<C> = vec![(0, "Text", false), (200, "Web", false)];
        assert_eq!(in_place_changes(&old, &renamed, |c| c.0), Some(vec![1]));
    }
}
