//! The history window's sidebar and toolbar date menu (Task 703). Pure: no window, no I/O.
//!
//! The sidebar regroups what the window already offered: `chips::history_filters` (All, Pinned,
//! kinds, apps) and the organisation entries (`nav::history_nav_chips`: Favorites, collections,
//! tags). Nothing about filtering changes: `Filters`, `chips::toggle` and `nav::toggle_nav` stay
//! the only places that change filters, and `toggle` below only picks between the two. Group
//! collapse is UI state (a set the host keeps while the window is open), never part of `Filters`.
//!
//! Counts exist only where the data already exists (D-080): Favorites, user collections and tags.
//! Names of collections, tags and apps are user data: shown, never logged (GC-06).

use crate::organize::nav::{self, Nav, NAV_COLLECTION, NAV_FAVORITES, NAV_TAG};
use crate::popup::chips::{
    self, CHIP_ALL, CHIP_MONTH, CHIP_PINNED, CHIP_SOURCE, CHIP_TODAY, CHIP_WEEK, KIND_CHIPS,
};
use crate::popup::list::{Filters, TimeFilter};
use crate::popup::row::{kind_icon_name, KindIcon};
use darkclip_core::i18n::{t, Lang, Msg};
use std::collections::BTreeSet;

/// A sidebar group, in display order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Group {
    Library,
    Types,
    Collections,
    Tags,
    Apps,
}

impl Group {
    pub const ALL: [Group; 5] = [
        Group::Library,
        Group::Types,
        Group::Collections,
        Group::Tags,
        Group::Apps,
    ];

    /// The group's number in `FilterItem.group`.
    pub fn index(self) -> i32 {
        match self {
            Group::Library => 0,
            Group::Types => 1,
            Group::Collections => 2,
            Group::Tags => 3,
            Group::Apps => 4,
        }
    }

    pub fn from_index(i: i32) -> Option<Group> {
        Group::ALL.into_iter().find(|g| g.index() == i)
    }

    /// The id of the group's header row (negative, so it never meets a filter id).
    pub fn header_id(self) -> i32 {
        -1 - self.index()
    }

    pub fn title(self) -> Msg {
        match self {
            Group::Library => Msg::UiGroupLibrary,
            Group::Types => Msg::UiGroupType,
            Group::Collections => Msg::OrgGroupCollections,
            Group::Tags => Msg::OrgGroupTags,
            Group::Apps => Msg::UiFilterApps,
        }
    }
}

/// The group a header row id belongs to.
pub fn group_of_header(id: i32) -> Option<Group> {
    Group::from_index(-1 - id)
}

/// Groups that start collapsed (Apps can be long).
pub fn default_collapsed() -> BTreeSet<Group> {
    BTreeSet::from([Group::Apps])
}

/// The icon of a sidebar row (the host maps it to a kit `IconName`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SidebarIcon {
    None,
    All,
    Pinned,
    Favorites,
    Kind(KindIcon),
    Collection,
    Tag,
    App,
}

/// One line of the sidebar: a group header or a filter.
#[derive(Clone, PartialEq, Eq)]
pub struct SidebarRow {
    /// The filter id (`chips` / `nav` ids) or, for a header, `Group::header_id`.
    pub id: i32,
    pub label: String,
    pub on: bool,
    pub header: bool,
    pub group: Group,
    /// A header whose group is collapsed (its rows are not in the list).
    pub collapsed: bool,
    /// Clips behind the filter, only where the data exists (D-080).
    pub count: Option<u64>,
    pub icon: SidebarIcon,
    /// A header with a `+` button (Collections).
    pub add: bool,
}

/// Never prints names (GC-06).
impl std::fmt::Debug for SidebarRow {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SidebarRow")
            .field("id", &self.id)
            .field("header", &self.header)
            .field("group", &self.group)
            .field("on", &self.on)
            .finish_non_exhaustive()
    }
}

fn header_row(g: Group, collapsed: &BTreeSet<Group>, lang: Lang) -> SidebarRow {
    SidebarRow {
        id: g.header_id(),
        label: t(lang, g.title()).to_string(),
        on: false,
        header: true,
        group: g,
        collapsed: collapsed.contains(&g),
        count: None,
        icon: SidebarIcon::None,
        add: g == Group::Collections,
    }
}

fn item_row(
    id: i32,
    label: &str,
    on: bool,
    group: Group,
    count: Option<u64>,
    icon: SidebarIcon,
) -> SidebarRow {
    SidebarRow {
        id,
        label: label.to_string(),
        on,
        header: false,
        group,
        collapsed: false,
        count,
        icon,
        add: false,
    }
}

/// The sidebar: Library (All, Pinned, Favorites), Types, Collections (always shown: its header
/// has the `+`), Tags and Apps (each only when it has entries). The rows of a collapsed group are
/// left out. One item is on per group exactly as `Filters` says.
pub fn sidebar_groups(
    f: &Filters,
    nav: &Nav,
    sources: &[String],
    collapsed: &BTreeSet<Group>,
    lang: Lang,
) -> Vec<SidebarRow> {
    let base = chips::history_filters(f, sources, lang);
    let org = nav::history_nav_chips(f, nav, lang);
    let on_of = |id: i32| org.iter().any(|c| c.id == id && c.on);
    let mut out = Vec::new();
    let mut push_group = |g: Group, items: Vec<SidebarRow>, show_empty: bool| {
        if items.is_empty() && !show_empty {
            return;
        }
        out.push(header_row(g, collapsed, lang));
        if !collapsed.contains(&g) {
            out.extend(items);
        }
    };

    let mut library = Vec::new();
    for c in base
        .iter()
        .filter(|c| c.id == CHIP_ALL || c.id == CHIP_PINNED)
    {
        let icon = if c.id == CHIP_ALL {
            SidebarIcon::All
        } else {
            SidebarIcon::Pinned
        };
        library.push(item_row(c.id, &c.label, c.on, Group::Library, None, icon));
    }
    if let Some(fav) = nav.favorites() {
        library.push(item_row(
            NAV_FAVORITES,
            t(lang, Msg::OrgFavorites),
            on_of(NAV_FAVORITES),
            Group::Library,
            Some(fav.clip_count),
            SidebarIcon::Favorites,
        ));
    }
    push_group(Group::Library, library, false);

    let types: Vec<SidebarRow> = base
        .iter()
        .filter(|c| !c.header && usize::try_from(c.id).is_ok_and(|i| i < KIND_CHIPS.len()))
        .map(|c| {
            let icon = usize::try_from(c.id)
                .ok()
                .and_then(|i| KIND_CHIPS.get(i))
                .map_or(SidebarIcon::None, |k| SidebarIcon::Kind(kind_icon_name(*k)));
            item_row(c.id, &c.label, c.on, Group::Types, None, icon)
        })
        .collect();
    push_group(Group::Types, types, false);

    let collections: Vec<SidebarRow> = nav
        .user_collections()
        .iter()
        .enumerate()
        .map(|(i, c)| {
            let id = NAV_COLLECTION + i32::try_from(i).unwrap_or(0);
            item_row(
                id,
                &c.name,
                on_of(id),
                Group::Collections,
                Some(c.clip_count),
                SidebarIcon::Collection,
            )
        })
        .collect();
    push_group(Group::Collections, collections, true);

    let tags: Vec<SidebarRow> = nav
        .tags
        .iter()
        .enumerate()
        .map(|(i, tag)| {
            let id = NAV_TAG + i32::try_from(i).unwrap_or(0);
            item_row(
                id,
                &tag.name,
                on_of(id),
                Group::Tags,
                Some(tag.clip_count),
                SidebarIcon::Tag,
            )
        })
        .collect();
    push_group(Group::Tags, tags, false);

    let apps: Vec<SidebarRow> = base
        .iter()
        .filter(|c| !c.header && c.id >= CHIP_SOURCE)
        .map(|c| item_row(c.id, &c.label, c.on, Group::Apps, None, SidebarIcon::App))
        .collect();
    push_group(Group::Apps, apps, false);
    out
}

/// The filters after a click on sidebar item `id`: `nav::toggle_nav` for the organisation ids,
/// `chips::toggle` for the rest. `None` for a header, a date id or an id that means nothing.
pub fn toggle(f: &Filters, id: i32, nav: &Nav, sources: &[String]) -> Option<Filters> {
    if group_of_header(id).is_some() || DATE_CHIPS.contains(&id) {
        return None;
    }
    if nav::is_nav(id) {
        nav::toggle_nav(f, id, nav)
    } else {
        chips::toggle(f, id, sources)
    }
}

// ---- The toolbar's date menu ----------------------------------------------------------------

/// The date chips of the old sidebar (same ids, same `chips::toggle`).
pub const DATE_CHIPS: [i32; 3] = [CHIP_TODAY, CHIP_WEEK, CHIP_MONTH];

/// The date menu's items, in order: Any time, Today, Last 7 days, Last 30 days.
pub const DATE_ITEMS: [Msg; 4] = [
    Msg::UiFilterAnyTime,
    Msg::UiFilterToday,
    Msg::UiFilterLast7Days,
    Msg::UiFilterLast30Days,
];

/// Which date item is active (0 is "Any time").
pub fn date_active(f: &Filters) -> usize {
    match f.time {
        TimeFilter::Any => 0,
        TimeFilter::Today => 1,
        TimeFilter::Week => 2,
        TimeFilter::Month => 3,
    }
}

/// The label the toolbar button shows: the active item's name.
pub fn date_label(f: &Filters, lang: Lang) -> &'static str {
    t(lang, DATE_ITEMS[date_active(f).min(DATE_ITEMS.len() - 1)])
}

/// The chip to toggle for date item `i`: the item's own chip, or for "Any time" the active chip
/// (which clears it). `None` when nothing has to change.
pub fn date_chip(i: usize, f: &Filters) -> Option<i32> {
    match i {
        0 => date_active(f)
            .checked_sub(1)
            .and_then(|a| DATE_CHIPS.get(a))
            .copied(),
        _ => DATE_CHIPS.get(i - 1).copied(),
    }
}

/// The filters after choosing date item `i`.
pub fn choose_date(f: &Filters, i: usize) -> Option<Filters> {
    chips::toggle(f, date_chip(i, f)?, &[])
}

/// The window title bar text shown in the toolbar: the current filter's name, or "All".
pub fn title_of(rows: &[SidebarRow], f: &Filters, lang: Lang) -> String {
    // The most specific filter wins: a tag, a collection, an app, a type, Pinned, else All.
    let pick = |g: Group| rows.iter().find(|r| !r.header && r.on && r.group == g);
    for g in [
        Group::Tags,
        Group::Collections,
        Group::Apps,
        Group::Types,
        Group::Library,
    ] {
        if let Some(r) = pick(g) {
            if !(g == Group::Library && r.id == CHIP_ALL && !chips::is_all(f)) {
                return r.label.clone();
            }
        }
    }
    t(lang, Msg::UiFilterAll).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use darkclip_app::api::{CollectionView, TagCountView};
    use darkclip_core::{ClipType, CollectionId, TagId, Timestamp};

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

    fn srcs() -> Vec<String> {
        vec!["Code".to_string(), "Edge".to_string()]
    }

    fn none() -> BTreeSet<Group> {
        BTreeSet::new()
    }

    fn labels(rows: &[SidebarRow]) -> Vec<(String, bool)> {
        rows.iter().map(|r| (r.label.clone(), r.header)).collect()
    }

    #[test]
    fn groups_come_in_order_with_their_members() {
        let rows = sidebar_groups(&Filters::default(), &nav(), &srcs(), &none(), Lang::En);
        let got: Vec<&str> = rows.iter().map(|r| r.label.as_str()).collect();
        assert_eq!(
            got,
            [
                "Library",
                "All",
                "Pinned",
                "Favorites",
                "Type",
                "Text",
                "Rich text",
                "Link",
                "Code",
                "Image",
                "Files",
                "Color",
                "Collections",
                "Work",
                "Ideas",
                "Tags",
                "todo",
                "urgent",
                "Apps",
                "Code",
                "Edge",
            ]
        );
        let groups: Vec<Group> = rows.iter().filter(|r| r.header).map(|r| r.group).collect();
        assert_eq!(groups, Group::ALL);
    }

    #[test]
    fn empty_state_has_library_types_and_a_collections_header_with_a_plus() {
        let rows = sidebar_groups(&Filters::default(), &Nav::default(), &[], &none(), Lang::En);
        let heads: Vec<(Group, bool)> = rows
            .iter()
            .filter(|r| r.header)
            .map(|r| (r.group, r.add))
            .collect();
        assert_eq!(
            heads,
            [
                (Group::Library, false),
                (Group::Types, false),
                (Group::Collections, true)
            ]
        );
        // No Favorites row before the collections are loaded.
        assert!(rows.iter().all(|r| r.id != NAV_FAVORITES));
        assert!(rows.iter().any(|r| r.id == CHIP_ALL && r.on));
    }

    #[test]
    fn counts_exist_only_where_the_data_does() {
        let rows = sidebar_groups(&Filters::default(), &nav(), &srcs(), &none(), Lang::En);
        for r in &rows {
            let expect = match (r.header, r.group, r.id) {
                (true, _, _) => None,
                (_, Group::Library, NAV_FAVORITES) => Some(4),
                (_, Group::Collections, id) if id == NAV_COLLECTION => Some(3),
                (_, Group::Collections, id) if id == NAV_COLLECTION + 1 => Some(0),
                (_, Group::Tags, id) if id == NAV_TAG => Some(2),
                (_, Group::Tags, id) if id == NAV_TAG + 1 => Some(1),
                _ => None,
            };
            assert_eq!(r.count, expect, "{} ({:?})", r.label, r.group);
        }
    }

    #[test]
    fn apps_start_collapsed_and_collapse_is_not_in_the_filters() {
        let collapsed = default_collapsed();
        assert_eq!(collapsed, BTreeSet::from([Group::Apps]));
        let rows = sidebar_groups(&Filters::default(), &nav(), &srcs(), &collapsed, Lang::En);
        let apps: Vec<&SidebarRow> = rows.iter().filter(|r| r.group == Group::Apps).collect();
        assert_eq!(apps.len(), 1, "only the header");
        assert!(apps[0].header && apps[0].collapsed);
        // Collapsing changes the rows, not the filters.
        let f = Filters::default();
        let _ = sidebar_groups(&f, &nav(), &srcs(), &collapsed, Lang::En);
        assert_eq!(f, Filters::default());
        // A collapsed group shows no members; an expanded one does.
        let some = BTreeSet::from([Group::Types, Group::Tags]);
        let rows = sidebar_groups(&Filters::default(), &nav(), &srcs(), &some, Lang::En);
        assert_eq!(rows.iter().filter(|r| r.group == Group::Types).count(), 1);
        assert_eq!(rows.iter().filter(|r| r.group == Group::Tags).count(), 1);
        assert!(rows.iter().filter(|r| r.group == Group::Library).count() > 1);
    }

    #[test]
    fn the_date_filters_are_not_in_the_sidebar() {
        let rows = sidebar_groups(&Filters::default(), &nav(), &srcs(), &none(), Lang::En);
        for r in &rows {
            assert!(!DATE_CHIPS.contains(&r.id), "{}", r.label);
            assert_ne!(r.label, "Today");
        }
    }

    #[test]
    fn rows_are_on_exactly_when_the_filters_say_so() {
        let n = nav();
        let f = Filters {
            collection: Some(n.collections[1].id),
            tag: Some("urgent".into()),
            kinds: vec![ClipType::Url],
            source: Some("Edge".into()),
            ..Filters::default()
        };
        let rows = sidebar_groups(&f, &n, &srcs(), &none(), Lang::En);
        let on: Vec<&str> = rows
            .iter()
            .filter(|r| r.on)
            .map(|r| r.label.as_str())
            .collect();
        assert_eq!(on, ["Link", "Work", "urgent", "Edge"]);
        let fav = Filters {
            collection: n.favorites_id(),
            ..Filters::default()
        };
        let rows = sidebar_groups(&fav, &n, &srcs(), &none(), Lang::En);
        assert!(rows.iter().any(|r| r.id == NAV_FAVORITES && r.on));
        assert!(rows.iter().all(|r| r.id != CHIP_ALL || !r.on));
    }

    /// Every id the sidebar can show gives the same filters as today's `toggle` / `toggle_nav`.
    #[test]
    fn toggling_through_the_sidebar_equals_the_old_paths_for_every_id() {
        let n = nav();
        let sources = srcs();
        let start = [
            Filters::default(),
            Filters {
                text: "abc".into(),
                pinned_only: true,
                kinds: vec![ClipType::Code],
                source: Some("Code".into()),
                collection: n.favorites_id(),
                tag: Some("todo".into()),
                time: TimeFilter::Week,
            },
        ];
        for f in &start {
            let rows = sidebar_groups(f, &n, &sources, &none(), Lang::En);
            let ids: Vec<i32> = rows.iter().filter(|r| !r.header).map(|r| r.id).collect();
            assert!(ids.len() > 15);
            for id in ids {
                let old = if nav::is_nav(id) {
                    nav::toggle_nav(f, id, &n)
                } else {
                    chips::toggle(f, id, &sources)
                };
                assert!(old.is_some(), "id {id} means nothing");
                assert_eq!(toggle(f, id, &n, &sources), old, "id {id}");
            }
        }
    }

    #[test]
    fn headers_and_date_ids_do_not_change_the_filters() {
        let f = Filters::default();
        for g in Group::ALL {
            assert_eq!(toggle(&f, g.header_id(), &nav(), &srcs()), None);
            assert_eq!(group_of_header(g.header_id()), Some(g));
        }
        for id in DATE_CHIPS {
            assert_eq!(toggle(&f, id, &nav(), &srcs()), None);
        }
        assert_eq!(group_of_header(CHIP_ALL), None);
        assert_eq!(group_of_header(NAV_TAG), None);
        // Unknown ids still mean nothing.
        assert_eq!(toggle(&f, CHIP_SOURCE + 9, &nav(), &srcs()), None);
        assert_eq!(toggle(&f, NAV_TAG + 9, &nav(), &srcs()), None);
    }

    #[test]
    fn one_item_per_group_as_today() {
        // A second tag replaces the first (the old `toggle_nav` rule); a second kind adds.
        let n = nav();
        let f = toggle(&Filters::default(), NAV_TAG, &n, &srcs()).unwrap();
        let f = toggle(&f, NAV_TAG + 1, &n, &srcs()).unwrap();
        assert_eq!(f.tag.as_deref(), Some("urgent"));
        let rows = sidebar_groups(&f, &n, &srcs(), &none(), Lang::En);
        assert_eq!(
            rows.iter()
                .filter(|r| r.on && r.group == Group::Tags)
                .count(),
            1
        );
    }

    #[test]
    fn labels_are_vietnamese_too_and_have_no_counts_in_them() {
        let rows = sidebar_groups(&Filters::default(), &nav(), &srcs(), &none(), Lang::Vi);
        let l = labels(&rows);
        assert!(l.contains(&("Thư viện".to_string(), true)));
        assert!(l.contains(&("Tất cả".to_string(), false)));
        assert!(l.contains(&("Yêu thích".to_string(), false)));
        assert!(rows.iter().all(|r| !r.label.contains('(')));
    }

    #[test]
    fn debug_never_prints_names() {
        let rows = sidebar_groups(&Filters::default(), &nav(), &srcs(), &none(), Lang::En);
        let s = format!("{rows:?}");
        for name in ["Work", "Ideas", "urgent", "Edge"] {
            assert!(!s.contains(name), "{name}");
        }
    }

    #[test]
    fn the_date_menu_has_four_items_and_the_same_toggle_as_the_old_chips() {
        assert_eq!(DATE_ITEMS.len(), 4);
        let f = Filters::default();
        assert_eq!(date_active(&f), 0);
        assert_eq!(date_label(&f, Lang::En), "Any time");
        // Choosing an item equals toggling its old chip.
        for (i, (chip, time)) in [
            (CHIP_TODAY, TimeFilter::Today),
            (CHIP_WEEK, TimeFilter::Week),
            (CHIP_MONTH, TimeFilter::Month),
        ]
        .into_iter()
        .enumerate()
        {
            let g = choose_date(&f, i + 1).unwrap();
            assert_eq!(g, chips::toggle(&f, chip, &[]).unwrap());
            assert_eq!(g.time, time);
            assert_eq!(date_active(&g), i + 1);
        }
        // "Any time" with nothing active changes nothing; with one active it clears it.
        assert_eq!(choose_date(&f, 0), None);
        let week = choose_date(&f, 2).unwrap();
        assert_eq!(date_label(&week, Lang::En), "Last 7 days");
        assert_eq!(choose_date(&week, 0).unwrap().time, TimeFilter::Any);
        // Picking the active item again clears it (the chip's toggle).
        assert_eq!(choose_date(&week, 2).unwrap().time, TimeFilter::Any);
        // Another filter and the search text stay.
        let busy = Filters {
            text: "x".into(),
            pinned_only: true,
            ..Filters::default()
        };
        let g = choose_date(&busy, 1).unwrap();
        assert!(g.pinned_only && g.text == "x" && g.time == TimeFilter::Today);
        assert_eq!(choose_date(&f, 9), None);
    }

    #[test]
    fn date_names_exist_in_both_languages() {
        for lang in [Lang::En, Lang::Vi] {
            for m in DATE_ITEMS {
                assert!(!t(lang, m).is_empty());
            }
        }
        assert_eq!(
            date_label(&choose_date(&Filters::default(), 3).unwrap(), Lang::Vi),
            "30 ngày qua"
        );
    }

    #[test]
    fn the_title_names_the_current_filter() {
        let n = nav();
        let rows = |f: &Filters| sidebar_groups(f, &n, &srcs(), &none(), Lang::En);
        let f = Filters::default();
        assert_eq!(title_of(&rows(&f), &f, Lang::En), "All");
        let f = toggle(&f, NAV_COLLECTION, &n, &srcs()).unwrap();
        assert_eq!(title_of(&rows(&f), &f, Lang::En), "Work");
        let f = toggle(&f, NAV_TAG, &n, &srcs()).unwrap();
        assert_eq!(title_of(&rows(&f), &f, Lang::En), "todo");
        let f = toggle(&Filters::default(), CHIP_PINNED, &n, &srcs()).unwrap();
        assert_eq!(title_of(&rows(&f), &f, Lang::En), "Pinned");
        // A date filter alone keeps the title "All" (the toolbar's date button shows it).
        let f = choose_date(&Filters::default(), 1).unwrap();
        assert_eq!(title_of(&rows(&f), &f, Lang::En), "All");
    }
}
