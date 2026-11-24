//! Row formatting for the popup list (Task 206): kind icon, meta line with the source app and
//! the relative time, badges, and the two-line preview split. Pure and window-free. Previews,
//! source names and times are never logged (GC-06).

use super::highlight::Segment;
use super::list::Item;
use darkclip_core::i18n::{t, Lang, Msg};
use darkclip_core::{ClipType, Timestamp};

/// Characters on one preview line (proportional font).
pub const LINE_CHARS: usize = 44;
/// Characters on one preview line of a code clip (monospace).
pub const MONO_LINE_CHARS: usize = 38;

pub fn kind_msg(k: ClipType) -> Msg {
    match k {
        ClipType::Text => Msg::ClipTypeText,
        ClipType::RichText => Msg::ClipTypeRichText,
        ClipType::Url => Msg::ClipTypeUrl,
        ClipType::Color => Msg::ClipTypeColor,
        ClipType::Code => Msg::ClipTypeCode,
        ClipType::Image => Msg::ClipTypeImage,
        ClipType::Files => Msg::ClipTypeFiles,
    }
}

/// `template` with `{n}` replaced by `n`.
pub fn fill(template: &str, n: u64) -> String {
    template.replace("{n}", &n.to_string())
}

/// "just now", "5 min ago", "3 h ago", "2 d ago". A time in the future counts as now.
pub fn relative_time(lang: Lang, now: Timestamp, then: Timestamp) -> String {
    let secs = now.0.saturating_sub(then.0).max(0) / 1000;
    let secs = u64::try_from(secs).unwrap_or(0);
    match secs {
        0..=59 => t(lang, Msg::UiTimeNow).to_string(),
        60..=3599 => fill(t(lang, Msg::UiTimeMinutes), secs / 60),
        3600..=86_399 => fill(t(lang, Msg::UiTimeHours), secs / 3600),
        _ => fill(t(lang, Msg::UiTimeDays), secs / 86_400),
    }
}

// ---- The 48 px popup row (Task 701) ----------------------------------------------------------

/// Which kit icon a row shows for its kind (the host maps it to an `IconName`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KindIcon {
    FileText,
    Link,
    Code,
    Image,
    File,
    Type,
}

pub fn kind_icon_name(k: ClipType) -> KindIcon {
    match k {
        ClipType::Text | ClipType::RichText => KindIcon::FileText,
        ClipType::Url => KindIcon::Link,
        ClipType::Code => KindIcon::Code,
        ClipType::Image => KindIcon::Image,
        ClipType::Files => KindIcon::File,
        ClipType::Color => KindIcon::Type,
    }
}

/// The texts of one popup row besides its preview: a meta line of source and age (no kind word,
/// no Alt prefix), the Alt+N number, the badges and the screen-reader description.
#[derive(Clone, PartialEq, Eq)]
pub struct PopupText {
    pub kind_icon: KindIcon,
    /// "Notepad · 5 min ago", or just the age when the source is unknown.
    pub meta: String,
    /// Screen-reader description: kind, source, age and the badges as words.
    pub description: String,
    /// "1".."9" for the first nine rows, empty after.
    pub number: String,
    pub pinned: bool,
    pub favorite: bool,
    pub masked: bool,
    pub mono: bool,
}

/// Never prints the source name (GC-06).
impl std::fmt::Debug for PopupText {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PopupText")
            .field("kind_icon", &self.kind_icon)
            .field("masked", &self.masked)
            .finish_non_exhaustive()
    }
}

pub fn popup_text(item: &Item, index: usize, lang: Lang, now: Timestamp) -> PopupText {
    let age = relative_time(lang, now, item.at);
    let (meta, mut description) = match &item.source {
        Some(src) => (
            format!("{src} · {age}"),
            format!("{} · {src} · {age}", t(lang, kind_msg(item.kind))),
        ),
        None => (
            age.clone(),
            format!("{} · {age}", t(lang, kind_msg(item.kind))),
        ),
    };
    let badges = [
        item.pinned.then(|| t(lang, Msg::UiFilterPinned)),
        item.favorite.then(|| t(lang, Msg::OrgFavoriteBadge)),
        item.masked.then(|| t(lang, Msg::UiMaskedBadge)),
    ];
    for b in badges.into_iter().flatten() {
        description.push_str(" · ");
        description.push_str(b);
    }
    PopupText {
        kind_icon: kind_icon_name(item.kind),
        meta,
        description,
        number: if index < 9 {
            (index + 1).to_string()
        } else {
            String::new()
        },
        pinned: item.pinned,
        favorite: item.favorite,
        masked: item.masked,
        mono: item.kind == ClipType::Code,
    }
}

/// The preview line of a masked clip: a fixed i18n text. The stored masked preview (the first
/// characters of the secret and its label) is never shown in the popup. `None` for a clip that
/// is not masked: its own preview is used.
pub fn popup_preview_text(item: &Item, lang: Lang) -> Option<&'static str> {
    item.masked.then(|| t(lang, Msg::UiMaskedRowText))
}

/// The texts of one history row besides its preview (Task 703): meta line of source, age and the
/// first tag (else the collection the row is known to be in), no Alt+N and no kind word.
#[derive(Clone, PartialEq, Eq)]
pub struct HistoryText {
    pub kind_icon: KindIcon,
    /// "DBeaver · 1 h ago · sql".
    pub meta: String,
    /// Screen-reader description: kind, source, age, tag or collection, and the badges as words.
    pub description: String,
    pub pinned: bool,
    pub favorite: bool,
    pub masked: bool,
    pub mono: bool,
}

/// Never prints the source name or a tag (GC-06).
impl std::fmt::Debug for HistoryText {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HistoryText")
            .field("kind_icon", &self.kind_icon)
            .field("masked", &self.masked)
            .finish_non_exhaustive()
    }
}

/// The row texts of `item`. A list row knows its tags and whether it is a favorite (a member of
/// the Favorites collection); other collections are not part of the list data, so a favorite
/// shows "Favorites" when it has no tag. A masked clip shows neither tag nor collection.
pub fn history_text(item: &Item, lang: Lang, now: Timestamp) -> HistoryText {
    let age = relative_time(lang, now, item.at);
    let extra: Option<&str> = if item.masked {
        None
    } else {
        item.tags
            .first()
            .map(String::as_str)
            .or_else(|| item.favorite.then(|| t(lang, Msg::OrgFavorites)))
    };
    let mut parts: Vec<&str> = Vec::new();
    if let Some(src) = item.source.as_deref() {
        parts.push(src);
    }
    parts.push(&age);
    if let Some(x) = extra {
        parts.push(x);
    }
    let meta = parts.join(" · ");
    let mut description = format!("{} · {meta}", t(lang, kind_msg(item.kind)));
    let badges = [
        item.pinned.then(|| t(lang, Msg::UiFilterPinned)),
        item.favorite.then(|| t(lang, Msg::OrgFavoriteBadge)),
        item.masked.then(|| t(lang, Msg::UiMaskedBadge)),
    ];
    for b in badges.into_iter().flatten() {
        description.push_str(" · ");
        description.push_str(b);
    }
    HistoryText {
        kind_icon: kind_icon_name(item.kind),
        meta,
        description,
        pinned: item.pinned,
        favorite: item.favorite,
        masked: item.masked,
        mono: item.kind == ClipType::Code,
    }
}

/// Where an overline (PINNED n / RECENT n) goes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Section {
    None,
    FirstPinned,
    FirstRecent,
}

impl Section {
    /// The value of `ClipRow.section` (0 none, 1 first pinned, 2 first recent).
    pub fn code(self) -> i32 {
        match self {
            Section::None => 0,
            Section::FirstPinned => 1,
            Section::FirstRecent => 2,
        }
    }
}

/// The overline of every row: the first pinned row and the first recent row. With no pinned clip
/// the list is one flat list.
pub fn section_for(items: &[&Item]) -> Vec<Section> {
    let mut out = vec![Section::None; items.len()];
    if !items.iter().any(|i| i.pinned) {
        return out;
    }
    if let Some(at) = items.iter().position(|i| i.pinned) {
        if let Some(s) = out.get_mut(at) {
            *s = Section::FirstPinned;
        }
    }
    if let Some(at) = items.iter().position(|i| !i.pinned) {
        if let Some(s) = out.get_mut(at) {
            *s = Section::FirstRecent;
        }
    }
    out
}

/// (pinned, recent) rows among `items`.
pub fn section_counts(items: &[&Item]) -> (usize, usize) {
    let pinned = items.iter().filter(|i| i.pinned).count();
    (pinned, items.len() - pinned)
}

/// "PINNED 2" / "RECENT 128" for the overline of `s`; empty for no overline.
pub fn section_title(s: Section, pinned: usize, recent: usize, lang: Lang) -> String {
    let n = |v: usize| u64::try_from(v).unwrap_or(u64::MAX);
    match s {
        Section::None => String::new(),
        Section::FirstPinned => fill(t(lang, Msg::UiSectionPinned), n(pinned)),
        Section::FirstRecent => fill(t(lang, Msg::UiSectionRecent), n(recent)),
    }
}

/// Overlines show only in the plain list: no search text and no filter (then one flat list).
pub fn sections_visible(f: &super::list::Filters) -> bool {
    f.is_empty()
}

/// Splits highlighted segments into at most two lines of `line_chars` characters. The break is
/// the last space in the second half of the first line, else a hard cut; the space is dropped.
/// Highlight flags are kept. The second line is empty when everything fits on the first.
pub fn split_two_lines(segs: Vec<Segment>, line_chars: usize) -> (Vec<Segment>, Vec<Segment>) {
    let line_chars = line_chars.max(2);
    let flat: Vec<(char, bool)> = segs
        .iter()
        .flat_map(|s| s.text.chars().map(move |c| (c, s.hit)))
        .collect();
    if flat.len() <= line_chars {
        return (segs, Vec::new());
    }
    let window = flat.get(..=line_chars).unwrap_or(&flat);
    let space = window
        .iter()
        .rposition(|(c, _)| *c == ' ')
        .filter(|&i| i >= line_chars / 2);
    let (cut, skip) = match space {
        Some(i) => (i, 1),
        None => (line_chars, 0),
    };
    let regroup = |chars: &[(char, bool)]| -> Vec<Segment> {
        let mut out: Vec<Segment> = Vec::new();
        for &(c, hit) in chars {
            match out.last_mut() {
                Some(last) if last.hit == hit => last.text.push(c),
                _ => out.push(Segment {
                    text: c.to_string(),
                    hit,
                }),
            }
        }
        out
    };
    let first = flat.get(..cut).unwrap_or(&flat);
    let second = flat.get(cut + skip..).unwrap_or(&[]);
    (regroup(first), regroup(second))
}

#[cfg(test)]
mod tests {
    use super::*;
    use darkclip_core::ClipId;

    const NOW: Timestamp = Timestamp(1_795_320_000_000);

    fn item(kind: ClipType) -> Item {
        Item {
            id: ClipId::new_v7(NOW),
            kind,
            preview: "preview".into(),
            pinned: false,
            masked: false,
            source: None,
            at: Timestamp(NOW.0 - 5 * 60_000),
            favorite: false,
            tags: Vec::new(),
        }
    }

    fn seg(text: &str, hit: bool) -> Segment {
        Segment {
            text: text.into(),
            hit,
        }
    }

    fn joined(v: &[Segment]) -> String {
        v.iter().map(|s| s.text.as_str()).collect()
    }

    #[test]
    fn relative_time_en_and_vi() {
        let ago = |ms: i64, lang| relative_time(lang, NOW, Timestamp(NOW.0 - ms));
        assert_eq!(ago(10_000, Lang::En), "just now");
        assert_eq!(ago(10_000, Lang::Vi), "vừa xong");
        assert_eq!(ago(5 * 60_000, Lang::En), "5 min ago");
        assert_eq!(ago(5 * 60_000, Lang::Vi), "5 phút trước");
        assert_eq!(ago(3 * 3_600_000, Lang::En), "3 h ago");
        assert_eq!(ago(3 * 3_600_000, Lang::Vi), "3 giờ trước");
        assert_eq!(ago(2 * 86_400_000 + 5, Lang::En), "2 d ago");
        assert_eq!(ago(2 * 86_400_000 + 5, Lang::Vi), "2 ngày trước");
        // Edges and a clock that went backwards.
        assert_eq!(ago(59_999, Lang::En), "just now");
        assert_eq!(ago(60_000, Lang::En), "1 min ago");
        assert_eq!(ago(-5_000, Lang::En), "just now");
    }

    #[test]
    fn short_text_stays_on_one_line() {
        let (a, b) = split_two_lines(vec![seg("hello", false)], 10);
        assert_eq!(joined(&a), "hello");
        assert!(b.is_empty());
    }

    #[test]
    fn long_text_breaks_at_a_space_or_hard() {
        let (a, b) = split_two_lines(vec![seg("aaaa bbbb cccc dddd", false)], 10);
        assert_eq!(
            (joined(&a).as_str(), joined(&b).as_str()),
            ("aaaa bbbb", "cccc dddd")
        );
        let (a, b) = split_two_lines(vec![seg(&"x".repeat(25), false)], 10);
        assert_eq!((joined(&a).len(), joined(&b).len()), (10, 15));
        // A space too early in the line is not used (it would waste the first line).
        let (a, b) = split_two_lines(vec![seg(&format!("ab {}", "y".repeat(20)), false)], 10);
        assert_eq!(joined(&a).len(), 10);
        assert_eq!(joined(&b).len(), 13);
    }

    #[test]
    fn highlights_survive_the_split_with_wide_characters() {
        let (a, b) = split_two_lines(
            vec![seg("foo ", false), seg("bar", true), seg(" baz qux", false)],
            8,
        );
        assert_eq!(a, vec![seg("foo ", false), seg("bar", true)]);
        assert_eq!(b, vec![seg("baz qux", false)]);
        // Multi-byte characters are counted as characters, never cut in the middle.
        let (a, b) = split_two_lines(vec![seg("ệệệệệệệệệệệệ", true)], 5);
        assert_eq!(
            (joined(&a).chars().count(), joined(&b).chars().count()),
            (5, 7)
        );
        assert!(a.iter().chain(&b).all(|s| s.hit));
    }

    // ---- Task 701: the 48 px popup row ----

    fn pinned_item(pinned: bool) -> Item {
        let mut i = item(ClipType::Text);
        i.pinned = pinned;
        i
    }

    #[test]
    fn kind_icons_follow_the_design() {
        assert_eq!(kind_icon_name(ClipType::Text), KindIcon::FileText);
        assert_eq!(kind_icon_name(ClipType::RichText), KindIcon::FileText);
        assert_eq!(kind_icon_name(ClipType::Url), KindIcon::Link);
        assert_eq!(kind_icon_name(ClipType::Code), KindIcon::Code);
        assert_eq!(kind_icon_name(ClipType::Image), KindIcon::Image);
        assert_eq!(kind_icon_name(ClipType::Files), KindIcon::File);
        assert_eq!(kind_icon_name(ClipType::Color), KindIcon::Type);
    }

    #[test]
    fn popup_meta_is_source_and_age_without_kind_word_or_alt() {
        let mut i = item(ClipType::Url);
        i.source = Some("Edge".into());
        for lang in [Lang::En, Lang::Vi] {
            let p = popup_text(&i, 0, lang, NOW);
            assert!(!p.meta.contains("Alt+"), "{}", p.meta);
            assert!(!p.meta.contains(t(lang, kind_msg(ClipType::Url))));
            assert!(p.meta.starts_with("Edge · "), "{}", p.meta);
        }
        assert_eq!(popup_text(&i, 0, Lang::En, NOW).meta, "Edge · 5 min ago");
        assert_eq!(popup_text(&i, 0, Lang::Vi, NOW).meta, "Edge · 5 phút trước");
        i.source = None;
        assert_eq!(popup_text(&i, 3, Lang::En, NOW).meta, "5 min ago");
        // The screen-reader description keeps the kind word.
        assert!(popup_text(&i, 3, Lang::En, NOW)
            .description
            .starts_with("Link · "));
    }

    #[test]
    fn the_alt_number_is_only_for_the_first_nine_rows() {
        let i = item(ClipType::Text);
        for n in 0..9 {
            assert_eq!(popup_text(&i, n, Lang::En, NOW).number, (n + 1).to_string());
        }
        assert_eq!(popup_text(&i, 9, Lang::En, NOW).number, "");
        assert_eq!(popup_text(&i, 500, Lang::En, NOW).number, "");
    }

    #[test]
    fn badges_say_pinned_favorite_masked_in_the_description() {
        let mut i = item(ClipType::Text);
        i.pinned = true;
        i.favorite = true;
        i.masked = true;
        let p = popup_text(&i, 0, Lang::En, NOW);
        assert!(p.pinned && p.favorite && p.masked);
        assert!(p.description.ends_with("Pinned · Favorite · Masked"));
        let vi = popup_text(&i, 0, Lang::Vi, NOW);
        assert!(vi.description.ends_with("Đã ghim · Yêu thích · Đã che"));
        assert!(!popup_text(&item(ClipType::Code), 0, Lang::En, NOW).pinned);
        assert!(popup_text(&item(ClipType::Code), 0, Lang::En, NOW).mono);
    }

    #[test]
    fn a_masked_row_shows_only_the_i18n_text_never_the_stored_preview() {
        let mut i = item(ClipType::Text);
        i.masked = true;
        i.source = Some("Vault".into());
        // What storage holds for a masked clip: the first characters of the secret and a label.
        i.preview = "abc…•••• (Password)".into();
        for lang in [Lang::En, Lang::Vi] {
            let line = popup_preview_text(&i, lang);
            assert_eq!(line, Some(t(lang, Msg::UiMaskedRowText)));
            let p = popup_text(&i, 0, lang, NOW);
            for text in [&p.meta, &p.description] {
                assert!(!text.contains("abc"), "{text}");
                assert!(!text.contains("Password"), "{text}");
                assert!(!text.contains('•'), "{text}");
            }
        }
        // A clip that is not masked uses its own preview.
        assert_eq!(popup_preview_text(&item(ClipType::Text), Lang::En), None);
    }

    #[test]
    fn debug_of_the_row_texts_never_prints_the_source() {
        let mut i = item(ClipType::Text);
        i.source = Some("SecretApp.exe".into());
        let s = format!("{:?}", popup_text(&i, 0, Lang::En, NOW));
        assert!(!s.contains("SecretApp"), "{s}");
    }

    #[test]
    fn sections_split_pinned_and_recent_rows() {
        let items = [
            pinned_item(true),
            pinned_item(true),
            pinned_item(false),
            pinned_item(false),
            pinned_item(false),
        ];
        let refs: Vec<&Item> = items.iter().collect();
        assert_eq!(
            section_for(&refs),
            [
                Section::FirstPinned,
                Section::None,
                Section::FirstRecent,
                Section::None,
                Section::None
            ]
        );
        assert_eq!(section_counts(&refs), (2, 3));
        // No pinned clip: one flat list, no overlines.
        let flat: Vec<&Item> = items[2..].iter().collect();
        assert!(section_for(&flat).iter().all(|s| *s == Section::None));
        // Only pinned clips: just the pinned overline.
        let only: Vec<&Item> = items[..2].iter().collect();
        assert_eq!(section_for(&only)[0], Section::FirstPinned);
        assert!(section_for(&only)[1..].iter().all(|s| *s == Section::None));
        assert!(section_for(&[]).is_empty());
        assert_eq!(section_counts(&[]), (0, 0));
    }

    #[test]
    fn section_titles_carry_their_count_in_both_languages() {
        assert_eq!(
            section_title(Section::FirstPinned, 2, 5, Lang::En),
            "PINNED 2"
        );
        assert_eq!(
            section_title(Section::FirstRecent, 2, 5, Lang::En),
            "RECENT 5"
        );
        assert_eq!(section_title(Section::None, 2, 5, Lang::En), "");
        assert_eq!(
            section_title(Section::FirstPinned, 2, 5, Lang::Vi),
            "ĐÃ GHIM 2"
        );
        assert_eq!(
            section_title(Section::FirstRecent, 2, 5, Lang::Vi),
            "GẦN ĐÂY 5"
        );
        assert_eq!(Section::None.code(), 0);
        assert_eq!(Section::FirstPinned.code(), 1);
        assert_eq!(Section::FirstRecent.code(), 2);
    }

    #[test]
    fn overlines_show_only_without_a_search_or_filter() {
        use crate::popup::list::{Filters, TimeFilter};
        assert!(sections_visible(&Filters::default()));
        assert!(!sections_visible(&Filters {
            text: "a".into(),
            ..Filters::default()
        }));
        assert!(!sections_visible(&Filters {
            pinned_only: true,
            ..Filters::default()
        }));
        assert!(!sections_visible(&Filters {
            time: TimeFilter::Week,
            ..Filters::default()
        }));
        // Whitespace only is not a search.
        assert!(sections_visible(&Filters {
            text: "  ".into(),
            ..Filters::default()
        }));
    }

    // ---- The history row (Task 703) ----

    #[test]
    fn history_meta_is_source_age_and_the_first_tag_or_collection() {
        let mut i = item(ClipType::Text);
        i.source = Some("DBeaver".into());
        // Nothing to add: source and age only; no Alt+N, no kind word.
        let m = history_text(&i, Lang::En, NOW);
        assert_eq!(m.meta, "DBeaver · 5 min ago");
        assert!(!m.meta.contains("Alt+") && !m.meta.contains("Text"));
        // No source: just the age.
        i.source = None;
        assert_eq!(history_text(&i, Lang::En, NOW).meta, "5 min ago");
        // The first tag comes next.
        i.source = Some("DBeaver".into());
        i.tags = vec!["sql".into(), "work".into()];
        assert_eq!(
            history_text(&i, Lang::En, NOW).meta,
            "DBeaver · 5 min ago · sql"
        );
        // A tag wins over the collection (Favorites is the one collection a row knows).
        i.favorite = true;
        assert_eq!(
            history_text(&i, Lang::En, NOW).meta,
            "DBeaver · 5 min ago · sql"
        );
        i.tags.clear();
        assert_eq!(
            history_text(&i, Lang::En, NOW).meta,
            "DBeaver · 5 min ago · Favorites"
        );
        assert_eq!(
            history_text(&i, Lang::Vi, NOW).meta,
            "DBeaver · 5 phút trước · Yêu thích"
        );
    }

    #[test]
    fn history_description_names_kind_badges_and_the_extra_for_screen_readers() {
        let mut i = item(ClipType::Url);
        i.source = Some("Edge".into());
        i.pinned = true;
        i.favorite = true;
        i.tags = vec!["todo".into()];
        let d = history_text(&i, Lang::En, NOW).description;
        assert_eq!(d, "Link · Edge · 5 min ago · todo · Pinned · Favorite");
        assert!(!d.contains("Alt+"));
        let vi = history_text(&i, Lang::Vi, NOW).description;
        assert!(vi.ends_with("Đã ghim · Yêu thích"));
    }

    #[test]
    fn a_masked_history_row_carries_no_tag_or_collection() {
        let mut i = item(ClipType::Text);
        i.masked = true;
        i.source = Some("KeePass".into());
        i.tags = vec!["secret-tag".into()];
        i.favorite = true;
        let m = history_text(&i, Lang::En, NOW);
        assert_eq!(m.meta, "KeePass · 5 min ago");
        assert!(!m.description.contains("secret-tag"));
        assert!(m.masked && m.description.ends_with("Favorite · Masked"));
    }

    #[test]
    fn only_code_rows_are_monospace() {
        assert!(history_text(&item(ClipType::Code), Lang::En, NOW).mono);
        for k in [
            ClipType::Text,
            ClipType::RichText,
            ClipType::Url,
            ClipType::Color,
            ClipType::Image,
            ClipType::Files,
        ] {
            assert!(!history_text(&item(k), Lang::En, NOW).mono);
        }
    }

    #[test]
    fn history_text_debug_hides_names() {
        let mut i = item(ClipType::Text);
        i.source = Some("PrivateApp".into());
        i.tags = vec!["hidden-tag".into()];
        let s = format!("{:?}", history_text(&i, Lang::En, NOW));
        assert!(!s.contains("PrivateApp") && !s.contains("hidden-tag"));
    }

    /// Task 707: the Narrator text of both windows' rows (description) in both languages.
    #[test]
    fn narrator_description_has_kind_age_source_and_every_badge_in_both_languages() {
        let mut i = item(ClipType::Url);
        i.source = Some("Edge".into());
        i.pinned = true;
        i.favorite = true;
        i.masked = true;
        for lang in [Lang::En, Lang::Vi] {
            let age = relative_time(lang, NOW, i.at);
            let want = [
                t(lang, kind_msg(ClipType::Url)),
                "Edge",
                t(lang, Msg::UiFilterPinned),
                t(lang, Msg::OrgFavoriteBadge),
                t(lang, Msg::UiMaskedBadge),
                age.as_str(),
            ];
            let popup = popup_text(&i, 0, lang, NOW).description;
            let history = history_text(&i, lang, NOW).description;
            for d in [&popup, &history] {
                for w in want {
                    assert!(d.contains(w), "{lang:?}: {d:?} lacks {w:?}");
                }
            }
        }
    }

    /// Review Focus 1: neither the label's source (the masked line) nor the description of a
    /// masked clip holds any piece of the content or of its stored preview, in either window.
    #[test]
    fn a_masked_row_label_and_description_never_hold_the_secret_in_either_window() {
        let secret = format!("{}{}", "hunter", "2-zebra-quartz");
        let mut i = item(ClipType::Text);
        i.masked = true;
        i.source = Some("Vault".into());
        i.tags = vec!["work".into()];
        i.preview = format!("{}…•••• (Password)", &secret[..4]);
        for lang in [Lang::En, Lang::Vi] {
            let p = popup_text(&i, 0, lang, NOW);
            let h = history_text(&i, lang, NOW);
            // The row label is the masked line from i18n (what `ui/common.rs` binds).
            let label = popup_preview_text(&i, lang).unwrap_or_default();
            assert_eq!(label, t(lang, Msg::UiMaskedRowText));
            for text in [label, &p.meta, &p.description, &h.meta, &h.description] {
                for n in 3..=secret.len() {
                    assert!(
                        !text.contains(&secret[..n]),
                        "{lang:?}: {text:?} leaks the content"
                    );
                }
                for piece in ["•", "(Password)"] {
                    assert!(
                        !text.contains(piece),
                        "{lang:?}: {text:?} leaks the preview"
                    );
                }
            }
        }
    }
}
