//! Filter chips shared by the popup and the history window (Task 211): which chips exist, what
//! each one means and what a click does to the filters. Pure. The host mirrors the specs into
//! its Slint model. Source app names are shown, never logged (GC-06).

use super::list::{Filters, TimeFilter};
use super::row::kind_msg;
use darkclip_core::i18n::{t, Lang, Msg};
use darkclip_core::ClipType;

/// Kind chips, in order; the chip id is the index.
pub const KIND_CHIPS: [ClipType; 7] = [
    ClipType::Text,
    ClipType::RichText,
    ClipType::Url,
    ClipType::Code,
    ClipType::Image,
    ClipType::Files,
    ClipType::Color,
];
pub const CHIP_PINNED: i32 = 100;
pub const CHIP_TODAY: i32 = 101;
pub const CHIP_WEEK: i32 = 102;
pub const CHIP_MONTH: i32 = 103;
/// History window only: clears every filter but the search text.
pub const CHIP_ALL: i32 = 104;
/// Source chips are `CHIP_SOURCE + index` into the current source list.
pub const CHIP_SOURCE: i32 = 200;

/// One chip, or (in the history window) a group heading.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChipSpec {
    pub id: i32,
    pub label: String,
    pub on: bool,
    pub header: bool,
}

pub(crate) fn chip(id: i32, label: &str, on: bool) -> ChipSpec {
    ChipSpec {
        id,
        label: label.to_string(),
        on,
        header: false,
    }
}

pub(crate) fn header(label: &str) -> ChipSpec {
    ChipSpec {
        id: -1,
        label: label.to_string(),
        on: false,
        header: true,
    }
}

fn index_id(base: i32, i: usize) -> i32 {
    base + i32::try_from(i).unwrap_or(0)
}

fn kind_chips(f: &Filters, lang: Lang) -> Vec<ChipSpec> {
    KIND_CHIPS
        .iter()
        .enumerate()
        .map(|(i, k)| chip(index_id(0, i), t(lang, kind_msg(*k)), f.kinds.contains(k)))
        .collect()
}

fn source_chips(f: &Filters, sources: &[String]) -> Vec<ChipSpec> {
    sources
        .iter()
        .enumerate()
        .map(|(i, s)| chip(index_id(CHIP_SOURCE, i), s, f.source.as_ref() == Some(s)))
        .collect()
}

/// The popup's chips: kinds, Pinned, Today, 7 days, then the source apps.
pub fn popup_chips(f: &Filters, sources: &[String], lang: Lang) -> Vec<ChipSpec> {
    let mut v = kind_chips(f, lang);
    v.push(chip(
        CHIP_PINNED,
        t(lang, Msg::UiFilterPinned),
        f.pinned_only,
    ));
    v.push(chip(
        CHIP_TODAY,
        t(lang, Msg::UiFilterToday),
        f.time == TimeFilter::Today,
    ));
    v.push(chip(
        CHIP_WEEK,
        t(lang, Msg::UiFilter7Days),
        f.time == TimeFilter::Week,
    ));
    v.extend(source_chips(f, sources));
    v
}

/// True when no filter besides the search text is on.
pub fn is_all(f: &Filters) -> bool {
    f.kinds.is_empty()
        && !f.pinned_only
        && f.time == TimeFilter::Any
        && f.source.is_none()
        && f.collection.is_none()
        && f.tag.is_none()
}

/// The history window's left pane: All, Pinned, then the Type, Date and Source groups.
pub fn history_filters(f: &Filters, sources: &[String], lang: Lang) -> Vec<ChipSpec> {
    let mut v = vec![
        chip(CHIP_ALL, t(lang, Msg::UiFilterAll), is_all(f)),
        chip(CHIP_PINNED, t(lang, Msg::UiFilterPinned), f.pinned_only),
        header(t(lang, Msg::UiGroupType)),
    ];
    v.extend(kind_chips(f, lang));
    v.push(header(t(lang, Msg::UiGroupDate)));
    for (id, msg, time) in [
        (CHIP_TODAY, Msg::UiFilterToday, TimeFilter::Today),
        (CHIP_WEEK, Msg::UiFilter7Days, TimeFilter::Week),
        (CHIP_MONTH, Msg::UiFilter30Days, TimeFilter::Month),
    ] {
        v.push(chip(id, t(lang, msg), f.time == time));
    }
    if !sources.is_empty() {
        v.push(header(t(lang, Msg::UiGroupSource)));
        v.extend(source_chips(f, sources));
    }
    v
}

/// The filters after a click on chip `id` (the search text is kept), or `None` for an id that
/// means nothing (a source chip whose list has changed).
pub fn toggle(f: &Filters, id: i32, sources: &[String]) -> Option<Filters> {
    let mut f = f.clone();
    match id {
        CHIP_ALL => {
            f = Filters {
                text: f.text,
                ..Filters::default()
            };
        }
        CHIP_PINNED => f.pinned_only = !f.pinned_only,
        CHIP_TODAY | CHIP_WEEK | CHIP_MONTH => {
            let want = match id {
                CHIP_TODAY => TimeFilter::Today,
                CHIP_WEEK => TimeFilter::Week,
                _ => TimeFilter::Month,
            };
            f.time = if f.time == want {
                TimeFilter::Any
            } else {
                want
            };
        }
        id if id >= CHIP_SOURCE => {
            let i = usize::try_from(id - CHIP_SOURCE).ok()?;
            let src = sources.get(i)?.clone();
            f.source = if f.source.as_ref() == Some(&src) {
                None
            } else {
                Some(src)
            };
        }
        id => {
            let k = usize::try_from(id).ok().and_then(|i| KIND_CHIPS.get(i))?;
            if let Some(pos) = f.kinds.iter().position(|x| x == k) {
                f.kinds.remove(pos);
            } else {
                f.kinds.push(*k);
            }
        }
    }
    Some(f)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn srcs() -> Vec<String> {
        vec!["Code".to_string(), "Edge".to_string()]
    }

    #[test]
    fn the_popup_keeps_its_chip_order_and_ids() {
        let v = popup_chips(&Filters::default(), &srcs(), Lang::En);
        let ids: Vec<i32> = v.iter().map(|c| c.id).collect();
        assert_eq!(ids, [0, 1, 2, 3, 4, 5, 6, 100, 101, 102, 200, 201]);
        assert!(v.iter().all(|c| !c.on && !c.header));
    }

    #[test]
    fn the_history_pane_has_all_pinned_and_three_groups() {
        let v = history_filters(&Filters::default(), &srcs(), Lang::En);
        let heads: Vec<&str> = v
            .iter()
            .filter(|c| c.header)
            .map(|c| c.label.as_str())
            .collect();
        assert_eq!(heads, ["Type", "Date", "Source app"]);
        assert_eq!(v[0].id, CHIP_ALL);
        assert!(v[0].on, "All is on while no filter is");
        assert_eq!(v[1].id, CHIP_PINNED);
        // No sources: no Source heading.
        let none = history_filters(&Filters::default(), &[], Lang::En);
        assert_eq!(none.iter().filter(|c| c.header).count(), 2);
        // Vietnamese labels exist.
        assert_eq!(
            history_filters(&Filters::default(), &[], Lang::Vi)[0].label,
            "Tất cả"
        );
    }

    #[test]
    fn all_is_off_when_any_filter_is_on() {
        let f = Filters {
            pinned_only: true,
            ..Filters::default()
        };
        assert!(!history_filters(&f, &[], Lang::En)[0].on);
        assert!(history_filters(&f, &[], Lang::En)[1].on);
    }

    #[test]
    fn clicking_all_clears_filters_but_keeps_the_search_text() {
        let f = Filters {
            text: "abc".into(),
            kinds: vec![ClipType::Url],
            pinned_only: true,
            time: TimeFilter::Week,
            source: Some("Code".into()),
            ..Filters::default()
        };
        let g = toggle(&f, CHIP_ALL, &srcs()).unwrap();
        assert_eq!(g.text, "abc");
        assert!(is_all(&g));
    }

    #[test]
    fn all_is_off_while_a_collection_or_a_tag_filter_is_on() {
        let c = Filters {
            collection: Some(darkclip_core::CollectionId::new_v7(
                darkclip_core::Timestamp(1_795_320_000_000),
            )),
            ..Filters::default()
        };
        assert!(!is_all(&c));
        let t = Filters {
            tag: Some("todo".into()),
            ..Filters::default()
        };
        assert!(!is_all(&t));
        assert!(!history_filters(&t, &[], Lang::En)[0].on);
        // "All" clears them too.
        let g = toggle(&t, CHIP_ALL, &[]).unwrap();
        assert_eq!(g.tag, None);
        assert!(is_all(&g));
    }

    #[test]
    fn date_chips_are_exclusive_and_toggle_off() {
        let f = toggle(&Filters::default(), CHIP_MONTH, &[]).unwrap();
        assert_eq!(f.time, TimeFilter::Month);
        let f = toggle(&f, CHIP_TODAY, &[]).unwrap();
        assert_eq!(f.time, TimeFilter::Today);
        let f = toggle(&f, CHIP_TODAY, &[]).unwrap();
        assert_eq!(f.time, TimeFilter::Any);
    }

    #[test]
    fn kind_pinned_and_source_chips_toggle() {
        let f = toggle(&Filters::default(), 2, &[]).unwrap();
        assert_eq!(f.kinds, [ClipType::Url]);
        let f = toggle(&f, 2, &[]).unwrap();
        assert!(f.kinds.is_empty());
        let f = toggle(&f, CHIP_PINNED, &[]).unwrap();
        assert!(f.pinned_only);
        let f = toggle(&f, CHIP_SOURCE + 1, &srcs()).unwrap();
        assert_eq!(f.source.as_deref(), Some("Edge"));
        let f = toggle(&f, CHIP_SOURCE + 1, &srcs()).unwrap();
        assert_eq!(f.source, None);
    }

    #[test]
    fn unknown_chips_do_nothing() {
        assert_eq!(toggle(&Filters::default(), CHIP_SOURCE + 5, &srcs()), None);
        assert_eq!(toggle(&Filters::default(), 50, &srcs()), None);
        assert_eq!(toggle(&Filters::default(), -1, &srcs()), None);
    }

    // ---- Task 701: the popup segment, More filters and the extras ----

    use crate::organize::nav::{Nav, NAV_COLLECTION, NAV_FAVORITES};
    use darkclip_app::api::CollectionView;
    use darkclip_core::{CollectionId, Timestamp};

    const T0: i64 = 1_795_320_000_000;

    fn col(n: i64, name: &str, system: bool) -> CollectionView {
        CollectionView {
            id: CollectionId::new_v7(Timestamp(T0 + n)),
            name: name.to_string(),
            system,
            clip_count: 0,
        }
    }

    fn nav() -> Nav {
        Nav {
            collections: vec![col(0, "Favorites", true), col(1, "Work", false)],
            tags: Vec::new(),
        }
    }

    #[test]
    fn the_segment_has_the_designed_tabs_in_order() {
        let v = popup_segment(&Filters::default(), &nav(), Lang::En);
        let ids: Vec<i32> = v.iter().map(|t| t.id).collect();
        assert_eq!(ids, [CHIP_ALL, 0, 2, 3, 4, 5, CHIP_PINNED, NAV_FAVORITES]);
        let icons: Vec<TabIcon> = v.iter().map(|t| t.icon).collect();
        assert_eq!(
            icons,
            [
                TabIcon::All,
                TabIcon::Text,
                TabIcon::Link,
                TabIcon::Code,
                TabIcon::Image,
                TabIcon::Files,
                TabIcon::Pinned,
                TabIcon::Favorites
            ]
        );
        // The divider sits before Pinned and nowhere else.
        let dividers: Vec<usize> = v
            .iter()
            .enumerate()
            .filter(|(_, t)| t.divider_before)
            .map(|(i, _)| i)
            .collect();
        assert_eq!(dividers, [6]);
        let labels: Vec<&str> = v.iter().map(|t| t.label.as_str()).collect();
        assert_eq!(
            labels,
            ["All", "Text", "Link", "Code", "Image", "Files", "Pinned", "Favorites"]
        );
        assert_eq!(
            popup_segment(&Filters::default(), &nav(), Lang::Vi)[0].label,
            "Tất cả"
        );
    }

    #[test]
    fn the_segment_reflects_the_filters_exactly() {
        let n = nav();
        let on = |f: &Filters| -> Vec<i32> {
            popup_segment(f, &n, Lang::En)
                .iter()
                .filter(|t| t.on)
                .map(|t| t.id)
                .collect()
        };
        assert_eq!(on(&Filters::default()), [CHIP_ALL]);
        let f = Filters {
            kinds: vec![ClipType::Url, ClipType::Code],
            pinned_only: true,
            collection: n.favorites_id(),
            ..Filters::default()
        };
        assert_eq!(on(&f), [2, 3, CHIP_PINNED, NAV_FAVORITES]);
        // A filter that has no tab (Today, a user collection) still turns All off.
        let f = Filters {
            time: TimeFilter::Today,
            ..Filters::default()
        };
        assert!(on(&f).is_empty());
        // Without the Favorites collection loaded the tab exists and is off.
        let none = popup_segment(&Filters::default(), &Nav::default(), Lang::En);
        assert_eq!(none.len(), 8);
        assert!(!none[7].on);
    }

    #[test]
    fn segment_clicks_equal_todays_chip_toggles() {
        let n = nav();
        let empty: Vec<String> = Vec::new();
        for tab in popup_segment(&Filters::default(), &n, Lang::En) {
            let via_segment = if crate::organize::nav::is_nav(tab.id) {
                crate::organize::nav::toggle_nav(&Filters::default(), tab.id, &n)
            } else {
                toggle(&Filters::default(), tab.id, &empty)
            };
            assert!(via_segment.is_some(), "tab {} does something", tab.id);
        }
        // A kind tab toggles that kind in and out, as the kind chip did.
        let f = toggle(&Filters::default(), 2, &empty).unwrap();
        assert_eq!(f.kinds, [ClipType::Url]);
        let seg = popup_segment(&f, &n, Lang::En);
        assert!(seg[2].on && !seg[0].on);
        // All clears every filter but the text, like the history window's All.
        let f = Filters {
            text: "abc".into(),
            kinds: vec![ClipType::Url],
            pinned_only: true,
            ..Filters::default()
        };
        let g = toggle(&f, CHIP_ALL, &empty).unwrap();
        assert!(is_all(&g));
        assert_eq!(g.text, "abc");
    }

    #[test]
    fn ctrl_digits_pick_the_segment_tabs() {
        assert_eq!(segment_chip(1), Some(CHIP_ALL));
        assert_eq!(segment_chip(2), Some(0));
        assert_eq!(segment_chip(3), Some(2));
        assert_eq!(segment_chip(4), Some(3));
        assert_eq!(segment_chip(5), Some(4));
        assert_eq!(segment_chip(6), Some(5));
        assert_eq!(segment_chip(7), Some(CHIP_PINNED));
        assert_eq!(segment_chip(8), Some(NAV_FAVORITES));
        assert_eq!(segment_chip(9), None);
        assert_eq!(segment_chip(0), None);
        assert_eq!(segment_chip(10), None);
        // Each number is the n-th tab of the segment.
        let ids: Vec<i32> = popup_segment(&Filters::default(), &nav(), Lang::En)
            .iter()
            .map(|t| t.id)
            .collect();
        for n in 1..=8u8 {
            assert_eq!(segment_chip(n), ids.get(usize::from(n) - 1).copied());
        }
    }

    fn srcs2() -> Vec<String> {
        vec!["Code".to_string(), "Edge".to_string()]
    }

    #[test]
    fn more_filters_lists_the_extras_in_todays_order() {
        let v = more_filter_items(
            &Filters::default(),
            &nav(),
            &srcs2(),
            Lang::En,
            MoreLevel::Top,
        );
        let labels: Vec<&str> = v.iter().map(|e| e.label.as_str()).collect();
        assert_eq!(
            labels,
            [
                "Today",
                "Last 7 days",
                "Rich text",
                "Color",
                "Collections",
                "Apps"
            ]
        );
        let subs: Vec<bool> = v.iter().map(|e| e.sub).collect();
        assert_eq!(subs, [false, false, false, false, true, true]);
        assert!(v.iter().all(|e| !e.on));
        // Groups with nothing in them are left out.
        let v = more_filter_items(
            &Filters::default(),
            &Nav::default(),
            &[],
            Lang::En,
            MoreLevel::Top,
        );
        assert_eq!(v.len(), 4);
        assert_eq!(
            more_filter_items(&Filters::default(), &nav(), &[], Lang::Vi, MoreLevel::Top)[1]
                .label,
            "7 ngày qua"
        );
    }

    #[test]
    fn more_filters_second_level_lists_collections_and_apps() {
        let cols = more_filter_items(
            &Filters::default(),
            &nav(),
            &srcs2(),
            Lang::En,
            MoreLevel::Collections,
        );
        let labels: Vec<&str> = cols.iter().map(|e| e.label.as_str()).collect();
        assert_eq!(labels, ["Work"], "the system Favorites is a segment tab");
        assert_eq!(cols[0].item.chip_id(), Some(NAV_COLLECTION));
        let apps = more_filter_items(
            &Filters::default(),
            &nav(),
            &srcs2(),
            Lang::En,
            MoreLevel::Apps,
        );
        let ids: Vec<Option<i32>> = apps.iter().map(|e| e.item.chip_id()).collect();
        assert_eq!(ids, [Some(CHIP_SOURCE), Some(CHIP_SOURCE + 1)]);
        assert!(apps.len() <= 10 && cols.len() <= 10);
    }

    #[test]
    fn more_filters_toggle_like_the_chips_did() {
        let n = nav();
        let sources = srcs2();
        let f = Filters {
            time: TimeFilter::Week,
            kinds: vec![ClipType::Color],
            source: Some("Edge".into()),
            collection: n.user_collections().first().map(|c| c.id),
            ..Filters::default()
        };
        let top = more_filter_items(&f, &n, &sources, Lang::En, MoreLevel::Top);
        let on: Vec<&str> = top
            .iter()
            .filter(|e| e.on)
            .map(|e| e.label.as_str())
            .collect();
        assert_eq!(on, ["Last 7 days", "Color"]);
        let apps = more_filter_items(&f, &n, &sources, Lang::En, MoreLevel::Apps);
        assert_eq!(apps.iter().filter(|e| e.on).count(), 1);
        let cols = more_filter_items(&f, &n, &sources, Lang::En, MoreLevel::Collections);
        assert_eq!(cols.iter().filter(|e| e.on).count(), 1);
        // Today / Rich text / Color ids are the chip ids `toggle` already knows.
        let ids: Vec<Option<i32>> = top.iter().map(|e| e.item.chip_id()).collect();
        assert_eq!(
            ids,
            [
                Some(CHIP_TODAY),
                Some(CHIP_WEEK),
                Some(1),
                Some(6),
                None,
                None
            ]
        );
        for id in ids.into_iter().flatten() {
            assert!(toggle(&Filters::default(), id, &sources).is_some());
        }
    }

    #[test]
    fn only_active_extras_show_as_removable_chips() {
        let n = nav();
        assert!(popup_extras(&Filters::default(), &n, &srcs2(), Lang::En).is_empty());
        let f = Filters {
            time: TimeFilter::Today,
            kinds: vec![ClipType::Text, ClipType::RichText, ClipType::Color],
            pinned_only: true,
            source: Some("Edge".into()),
            collection: n.user_collections().first().map(|c| c.id),
            ..Filters::default()
        };
        let v = popup_extras(&f, &n, &srcs2(), Lang::En);
        let labels: Vec<&str> = v.iter().map(|c| c.label.as_str()).collect();
        assert_eq!(labels, ["Today", "Rich text", "Color", "Work", "Edge"]);
        assert!(v.iter().all(|c| c.on && !c.header));
        // Their ids toggle them off.
        let ids: Vec<i32> = v.iter().map(|c| c.id).collect();
        assert_eq!(ids, [CHIP_TODAY, 1, 6, NAV_COLLECTION, CHIP_SOURCE + 1]);
        // The 7-day filter shows too; the Favorites collection does not (it is a tab).
        let g = Filters {
            time: TimeFilter::Week,
            collection: n.favorites_id(),
            ..Filters::default()
        };
        let labels: Vec<String> = popup_extras(&g, &n, &srcs2(), Lang::En)
            .into_iter()
            .map(|c| c.label)
            .collect();
        assert_eq!(labels, ["Last 7 days"]);
    }
}
