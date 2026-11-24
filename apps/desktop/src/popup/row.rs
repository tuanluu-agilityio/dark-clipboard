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

/// A short symbol for the kind badge. The kind's name stays in the meta line and in the
/// accessible description, so the symbol is decoration.
pub fn kind_icon(k: ClipType) -> &'static str {
    match k {
        ClipType::Text => "Aa",
        ClipType::RichText => "Rt",
        ClipType::Url => "://",
        ClipType::Color => "#",
        ClipType::Code => "</>",
        ClipType::Image => "Img",
        ClipType::Files => "Fi",
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

/// The texts of one row besides its preview.
#[derive(Clone, PartialEq, Eq)]
pub struct RowText {
    pub icon: &'static str,
    /// "Alt+1 · Text · Notepad · 5 min ago" (the Alt hint only for the first nine rows).
    pub meta: String,
    /// Screen-reader description: the meta without the shortcut, plus the badges.
    pub description: String,
    pub pin_badge: Option<&'static str>,
    /// A star for a favorite clip (decoration; the screen-reader description says "Favorite").
    pub favorite: bool,
    pub masked_badge: Option<&'static str>,
    /// Code clips use the monospace font.
    pub mono: bool,
}

/// Never prints the source name (GC-06).
impl std::fmt::Debug for RowText {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RowText")
            .field("icon", &self.icon)
            .field("mono", &self.mono)
            .finish_non_exhaustive()
    }
}

pub fn row_text(item: &Item, index: usize, lang: Lang, now: Timestamp) -> RowText {
    let mut detail = String::from(t(lang, kind_msg(item.kind)));
    if let Some(src) = &item.source {
        detail.push_str(" · ");
        detail.push_str(src);
    }
    detail.push_str(" · ");
    detail.push_str(&relative_time(lang, now, item.at));
    let meta = if index < 9 {
        format!("Alt+{} · {detail}", index + 1)
    } else {
        detail.clone()
    };
    let pin_badge = item.pinned.then(|| t(lang, Msg::UiFilterPinned));
    let masked_badge = item.masked.then(|| t(lang, Msg::UiMaskedBadge));
    let favorite_word = item.favorite.then(|| t(lang, Msg::OrgFavoriteBadge));
    let mut description = detail;
    for b in [pin_badge, favorite_word, masked_badge]
        .into_iter()
        .flatten()
    {
        description.push_str(" · ");
        description.push_str(b);
    }
    RowText {
        icon: kind_icon(item.kind),
        meta,
        description,
        pin_badge,
        favorite: item.favorite,
        masked_badge,
        mono: item.kind == ClipType::Code,
    }
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
    fn row_meta_has_kind_source_and_time_in_both_languages() {
        let mut i = item(ClipType::Text);
        i.source = Some("Notepad".into());
        let en = row_text(&i, 0, Lang::En, NOW);
        assert_eq!(en.meta, "Alt+1 · Text · Notepad · 5 min ago");
        assert_eq!(en.icon, "Aa");
        assert_eq!(en.pin_badge, None);
        assert_eq!(en.masked_badge, None);
        let vi = row_text(&i, 2, Lang::Vi, NOW);
        assert_eq!(vi.meta, "Alt+3 · Văn bản · Notepad · 5 phút trước");
        // The tenth row and later have no Alt hint; no source: no source part.
        i.source = None;
        assert_eq!(row_text(&i, 9, Lang::En, NOW).meta, "Text · 5 min ago");
    }

    #[test]
    fn pin_and_masked_badges_are_translated() {
        let mut i = item(ClipType::Text);
        i.pinned = true;
        i.masked = true;
        let en = row_text(&i, 0, Lang::En, NOW);
        assert_eq!(en.pin_badge, Some("Pinned"));
        assert_eq!(en.masked_badge, Some("Masked"));
        assert!(en.description.ends_with("Pinned · Masked"));
        assert!(!en.description.contains("Alt+"));
        let vi = row_text(&i, 0, Lang::Vi, NOW);
        assert_eq!(vi.pin_badge, Some("Đã ghim"));
        assert_eq!(vi.masked_badge, Some("Đã che"));
    }

    #[test]
    fn a_favorite_row_has_a_star_and_says_favorite_for_screen_readers() {
        let plain = row_text(&item(ClipType::Text), 0, Lang::En, NOW);
        assert!(!plain.favorite);
        assert!(!plain.description.contains("Favorite"));
        let mut i = item(ClipType::Text);
        i.favorite = true;
        i.pinned = true;
        let en = row_text(&i, 0, Lang::En, NOW);
        assert!(en.favorite);
        assert!(en.description.ends_with("Pinned · Favorite"));
        let vi = row_text(&i, 0, Lang::Vi, NOW);
        assert!(vi.description.ends_with("Đã ghim · Yêu thích"));
    }

    #[test]
    fn code_rows_are_monospace_others_are_not() {
        assert!(row_text(&item(ClipType::Code), 0, Lang::En, NOW).mono);
        for k in [
            ClipType::Text,
            ClipType::RichText,
            ClipType::Url,
            ClipType::Color,
            ClipType::Image,
            ClipType::Files,
        ] {
            assert!(!row_text(&item(k), 0, Lang::En, NOW).mono);
        }
    }

    #[test]
    fn every_kind_has_a_distinct_icon() {
        let kinds = [
            ClipType::Text,
            ClipType::RichText,
            ClipType::Url,
            ClipType::Color,
            ClipType::Code,
            ClipType::Image,
            ClipType::Files,
        ];
        let mut icons: Vec<&str> = kinds.iter().map(|k| kind_icon(*k)).collect();
        icons.sort_unstable();
        icons.dedup();
        assert_eq!(icons.len(), kinds.len());
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
}
