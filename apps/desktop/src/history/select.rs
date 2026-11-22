//! Multi-select and delete confirmation for the history window (Task 211). Pure: no window, no
//! I/O. The host asks these types what to do; no business rule lives in the markup.
//!
//! Rules:
//! - A selection holds clip ids (never content). "Select all" means every *loaded* row.
//! - Actions run on the marked clips when there are any, else on the clip under the cursor.
//! - Delete never runs without a confirmation: `DeleteFlow::confirm` only returns ids after
//!   `request`, and only once.
//! - Copy needs exactly one target; masked clips may be selected and deleted (no content is
//!   read for that).

use darkclip_core::ClipId;

/// The marked clips (Ctrl+click, Shift+click or arrows, Space, Ctrl+A).
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct MultiSelect {
    marked: Vec<ClipId>,
    /// Where a Shift range starts.
    anchor: Option<ClipId>,
}

impl MultiSelect {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn is_empty(&self) -> bool {
        self.marked.is_empty()
    }

    pub fn contains(&self, id: ClipId) -> bool {
        self.marked.contains(&id)
    }

    /// A plain click or move: marks are dropped, the clicked row becomes the anchor.
    pub fn plain(&mut self, anchor: Option<ClipId>) {
        self.marked.clear();
        self.anchor = anchor;
    }

    /// Ctrl+click or Space on `id`: adds or removes it. When nothing was marked yet, the clip
    /// under the cursor (`cursor_before`) is marked first, like a file manager.
    pub fn toggle(&mut self, id: ClipId, cursor_before: Option<ClipId>) {
        if self.marked.is_empty() {
            if let Some(c) = cursor_before.filter(|c| *c != id) {
                self.marked.push(c);
            }
        }
        if let Some(pos) = self.marked.iter().position(|m| *m == id) {
            self.marked.remove(pos);
        } else {
            self.marked.push(id);
        }
        self.anchor = Some(id);
    }

    /// Shift+click or Shift+arrow to row `to` of `ids` (the visible list): marks the range
    /// from the anchor (or the cursor when there is none) to `to`, inclusive.
    pub fn extend(&mut self, ids: &[ClipId], to: usize, cursor_before: Option<ClipId>) {
        let Some(target) = ids.get(to).copied() else {
            return;
        };
        let from_id = self.anchor.or(cursor_before).unwrap_or(target);
        let from = ids.iter().position(|i| *i == from_id).unwrap_or(to);
        let (lo, hi) = (from.min(to), from.max(to));
        self.marked = ids[lo..=hi].to_vec();
        if self.anchor.is_none() {
            self.anchor = Some(from_id);
        }
    }

    /// Ctrl+A: every loaded row.
    pub fn select_all(&mut self, ids: &[ClipId]) {
        self.marked = ids.to_vec();
        self.anchor = ids.first().copied();
    }

    /// Rows went away (a refresh, a delete): marks of clips no longer listed are dropped.
    pub fn prune(&mut self, ids: &[ClipId]) {
        self.marked.retain(|m| ids.contains(m));
        if self.anchor.is_some_and(|a| !ids.contains(&a)) {
            self.anchor = None;
        }
    }

    /// The clips an action applies to, in list order: the marked ones, else the one under the
    /// cursor.
    pub fn targets(&self, ids: &[ClipId], cursor: Option<ClipId>) -> Vec<ClipId> {
        if self.marked.is_empty() {
            return cursor.into_iter().collect();
        }
        ids.iter()
            .copied()
            .filter(|i| self.marked.contains(i))
            .collect()
    }

    /// One flag per row: marked or not.
    pub fn flags(&self, ids: &[ClipId]) -> Vec<bool> {
        ids.iter().map(|i| self.marked.contains(i)).collect()
    }
}

/// Copy and "copy as plain text" need exactly one clip.
pub fn can_copy(targets: &[ClipId]) -> bool {
    targets.len() == 1
}

/// What a pin action does to `targets` (id, pinned): if every one is pinned they are unpinned,
/// otherwise the unpinned ones are pinned. Returns the new state and the clips to change.
pub fn pin_plan(targets: &[(ClipId, bool)]) -> (bool, Vec<ClipId>) {
    let all_pinned = !targets.is_empty() && targets.iter().all(|t| t.1);
    let pin = !all_pinned;
    let change = targets.iter().filter(|t| t.1 != pin).map(|t| t.0).collect();
    (pin, change)
}

/// The confirmation step before a delete.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct DeleteFlow {
    pending: Option<Vec<ClipId>>,
}

impl DeleteFlow {
    pub fn new() -> Self {
        Self::default()
    }

    /// Opens the confirmation for `ids`. False (nothing opens) for an empty list.
    pub fn request(&mut self, ids: Vec<ClipId>) -> bool {
        if ids.is_empty() {
            return false;
        }
        self.pending = Some(ids);
        true
    }

    pub fn is_confirming(&self) -> bool {
        self.pending.is_some()
    }

    /// How many clips the open confirmation would delete.
    pub fn count(&self) -> Option<usize> {
        self.pending.as_ref().map(Vec::len)
    }

    /// The user confirmed: the ids to delete, once. `None` when no confirmation was open.
    pub fn confirm(&mut self) -> Option<Vec<ClipId>> {
        self.pending.take()
    }

    pub fn cancel(&mut self) {
        self.pending = None;
    }

    /// Rows went away while the confirmation was open: ids no longer listed are dropped, and
    /// the confirmation closes when none are left.
    pub fn retain(&mut self, ids: &[ClipId]) {
        if let Some(p) = &mut self.pending {
            p.retain(|i| ids.contains(i));
            if p.is_empty() {
                self.pending = None;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use darkclip_core::Timestamp;

    fn ids(n: usize) -> Vec<ClipId> {
        (0..n)
            .map(|i| ClipId::new_v7(Timestamp(1_795_320_000_000 + i64::try_from(i).unwrap())))
            .collect()
    }

    #[test]
    fn nothing_marked_targets_the_cursor_clip() {
        let v = ids(5);
        let s = MultiSelect::new();
        assert_eq!(s.targets(&v, Some(v[2])), vec![v[2]]);
        assert_eq!(s.targets(&v, None), Vec::<ClipId>::new());
    }

    #[test]
    fn ctrl_click_marks_the_cursor_clip_first_then_toggles() {
        let v = ids(5);
        let mut s = MultiSelect::new();
        s.toggle(v[3], Some(v[1]));
        assert_eq!(s.targets(&v, Some(v[3])), vec![v[1], v[3]]);
        // Toggle again removes it.
        s.toggle(v[3], Some(v[3]));
        assert_eq!(s.targets(&v, Some(v[3])), vec![v[1]]);
        // Clicking the cursor clip itself when nothing is marked marks only it.
        let mut t = MultiSelect::new();
        t.toggle(v[2], Some(v[2]));
        assert_eq!(t.targets(&v, Some(v[2])), vec![v[2]]);
    }

    #[test]
    fn shift_extends_from_the_anchor_in_either_direction() {
        let v = ids(8);
        let mut s = MultiSelect::new();
        s.plain(Some(v[2]));
        s.extend(&v, 5, Some(v[2]));
        assert_eq!(s.targets(&v, Some(v[5])), v[2..=5].to_vec());
        // Shrinking back keeps the anchor.
        s.extend(&v, 3, Some(v[5]));
        assert_eq!(s.targets(&v, Some(v[3])), v[2..=3].to_vec());
        // Past the anchor, upwards.
        s.extend(&v, 0, Some(v[3]));
        assert_eq!(s.targets(&v, Some(v[0])), v[0..=2].to_vec());
    }

    #[test]
    fn shift_without_an_anchor_starts_at_the_cursor() {
        let v = ids(6);
        let mut s = MultiSelect::new();
        s.extend(&v, 3, Some(v[1]));
        assert_eq!(s.targets(&v, Some(v[3])), v[1..=3].to_vec());
        // An out-of-range row changes nothing.
        let before = s.clone();
        s.extend(&v, 99, Some(v[1]));
        assert_eq!(s, before);
    }

    #[test]
    fn select_all_marks_every_loaded_row_and_plain_clears() {
        let v = ids(4);
        let mut s = MultiSelect::new();
        s.select_all(&v);
        assert_eq!(s.targets(&v, Some(v[0])), v);
        assert_eq!(s.flags(&v), vec![true; 4]);
        s.plain(Some(v[1]));
        assert!(s.is_empty());
        assert_eq!(s.flags(&v), vec![false; 4]);
    }

    #[test]
    fn prune_drops_clips_that_left_the_list() {
        let v = ids(5);
        let mut s = MultiSelect::new();
        s.select_all(&v);
        s.prune(&[v[1], v[3]]);
        assert_eq!(s.targets(&[v[1], v[3]], None), vec![v[1], v[3]]);
        assert!(!s.contains(v[0]));
        // Targets follow the list order, not the marking order.
        let mut t = MultiSelect::new();
        t.toggle(v[4], None);
        t.toggle(v[0], None);
        assert_eq!(t.targets(&v, None), vec![v[0], v[4]]);
    }

    #[test]
    fn copy_needs_exactly_one_target() {
        let v = ids(3);
        assert!(can_copy(&v[..1]));
        assert!(!can_copy(&v[..2]));
        assert!(!can_copy(&[]));
    }

    #[test]
    fn pin_plan_unpins_only_when_everything_is_pinned() {
        let v = ids(3);
        assert_eq!(
            pin_plan(&[(v[0], true), (v[1], true)]),
            (false, vec![v[0], v[1]])
        );
        assert_eq!(pin_plan(&[(v[0], true), (v[1], false)]), (true, vec![v[1]]));
        assert_eq!(pin_plan(&[(v[0], false)]), (true, vec![v[0]]));
        assert_eq!(pin_plan(&[]), (true, vec![]));
    }

    #[test]
    fn delete_needs_a_confirmation_and_only_runs_once() {
        let v = ids(3);
        let mut f = DeleteFlow::new();
        // Nothing to confirm yet: nothing is returned.
        assert_eq!(f.confirm(), None);
        assert!(!f.request(vec![]));
        assert!(!f.is_confirming());
        assert!(f.request(v.clone()));
        assert_eq!(f.count(), Some(3));
        assert_eq!(f.confirm(), Some(v));
        // The confirmation is used up.
        assert!(!f.is_confirming());
        assert_eq!(f.confirm(), None);
    }

    #[test]
    fn cancel_deletes_nothing() {
        let v = ids(2);
        let mut f = DeleteFlow::new();
        f.request(v);
        f.cancel();
        assert_eq!(f.confirm(), None);
        assert_eq!(f.count(), None);
    }

    #[test]
    fn rows_that_vanish_shrink_or_close_the_confirmation() {
        let v = ids(3);
        let mut f = DeleteFlow::new();
        f.request(v.clone());
        f.retain(&[v[0], v[2]]);
        assert_eq!(f.count(), Some(2));
        f.retain(&[]);
        assert!(!f.is_confirming());
        assert_eq!(f.confirm(), None);
    }
}
