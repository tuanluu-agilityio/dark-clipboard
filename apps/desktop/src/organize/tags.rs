//! The tag editor (chips, add and remove) and tag validation (Task 403). Pure. Tag names are
//! normalised by `darkclip_core::Tag::validate_name` (trim, lowercase, NFC, letters, digits,
//! `_` and `-`, 40 characters); the editor never sends anything that the core would refuse.

use darkclip_core::i18n::Msg;

/// A tag name as the core would store it, or the message to show.
pub fn normalize(raw: &str) -> Result<String, Msg> {
    let _ = raw;
    Err(Msg::OrgTagInvalid)
}

/// `existing` plus `raw` (normalised), sorted without duplicates. `Ok(None)` when the clip has
/// the tag already; an error when the name is invalid or the clip has 20 tags.
pub fn merged(existing: &[String], raw: &str) -> Result<Option<Vec<String>>, Msg> {
    let _ = (existing, raw);
    Err(Msg::OrgTagInvalid)
}

/// The chips of one clip. Every change returns the complete new list, which is what
/// `update_clip_tags` takes.
#[derive(Clone, PartialEq, Eq)]
pub struct TagEditor {
    tags: Vec<String>,
}

/// Never prints the tag names.
impl std::fmt::Debug for TagEditor {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TagEditor")
            .field("count", &self.tags.len())
            .finish()
    }
}

impl TagEditor {
    pub fn new(tags: Vec<String>) -> Self {
        Self { tags }
    }

    pub fn tags(&self) -> &[String] {
        &self.tags
    }

    /// Adds a tag. `Ok(Some(list))`: send `list`; `Ok(None)`: already there; `Err`: show it.
    pub fn add(&mut self, raw: &str) -> Result<Option<Vec<String>>, Msg> {
        let _ = raw;
        Err(Msg::OrgTagInvalid)
    }

    /// Removes the chip named `name`; the new list, or `None` when there is no such chip.
    pub fn remove(&mut self, name: &str) -> Option<Vec<String>> {
        let _ = name;
        None
    }

    /// Backspace in an empty input removes the last chip.
    pub fn remove_last(&mut self) -> Option<Vec<String>> {
        None
    }

    /// The server's answer replaced the list (it is the truth).
    pub fn set(&mut self, tags: Vec<String>) {
        self.tags = tags;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(s: &[&str]) -> Vec<String> {
        s.iter().map(|x| x.to_string()).collect()
    }

    #[test]
    fn a_tag_is_normalised_like_the_core_does() {
        assert_eq!(normalize("  Work-Item_2 ").unwrap(), "work-item_2");
        assert_eq!(normalize("ĐÃ-xong").unwrap(), "đã-xong");
    }

    #[test]
    fn an_invalid_tag_gives_the_invalid_message() {
        for bad in ["", "   ", "two words", "a,b", "#tag", "a/b", "tag!"] {
            assert_eq!(normalize(bad), Err(Msg::OrgTagInvalid), "{bad:?}");
        }
    }

    #[test]
    fn a_tag_of_more_than_40_characters_gives_the_too_long_message() {
        assert_eq!(normalize(&"a".repeat(40)).unwrap(), "a".repeat(40));
        assert_eq!(normalize(&"a".repeat(41)), Err(Msg::OrgTagTooLong));
    }

    #[test]
    fn adding_returns_the_complete_sorted_list() {
        let mut e = TagEditor::new(v(&["beta"]));
        assert_eq!(e.add(" Alpha ").unwrap(), Some(v(&["alpha", "beta"])));
        assert_eq!(e.tags(), v(&["alpha", "beta"]).as_slice());
    }

    #[test]
    fn adding_a_tag_the_clip_has_changes_nothing() {
        let mut e = TagEditor::new(v(&["alpha"]));
        assert_eq!(e.add("ALPHA").unwrap(), None);
        assert_eq!(e.tags(), v(&["alpha"]).as_slice());
    }

    #[test]
    fn an_invalid_add_keeps_the_chips_and_reports_the_message() {
        let mut e = TagEditor::new(v(&["alpha"]));
        assert_eq!(e.add("not valid"), Err(Msg::OrgTagInvalid));
        assert_eq!(e.tags(), v(&["alpha"]).as_slice());
    }

    #[test]
    fn the_21st_tag_is_refused() {
        let twenty: Vec<String> = (0..20).map(|i| format!("t{i:02}")).collect();
        let mut e = TagEditor::new(twenty.clone());
        assert_eq!(e.add("extra"), Err(Msg::OrgTagsFull));
        assert_eq!(e.tags(), twenty.as_slice());
        // A tag it has already is still "nothing to do", not an error.
        assert_eq!(e.add("t03").unwrap(), None);
    }

    #[test]
    fn removing_a_chip_returns_the_rest() {
        let mut e = TagEditor::new(v(&["a", "b", "c"]));
        assert_eq!(e.remove("b"), Some(v(&["a", "c"])));
        assert_eq!(e.remove("b"), None, "already gone");
        assert_eq!(e.remove_last(), Some(v(&["a"])));
        assert_eq!(e.remove_last(), Some(Vec::new()));
        assert_eq!(e.remove_last(), None);
    }

    #[test]
    fn merged_is_the_bulk_add_rule() {
        assert_eq!(merged(&v(&["b"]), "A").unwrap(), Some(v(&["a", "b"])));
        assert_eq!(merged(&v(&["a"]), "a").unwrap(), None);
        assert_eq!(merged(&v(&["a"]), "bad tag"), Err(Msg::OrgTagInvalid));
    }
}
