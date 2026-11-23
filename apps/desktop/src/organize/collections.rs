//! Collection names and the delete confirmation (Task 403). Pure.

use super::fill_name;
use darkclip_core::i18n::{t, Lang, Msg};
use darkclip_core::{CollectionId, COLLECTION_NAME_MAX_CHARS};

/// A name for a new or renamed collection: trimmed, 1-80 characters, and not used by another
/// collection (compared without case). `existing` lists every collection (id, name), the system
/// Favorites included; `except` is the collection being renamed. The error is the message to show.
pub fn validate_name(
    raw: &str,
    existing: &[(CollectionId, String)],
    except: Option<CollectionId>,
) -> Result<String, Msg> {
    let name = raw.trim();
    match name.chars().count() {
        0 => return Err(Msg::OrgNameEmpty),
        n if n > COLLECTION_NAME_MAX_CHARS => return Err(Msg::OrgNameTooLong),
        _ => {}
    }
    let lower = name.to_lowercase();
    let taken = existing
        .iter()
        .any(|(id, n)| Some(*id) != except && n.trim().to_lowercase() == lower);
    if taken {
        return Err(Msg::OrgNameExists);
    }
    Ok(name.to_string())
}

/// The confirmation before a collection is deleted. The clips are kept; only the collection and
/// its memberships go.
#[derive(Clone, Default, PartialEq, Eq)]
pub struct CollectionDelete {
    pending: Option<(CollectionId, String, u64)>,
}

/// Never prints the collection name.
impl std::fmt::Debug for CollectionDelete {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CollectionDelete")
            .field("open", &self.pending.is_some())
            .finish()
    }
}

impl CollectionDelete {
    /// Opens the confirmation. False (nothing opens) for the system collection.
    pub fn request(&mut self, id: CollectionId, name: &str, clips: u64, system: bool) -> bool {
        if system {
            return false;
        }
        self.pending = Some((id, name.to_string(), clips));
        true
    }

    pub fn is_confirming(&self) -> bool {
        self.pending.is_some()
    }

    /// The question to show: names the collection and says its clips stay.
    pub fn text(&self, lang: Lang) -> Option<String> {
        let (_, name, clips) = self.pending.as_ref()?;
        Some(fill_name(
            t(lang, Msg::OrgConfirmDeleteCollection),
            name,
            *clips,
        ))
    }

    /// The user confirmed: the collection to delete, once.
    pub fn confirm(&mut self) -> Option<CollectionId> {
        self.pending.take().map(|(id, _, _)| id)
    }

    pub fn cancel(&mut self) {
        self.pending = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The same id for the same `n` (a v7 id has random bits, so `new_v7` would differ).
    fn id(n: i64) -> CollectionId {
        CollectionId::parse(&format!("01a20169-2e01-7000-8000-{n:012}")).unwrap()
    }

    fn existing() -> Vec<(CollectionId, String)> {
        vec![
            (id(0), "Favorites".to_string()),
            (id(1), "Work".to_string()),
            (id(2), "Ideas".to_string()),
        ]
    }

    #[test]
    fn a_name_is_trimmed() {
        assert_eq!(
            validate_name("  Recipes  ", &existing(), None).unwrap(),
            "Recipes"
        );
    }

    #[test]
    fn an_empty_or_blank_name_is_refused() {
        assert_eq!(validate_name("", &existing(), None), Err(Msg::OrgNameEmpty));
        assert_eq!(
            validate_name("  \t ", &existing(), None),
            Err(Msg::OrgNameEmpty)
        );
    }

    #[test]
    fn the_limit_is_80_characters_counted_as_characters() {
        let ok = "đ".repeat(80);
        assert_eq!(validate_name(&ok, &existing(), None).unwrap(), ok);
        let long = "đ".repeat(81);
        assert_eq!(
            validate_name(&long, &existing(), None),
            Err(Msg::OrgNameTooLong)
        );
    }

    #[test]
    fn a_name_in_use_is_refused_without_regard_to_case() {
        assert_eq!(
            validate_name("work", &existing(), None),
            Err(Msg::OrgNameExists)
        );
        assert_eq!(
            validate_name(" IDEAS ", &existing(), None),
            Err(Msg::OrgNameExists)
        );
        // The system collection's name is taken too.
        assert_eq!(
            validate_name("favorites", &existing(), None),
            Err(Msg::OrgNameExists)
        );
    }

    #[test]
    fn renaming_to_its_own_name_or_its_case_is_allowed() {
        assert_eq!(
            validate_name("Work", &existing(), Some(id(1))).unwrap(),
            "Work"
        );
        assert_eq!(
            validate_name("WORK", &existing(), Some(id(1))).unwrap(),
            "WORK"
        );
        // But not to another collection's name.
        assert_eq!(
            validate_name("Ideas", &existing(), Some(id(1))),
            Err(Msg::OrgNameExists)
        );
    }

    #[test]
    fn delete_asks_first_and_names_the_collection_and_that_clips_stay() {
        let mut d = CollectionDelete::default();
        assert!(!d.is_confirming());
        assert_eq!(d.confirm(), None, "nothing to confirm before a request");
        assert!(d.request(id(1), "Work", 3, false));
        assert!(d.is_confirming());
        let en = d.text(Lang::En).unwrap();
        assert_eq!(
            en,
            "Delete the collection \"Work\"? Its 3 clips stay in your history."
        );
        assert!(d.text(Lang::Vi).unwrap().contains("Work"));
        assert_eq!(d.confirm(), Some(id(1)));
        // Once only.
        assert_eq!(d.confirm(), None);
        assert!(!d.is_confirming());
    }

    #[test]
    fn cancel_drops_the_request_and_the_system_collection_cannot_be_deleted() {
        let mut d = CollectionDelete::default();
        assert!(d.request(id(1), "Work", 0, false));
        d.cancel();
        assert_eq!(d.confirm(), None);
        assert!(!d.request(id(0), "Favorites", 5, true));
        assert!(!d.is_confirming());
    }
}
