//! The collections and tags in the filter lists (Task 403): which chips exist and what a click
//! does to the filters. Pure. The history window shows Favorites, the user collections and the
//! tags with their counts in the left pane; the popup shows Favorites and the collections as
//! chips. Selecting one filters the list; selecting it again clears it. One collection at a time
//! (Favorites is a collection), and one tag; a collection and a tag combine.

use crate::popup::chips::ChipSpec;
use crate::popup::list::Filters;
use darkclip_app::api::{CollectionView, TagCountView};
use darkclip_core::i18n::Lang;
use darkclip_core::CollectionId;

/// The Favorites chip.
pub const NAV_FAVORITES: i32 = 105;
/// Collection chips are `NAV_COLLECTION + index` into the user collections.
pub const NAV_COLLECTION: i32 = 1000;
/// Tag chips are `NAV_TAG + index` into the tags.
pub const NAV_TAG: i32 = 2000;
/// Collections shown as popup chips at most.
pub const POPUP_MAX_COLLECTIONS: usize = 6;

/// What the list pane knows about the organisation: loaded on the worker, never from clip content.
#[derive(Clone, Default, PartialEq, Eq)]
pub struct Nav {
    pub collections: Vec<CollectionView>,
    pub tags: Vec<TagCountView>,
}

/// Never prints names.
impl std::fmt::Debug for Nav {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Nav")
            .field("collections", &self.collections.len())
            .field("tags", &self.tags.len())
            .finish()
    }
}

impl Nav {
    /// The system Favorites collection.
    pub fn favorites(&self) -> Option<&CollectionView> {
        self.collections.iter().find(|c| c.system)
    }

    pub fn favorites_id(&self) -> Option<CollectionId> {
        self.favorites().map(|c| c.id)
    }

    /// The collections the user made, in list order.
    pub fn user_collections(&self) -> Vec<&CollectionView> {
        self.collections.iter().filter(|c| !c.system).collect()
    }

    /// (id, name) of every collection, for the name check.
    pub fn names(&self) -> Vec<(CollectionId, String)> {
        self.collections
            .iter()
            .map(|c| (c.id, c.name.clone()))
            .collect()
    }

    /// The collection the list is filtered to, when it is a user collection.
    pub fn shown_user_collection(&self, f: &Filters) -> Option<&CollectionView> {
        let id = f.collection?;
        self.collections.iter().find(|c| c.id == id && !c.system)
    }
}

/// True for an id this module owns.
pub fn is_nav(id: i32) -> bool {
    id == NAV_FAVORITES || id >= NAV_COLLECTION
}

/// The history window's chips for the organisation: Favorites, a "Collections" heading with the
/// user collections ("Name (3)"), a "Tags" heading with the tags ("name (2)"). Headings appear
/// only when there is something under them.
pub fn history_nav_chips(f: &Filters, nav: &Nav, lang: Lang) -> Vec<ChipSpec> {
    let _ = (f, nav, lang);
    Vec::new()
}

/// The popup's chips: Favorites, then at most `POPUP_MAX_COLLECTIONS` user collections.
pub fn popup_nav_chips(f: &Filters, nav: &Nav, lang: Lang) -> Vec<ChipSpec> {
    let _ = (f, nav, lang);
    Vec::new()
}

/// The filters after a click on nav chip `id` (the search text and other filters are kept), or
/// `None` for an id that means nothing (the lists changed since).
pub fn toggle_nav(f: &Filters, id: i32, nav: &Nav) -> Option<Filters> {
    let _ = (f, id, nav);
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use darkclip_core::i18n::Lang;
    use darkclip_core::{TagId, Timestamp};

    const T: i64 = 1_795_320_000_000;

    fn col(n: i64, name: &str, system: bool, clips: u64) -> CollectionView {
        CollectionView {
            id: CollectionId::new_v7(Timestamp(T + n)),
            name: name.to_string(),
            system,
            clip_count: clips,
        }
    }

    fn tag(n: i64, name: &str, clips: u64) -> TagCountView {
        TagCountView {
            id: TagId::new_v7(Timestamp(T + n)),
            name: name.to_string(),
            clip_count: clips,
        }
    }

    fn nav() -> Nav {
        Nav {
            collections: vec![
                col(0, "Favorites", true, 4),
                col(1, "Work", false, 3),
                col(2, "Ideas", false, 0),
            ],
            tags: vec![tag(10, "todo", 2), tag(11, "urgent", 1)],
        }
    }

    #[test]
    fn the_history_pane_lists_favorites_collections_and_tags_with_counts() {
        let v = history_nav_chips(&Filters::default(), &nav(), Lang::En);
        let labels: Vec<(&str, bool)> = v.iter().map(|c| (c.label.as_str(), c.header)).collect();
        assert_eq!(
            labels,
            [
                ("Favorites (4)", false),
                ("Collections", true),
                ("Work (3)", false),
                ("Ideas (0)", false),
                ("Tags", true),
                ("todo (2)", false),
                ("urgent (1)", false),
            ]
        );
        let ids: Vec<i32> = v.iter().filter(|c| !c.header).map(|c| c.id).collect();
        assert_eq!(
            ids,
            [
                NAV_FAVORITES,
                NAV_COLLECTION,
                NAV_COLLECTION + 1,
                NAV_TAG,
                NAV_TAG + 1
            ]
        );
        assert!(v.iter().all(|c| !c.on));
    }

    #[test]
    fn headings_appear_only_with_something_under_them() {
        let only_system = Nav {
            collections: vec![col(0, "Favorites", true, 0)],
            tags: Vec::new(),
        };
        let v = history_nav_chips(&Filters::default(), &only_system, Lang::En);
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].id, NAV_FAVORITES);
        assert!(history_nav_chips(&Filters::default(), &Nav::default(), Lang::En).is_empty());
    }

    #[test]
    fn the_shown_collection_and_tag_are_on() {
        let n = nav();
        let f = Filters {
            collection: Some(n.collections[1].id),
            tag: Some("urgent".into()),
            ..Filters::default()
        };
        let v = history_nav_chips(&f, &n, Lang::En);
        let on: Vec<&str> = v
            .iter()
            .filter(|c| c.on)
            .map(|c| c.label.as_str())
            .collect();
        assert_eq!(on, ["Work (3)", "urgent (1)"]);
        let fav = Filters {
            collection: n.favorites_id(),
            ..Filters::default()
        };
        assert!(history_nav_chips(&fav, &n, Lang::En)[0].on);
    }

    #[test]
    fn the_labels_are_vietnamese_too() {
        let v = history_nav_chips(&Filters::default(), &nav(), Lang::Vi);
        assert_eq!(v[0].label, "Yêu thích (4)");
        assert_eq!(v[1].label, "Bộ sưu tập");
    }

    #[test]
    fn the_popup_has_favorites_and_the_collections_without_counts() {
        let v = popup_nav_chips(&Filters::default(), &nav(), Lang::En);
        let labels: Vec<&str> = v.iter().map(|c| c.label.as_str()).collect();
        assert_eq!(labels, ["Favorites", "Work", "Ideas"]);
        assert!(v.iter().all(|c| !c.header && !c.on));
        assert_eq!(v[0].id, NAV_FAVORITES);
        assert_eq!(v[1].id, NAV_COLLECTION);
    }

    #[test]
    fn the_popup_shows_at_most_six_collections() {
        let mut n = nav();
        for i in 0..10 {
            n.collections.push(col(20 + i, &format!("c{i}"), false, 1));
        }
        let v = popup_nav_chips(&Filters::default(), &n, Lang::En);
        assert_eq!(v.len(), 1 + POPUP_MAX_COLLECTIONS);
    }

    #[test]
    fn clicking_favorites_filters_to_the_system_collection_and_again_clears_it() {
        let n = nav();
        let f = toggle_nav(&Filters::default(), NAV_FAVORITES, &n).unwrap();
        assert_eq!(f.collection, n.favorites_id());
        let f = toggle_nav(&f, NAV_FAVORITES, &n).unwrap();
        assert_eq!(f.collection, None);
    }

    #[test]
    fn one_collection_at_a_time_and_the_other_filters_stay() {
        let n = nav();
        let start = Filters {
            text: "abc".into(),
            pinned_only: true,
            ..Filters::default()
        };
        let f = toggle_nav(&start, NAV_COLLECTION, &n).unwrap();
        assert_eq!(f.collection, Some(n.collections[1].id));
        assert_eq!(f.text, "abc");
        assert!(f.pinned_only);
        let f = toggle_nav(&f, NAV_COLLECTION + 1, &n).unwrap();
        assert_eq!(f.collection, Some(n.collections[2].id));
        let f = toggle_nav(&f, NAV_FAVORITES, &n).unwrap();
        assert_eq!(f.collection, n.favorites_id());
    }

    #[test]
    fn a_tag_filters_and_combines_with_a_collection() {
        let n = nav();
        let f = toggle_nav(&Filters::default(), NAV_COLLECTION, &n).unwrap();
        let f = toggle_nav(&f, NAV_TAG + 1, &n).unwrap();
        assert_eq!(f.tag.as_deref(), Some("urgent"));
        assert_eq!(f.collection, Some(n.collections[1].id));
        let f = toggle_nav(&f, NAV_TAG, &n).unwrap();
        assert_eq!(f.tag.as_deref(), Some("todo"));
        let f = toggle_nav(&f, NAV_TAG, &n).unwrap();
        assert_eq!(f.tag, None);
    }

    #[test]
    fn ids_that_mean_nothing_do_nothing() {
        let n = nav();
        assert_eq!(
            toggle_nav(&Filters::default(), NAV_COLLECTION + 5, &n),
            None
        );
        assert_eq!(toggle_nav(&Filters::default(), NAV_TAG + 9, &n), None);
        assert_eq!(toggle_nav(&Filters::default(), 7, &n), None);
        // Favorites does nothing before the collections are loaded.
        assert_eq!(
            toggle_nav(&Filters::default(), NAV_FAVORITES, &Nav::default()),
            None
        );
    }

    #[test]
    fn is_nav_owns_its_ids_only() {
        assert!(is_nav(NAV_FAVORITES));
        assert!(is_nav(NAV_COLLECTION + 3));
        assert!(is_nav(NAV_TAG));
        for other in [0, 6, 100, 104, 200, 212] {
            assert!(!is_nav(other), "{other}");
        }
    }

    #[test]
    fn the_shown_user_collection_is_found_for_rename_and_delete() {
        let n = nav();
        let f = Filters {
            collection: Some(n.collections[1].id),
            ..Filters::default()
        };
        assert_eq!(
            n.shown_user_collection(&f).map(|c| c.name.as_str()),
            Some("Work")
        );
        // Favorites cannot be renamed or deleted.
        let fav = Filters {
            collection: n.favorites_id(),
            ..Filters::default()
        };
        assert!(n.shown_user_collection(&fav).is_none());
        assert!(n.shown_user_collection(&Filters::default()).is_none());
    }

    #[test]
    fn debug_never_prints_names() {
        assert!(!format!("{:?}", nav()).contains("Work"));
    }
}
