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

fn chip(id: i32, label: &str, on: bool) -> ChipSpec {
    ChipSpec {
        id,
        label: label.to_string(),
        on,
        header: false,
    }
}

fn header(label: &str) -> ChipSpec {
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
    f.kinds.is_empty() && !f.pinned_only && f.time == TimeFilter::Any && f.source.is_none()
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
}
