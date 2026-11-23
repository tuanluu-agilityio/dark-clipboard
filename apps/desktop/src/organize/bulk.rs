//! Actions on the marked clips (Task 403): favorite or unfavorite, add to or remove from a
//! collection, add a tag. Pure: the plan says which clips an action touches and what the
//! confirmation says; the host runs it through `App`.
//!
//! Masked clips take no add action (D-060): the API would answer NotFound, so the plan leaves
//! them out and the confirmation counts only the clips that will change. Taking something away
//! (unfavorite, remove from a collection) works on masked clips too.

use super::fill_name;
use super::tags;
use crate::popup::list::Item;
use darkclip_core::i18n::{t, Lang, Msg};
use darkclip_core::{ClipId, CollectionId};

/// What the plan needs to know about one marked clip.
#[derive(Clone, PartialEq, Eq)]
pub struct Target {
    pub id: ClipId,
    pub masked: bool,
    pub favorite: bool,
    pub tags: Vec<String>,
}

/// Never prints tag names.
impl std::fmt::Debug for Target {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Target")
            .field("id", &self.id)
            .field("masked", &self.masked)
            .field("favorite", &self.favorite)
            .finish_non_exhaustive()
    }
}

impl From<&Item> for Target {
    fn from(i: &Item) -> Self {
        Target {
            id: i.id,
            masked: i.masked,
            favorite: i.favorite,
            tags: i.tags.clone(),
        }
    }
}

#[derive(Clone, PartialEq, Eq)]
pub enum BulkOp {
    /// Set the favorite flag to this value.
    Favorite(bool),
    AddToCollection {
        id: CollectionId,
        name: String,
    },
    RemoveFromCollection {
        id: CollectionId,
        name: String,
    },
    /// A normalised tag name.
    AddTag(String),
}

/// Never prints names.
impl std::fmt::Debug for BulkOp {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            BulkOp::Favorite(on) => write!(f, "Favorite({on})"),
            BulkOp::AddToCollection { .. } => f.write_str("AddToCollection"),
            BulkOp::RemoveFromCollection { .. } => f.write_str("RemoveFromCollection"),
            BulkOp::AddTag(_) => f.write_str("AddTag"),
        }
    }
}

/// One clip to change. `tags` is the complete new tag list for `AddTag` (empty otherwise).
#[derive(Clone, PartialEq, Eq)]
pub struct Job {
    pub id: ClipId,
    pub tags: Vec<String>,
}

#[derive(Clone, PartialEq, Eq)]
pub struct BulkPlan {
    op: BulkOp,
    jobs: Vec<Job>,
    skipped_masked: usize,
    skipped_full: usize,
}

impl std::fmt::Debug for BulkPlan {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BulkPlan")
            .field("op", &self.op)
            .field("jobs", &self.jobs.len())
            .finish()
    }
}

/// The favorite toggle for one clip: the state to set, or `None` when the clip is masked and
/// not a favorite (a masked clip can never be added to Favorites, D-060).
pub fn favorite_toggle(item: &Item) -> Option<bool> {
    if item.masked && !item.favorite {
        None
    } else {
        Some(!item.favorite)
    }
}

/// What a favorite action sets for several clips: unfavorite when every one is a favorite,
/// otherwise favorite. `None` when nothing would change (all masked non-favorites, or none).
pub fn favorite_target(targets: &[Target]) -> Option<bool> {
    if targets.is_empty() {
        return None;
    }
    // Something can be added: favorite. Otherwise unfavorite the favorites, if any.
    if targets.iter().any(|t| !t.masked && !t.favorite) {
        Some(true)
    } else {
        targets.iter().any(|t| t.favorite).then_some(false)
    }
}

impl BulkPlan {
    /// The plan for `op` on `targets`, or `None` when no clip would change.
    pub fn new(op: BulkOp, targets: &[Target]) -> Option<BulkPlan> {
        let mut jobs = Vec::new();
        let (mut skipped_masked, mut skipped_full) = (0, 0);
        // Taking something away works on masked clips; adding never does (D-060).
        let adds = matches!(
            op,
            BulkOp::Favorite(true) | BulkOp::AddToCollection { .. } | BulkOp::AddTag(_)
        );
        for t in targets {
            if adds && t.masked {
                skipped_masked += 1;
                continue;
            }
            let job_tags = match &op {
                BulkOp::Favorite(true) if t.favorite => continue,
                BulkOp::Favorite(false) if !t.favorite => continue,
                BulkOp::AddTag(tag) => match tags::merged(&t.tags, tag) {
                    Ok(Some(list)) => list,
                    // The clip has the tag already.
                    Ok(None) => continue,
                    Err(Msg::OrgTagsFull) => {
                        skipped_full += 1;
                        continue;
                    }
                    // An invalid name: nothing can run.
                    Err(_) => return None,
                },
                _ => Vec::new(),
            };
            jobs.push(Job {
                id: t.id,
                tags: job_tags,
            });
        }
        if jobs.is_empty() {
            return None;
        }
        Some(BulkPlan {
            op,
            jobs,
            skipped_masked,
            skipped_full,
        })
    }

    pub fn op(&self) -> &BulkOp {
        &self.op
    }

    pub fn jobs(&self) -> &[Job] {
        &self.jobs
    }

    /// Clips that will change.
    pub fn count(&self) -> usize {
        self.jobs.len()
    }

    /// False for "remove from collection": the marked clips that are not in it are counted
    /// but unchanged, so the result must not claim a number.
    pub fn count_is_exact(&self) -> bool {
        !matches!(self.op, BulkOp::RemoveFromCollection { .. })
    }

    pub fn skipped_masked(&self) -> usize {
        self.skipped_masked
    }

    pub fn skipped_full(&self) -> usize {
        self.skipped_full
    }

    /// More than one clip changes: the host asks first and shows the count.
    pub fn needs_confirmation(&self) -> bool {
        self.jobs.len() > 1
    }

    /// The question, with the exact count.
    pub fn confirm_text(&self, lang: Lang) -> String {
        let n = self.jobs.len() as u64;
        let (msg, name) = match &self.op {
            BulkOp::Favorite(true) => (Msg::OrgConfirmFavorite, ""),
            BulkOp::Favorite(false) => (Msg::OrgConfirmUnfavorite, ""),
            BulkOp::AddToCollection { name, .. } => (Msg::OrgConfirmAddToCollection, name.as_str()),
            BulkOp::RemoveFromCollection { name, .. } => {
                (Msg::OrgConfirmRemoveFromCollection, name.as_str())
            }
            BulkOp::AddTag(tag) => (Msg::OrgConfirmAddTag, tag.as_str()),
        };
        fill_name(t(lang, msg), name, n)
    }

    /// "2 masked clips were skipped." and "1 clips already have 20 tags.", when any.
    pub fn skipped_text(&self, lang: Lang) -> Option<String> {
        let mut parts = Vec::new();
        if self.skipped_masked() > 0 {
            parts.push(fill_name(
                t(lang, Msg::OrgSkippedMasked),
                "",
                self.skipped_masked() as u64,
            ));
        }
        if self.skipped_full() > 0 {
            parts.push(fill_name(
                t(lang, Msg::OrgSkippedFull),
                "",
                self.skipped_full() as u64,
            ));
        }
        (!parts.is_empty()).then(|| parts.join(" "))
    }
}

/// The confirmation step before a plan with several clips runs.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct BulkFlow {
    pending: Option<BulkPlan>,
}

impl BulkFlow {
    /// Opens the confirmation. False for a plan that changes nothing.
    pub fn request(&mut self, plan: BulkPlan) -> bool {
        if plan.jobs.is_empty() {
            return false;
        }
        self.pending = Some(plan);
        true
    }

    pub fn is_confirming(&self) -> bool {
        self.pending.is_some()
    }

    pub fn text(&self, lang: Lang) -> Option<String> {
        self.pending.as_ref().map(|p| p.confirm_text(lang))
    }

    /// The user confirmed: the plan to run, once.
    pub fn confirm(&mut self) -> Option<BulkPlan> {
        self.pending.take()
    }

    pub fn cancel(&mut self) {
        self.pending = None;
    }

    /// Rows went away while the confirmation was open: clips no longer listed are dropped from
    /// the plan, and the confirmation closes when none are left.
    pub fn retain(&mut self, ids: &[ClipId]) {
        if let Some(p) = &mut self.pending {
            p.jobs.retain(|j| ids.contains(&j.id));
            if p.jobs.is_empty() {
                self.pending = None;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::popup::list::Item;
    use darkclip_core::{ClipType, Timestamp};

    const T: i64 = 1_795_320_000_000;

    fn target(n: i64, masked: bool, favorite: bool, tags: &[&str]) -> Target {
        Target {
            id: ClipId::new_v7(Timestamp(T + n)),
            masked,
            favorite,
            tags: tags.iter().map(|s| s.to_string()).collect(),
        }
    }

    fn col() -> (CollectionId, String) {
        (CollectionId::new_v7(Timestamp(T)), "Work".to_string())
    }

    fn add_col() -> BulkOp {
        let (id, name) = col();
        BulkOp::AddToCollection { id, name }
    }

    fn item(masked: bool, favorite: bool) -> Item {
        Item {
            id: ClipId::new_v7(Timestamp(T)),
            kind: ClipType::Text,
            preview: "p".into(),
            pinned: false,
            masked,
            source: None,
            at: Timestamp(T),
            favorite,
            tags: Vec::new(),
        }
    }

    #[test]
    fn the_popup_favorite_toggle_flips_the_flag() {
        assert_eq!(favorite_toggle(&item(false, false)), Some(true));
        assert_eq!(favorite_toggle(&item(false, true)), Some(false));
    }

    #[test]
    fn a_masked_clip_can_be_unfavorited_but_never_favorited() {
        assert_eq!(favorite_toggle(&item(true, false)), None);
        assert_eq!(favorite_toggle(&item(true, true)), Some(false));
    }

    #[test]
    fn favorite_target_unfavorites_only_when_every_clip_is_a_favorite() {
        let all = [target(1, false, true, &[]), target(2, false, true, &[])];
        assert_eq!(favorite_target(&all), Some(false));
        let mixed = [target(1, false, true, &[]), target(2, false, false, &[])];
        assert_eq!(favorite_target(&mixed), Some(true));
        assert_eq!(favorite_target(&[]), None);
        // Only masked non-favorites: nothing can change.
        assert_eq!(favorite_target(&[target(1, true, false, &[])]), None);
        // An unmasked favorite next to a masked non-favorite: the eligible clip is unfavorited.
        let ts = [target(1, false, true, &[]), target(2, true, false, &[])];
        assert_eq!(favorite_target(&ts), Some(false));
        let p = BulkPlan::new(BulkOp::Favorite(false), &ts).unwrap();
        assert_eq!(p.count(), 1);
    }

    #[test]
    fn only_remove_from_collection_has_an_inexact_count() {
        let (id, name) = col();
        let ts = [target(1, false, false, &[])];
        let rm = BulkPlan::new(BulkOp::RemoveFromCollection { id, name }, &ts).unwrap();
        assert!(!rm.count_is_exact());
        assert!(BulkPlan::new(add_col(), &ts).unwrap().count_is_exact());
        assert!(BulkPlan::new(BulkOp::Favorite(true), &ts)
            .unwrap()
            .count_is_exact());
    }

    #[test]
    fn favoriting_skips_masked_clips_and_clips_that_already_are() {
        let ts = [
            target(1, false, false, &[]),
            target(2, true, false, &[]),
            target(3, false, true, &[]),
            target(4, false, false, &[]),
        ];
        let p = BulkPlan::new(BulkOp::Favorite(true), &ts).unwrap();
        let ids: Vec<ClipId> = p.jobs().iter().map(|j| j.id).collect();
        assert_eq!(ids, vec![ts[0].id, ts[3].id]);
        assert_eq!(p.count(), 2);
        assert_eq!(p.skipped_masked(), 1);
    }

    #[test]
    fn unfavoriting_includes_masked_favorites() {
        let ts = [target(1, true, true, &[]), target(2, false, false, &[])];
        let p = BulkPlan::new(BulkOp::Favorite(false), &ts).unwrap();
        assert_eq!(p.count(), 1);
        assert_eq!(p.jobs()[0].id, ts[0].id);
    }

    #[test]
    fn add_to_collection_leaves_masked_clips_out_and_counts_them() {
        let ts = [
            target(1, false, false, &[]),
            target(2, true, false, &[]),
            target(3, true, true, &[]),
        ];
        let p = BulkPlan::new(add_col(), &ts).unwrap();
        assert_eq!(p.count(), 1);
        assert_eq!(p.skipped_masked(), 2);
        assert_eq!(
            p.skipped_text(Lang::En).as_deref(),
            Some("Masked clips skipped: 2")
        );
    }

    #[test]
    fn remove_from_collection_works_on_masked_clips() {
        let (id, name) = col();
        let ts = [target(1, true, false, &[]), target(2, false, false, &[])];
        let p = BulkPlan::new(BulkOp::RemoveFromCollection { id, name }, &ts).unwrap();
        assert_eq!(p.count(), 2);
        assert_eq!(p.skipped_masked(), 0);
    }

    #[test]
    fn only_masked_clips_means_no_plan_for_an_add() {
        let ts = [target(1, true, false, &[]), target(2, true, false, &[])];
        assert_eq!(BulkPlan::new(add_col(), &ts), None);
        assert_eq!(BulkPlan::new(BulkOp::AddTag("x".into()), &ts), None);
        assert_eq!(BulkPlan::new(BulkOp::Favorite(true), &ts), None);
    }

    #[test]
    fn add_tag_sends_each_clips_complete_list_and_skips_clips_that_have_it() {
        let ts = [
            target(1, false, false, &["b"]),
            target(2, false, false, &["new", "z"]),
            target(3, false, false, &[]),
        ];
        let p = BulkPlan::new(BulkOp::AddTag("new".into()), &ts).unwrap();
        assert_eq!(p.count(), 2);
        assert_eq!(p.jobs()[0].tags, vec!["b".to_string(), "new".to_string()]);
        assert_eq!(p.jobs()[1].tags, vec!["new".to_string()]);
    }

    #[test]
    fn add_tag_skips_clips_that_already_have_20_tags() {
        let twenty: Vec<String> = (0..20).map(|i| format!("t{i:02}")).collect();
        let full = Target {
            tags: twenty,
            ..target(1, false, false, &[])
        };
        let ts = [full, target(2, false, false, &[])];
        let p = BulkPlan::new(BulkOp::AddTag("extra".into()), &ts).unwrap();
        assert_eq!(p.count(), 1);
        assert_eq!(p.skipped_full(), 1);
        assert_eq!(
            p.skipped_text(Lang::En).as_deref(),
            Some("Clips already at 20 tags, skipped: 1")
        );
    }

    #[test]
    fn an_invalid_tag_makes_no_plan() {
        let ts = [target(1, false, false, &[])];
        assert_eq!(BulkPlan::new(BulkOp::AddTag("not valid".into()), &ts), None);
    }

    #[test]
    fn several_clips_need_a_confirmation_one_does_not() {
        let one = BulkPlan::new(BulkOp::Favorite(true), &[target(1, false, false, &[])]).unwrap();
        assert!(!one.needs_confirmation());
        let two = BulkPlan::new(
            BulkOp::Favorite(true),
            &[target(1, false, false, &[]), target(2, false, false, &[])],
        )
        .unwrap();
        assert!(two.needs_confirmation());
    }

    #[test]
    fn the_confirmation_shows_the_count_of_clips_that_change() {
        let (id, name) = col();
        let ts = [
            target(1, false, false, &[]),
            target(2, false, false, &[]),
            target(3, true, false, &[]),
        ];
        let p = BulkPlan::new(add_col(), &ts).unwrap();
        assert_eq!(p.confirm_text(Lang::En), "Add 2 clips to \"Work\"?");
        assert!(p.confirm_text(Lang::Vi).contains("2"));
        let fav = BulkPlan::new(BulkOp::Favorite(true), &ts).unwrap();
        assert_eq!(fav.confirm_text(Lang::En), "Add 2 clips to Favorites?");
        let unfav = BulkPlan::new(
            BulkOp::Favorite(false),
            &[target(1, false, true, &[]), target(2, false, true, &[])],
        )
        .unwrap();
        assert_eq!(
            unfav.confirm_text(Lang::En),
            "Remove 2 clips from Favorites?"
        );
        let rm = BulkPlan::new(BulkOp::RemoveFromCollection { id, name }, &ts).unwrap();
        assert_eq!(
            rm.confirm_text(Lang::En),
            "Remove the 3 marked clips from \"Work\"? Clips not in it are unchanged."
        );
        let tag = BulkPlan::new(BulkOp::AddTag("x".into()), &ts).unwrap();
        assert_eq!(tag.confirm_text(Lang::En), "Add the tag \"x\" to 2 clips?");
    }

    #[test]
    fn the_flow_confirms_once_and_cancel_drops_the_plan() {
        let ts = [target(1, false, false, &[]), target(2, false, false, &[])];
        let plan = BulkPlan::new(BulkOp::Favorite(true), &ts).unwrap();
        let mut f = BulkFlow::default();
        assert!(!f.is_confirming());
        assert_eq!(f.confirm(), None);
        assert!(f.request(plan.clone()));
        assert!(f.is_confirming());
        assert_eq!(
            f.text(Lang::En).as_deref(),
            Some("Add 2 clips to Favorites?")
        );
        assert_eq!(f.confirm(), Some(plan.clone()));
        assert_eq!(f.confirm(), None);
        assert!(f.request(plan));
        f.cancel();
        assert!(!f.is_confirming());
        assert_eq!(f.confirm(), None);
    }

    #[test]
    fn rows_that_leave_the_list_leave_the_plan() {
        let ts = [
            target(1, false, false, &[]),
            target(2, false, false, &[]),
            target(3, false, false, &[]),
        ];
        let mut f = BulkFlow::default();
        assert!(f.request(BulkPlan::new(BulkOp::Favorite(true), &ts).unwrap()));
        f.retain(&[ts[0].id, ts[2].id]);
        assert_eq!(
            f.text(Lang::En).as_deref(),
            Some("Add 2 clips to Favorites?")
        );
        f.retain(&[]);
        assert!(!f.is_confirming());
    }

    #[test]
    fn a_plan_that_changes_nothing_does_not_open_the_flow() {
        let mut f = BulkFlow::default();
        let empty = BulkPlan {
            op: BulkOp::Favorite(true),
            jobs: Vec::new(),
            skipped_masked: 0,
            skipped_full: 0,
        };
        assert!(!f.request(empty));
    }
}
