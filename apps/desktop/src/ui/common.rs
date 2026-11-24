//! Pieces the popup host and the history host share (Task 211): rows, the detail pane's state,
//! the worker side of detail loads and the strings both windows show. No business logic lives
//! here: the decisions are in `crate::popup` (pure view-models). Nothing is logged; previews,
//! queries and paths are never formatted into messages (GC-06, SR-11).

use super::generated::IconName;
use super::{ClipRow, DetailState, FileRow, Poster, Seg, Strings};
use crate::popup::detail::{
    file_entries, props, DetailData, DetailModel, DetailProps, DetailRequest, Pic, Want,
    FULL_IMAGE_SIDE, TEXT_DISPLAY_CAP,
};
use crate::popup::highlight::{row_segments, Segment};
use crate::popup::list::Item;
use crate::popup::row::{
    fill, history_text, kind_msg, popup_preview_text, popup_text, relative_time, split_two_lines,
    KindIcon, LINE_CHARS, MONO_LINE_CHARS,
};
use darkclip_app::api::Caller;
use darkclip_app::{ClipDetailView, PixelsView};
use darkclip_core::i18n::{t, Lang, Msg};
use darkclip_core::{ClipId, Clock};
use slint::{
    ComponentHandle, Image, ModelRc, Rgba8Pixel, SharedPixelBuffer, SharedString, VecModel,
};
use std::sync::{Arc, Mutex};

/// A decoded picture as the UI thread holds it (cheap to clone).
pub type Px = SharedPixelBuffer<Rgba8Pixel>;

/// One history row (Task 703): two preview lines, source, age and the first tag or collection.
/// A masked clip shows only the fixed i18n text: the stored masked preview (the first characters
/// of the secret) never reaches the row, its highlights, its label or its description, and the
/// search text is never matched against it (GC-06).
pub fn history_clip_row(item: &Item, _index: usize, lang: Lang, query: &str) -> ClipRow {
    let text = history_text(item, lang, crate::SystemClock.now());
    let line = if text.mono {
        MONO_LINE_CHARS
    } else {
        LINE_CHARS
    };
    let (first, second) = match popup_preview_text(item, lang) {
        Some(masked) => (
            vec![Segment {
                text: masked.to_string(),
                hit: false,
            }],
            Vec::new(),
        ),
        None => split_two_lines(row_segments(&item.preview, query, line * 2), line),
    };
    let label: String = first.iter().map(|s| s.text.as_str()).collect();
    let to_model = |v: Vec<Segment>| {
        ModelRc::new(VecModel::from(
            v.into_iter()
                .map(|s| Seg {
                    text: s.text.into(),
                    hit: s.hit,
                })
                .collect::<Vec<_>>(),
        ))
    };
    ClipRow {
        segs: to_model(first),
        segs2: to_model(second),
        meta: text.meta.into(),
        label: label.into(),
        description: text.description.into(),
        icon: SharedString::new(),
        pin_badge: SharedString::new(),
        favorite: text.favorite,
        masked_badge: SharedString::new(),
        mono: text.mono,
        pinned: text.pinned,
        marked: false,
        kind_icon: icon_name(text.kind_icon),
        number: SharedString::new(),
        section: 0,
        masked: text.masked,
    }
}

/// The facts of the detail panel: type, source app and when it was copied. Nothing of the
/// content, so a masked clip shows the same lines.
#[derive(Clone, PartialEq, Eq)]
pub struct HistoryFacts {
    pub kind: String,
    /// Empty when the source is unknown.
    pub from: String,
    pub copied: String,
}

/// Never prints the source name (GC-06).
impl std::fmt::Debug for HistoryFacts {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HistoryFacts")
            .field("kind", &self.kind)
            .finish_non_exhaustive()
    }
}

pub fn history_facts(item: &Item, lang: Lang, now: darkclip_core::Timestamp) -> HistoryFacts {
    HistoryFacts {
        kind: t(lang, kind_msg(item.kind)).to_string(),
        from: item.source.clone().unwrap_or_default(),
        copied: relative_time(lang, now, item.at),
    }
}

/// The delete confirmation's question: only a count, never clip text.
pub fn delete_question(n: usize, lang: Lang) -> String {
    match n {
        1 => t(lang, Msg::UiConfirmDeleteOne).to_string(),
        n => fill(t(lang, Msg::UiConfirmDeleteMany), n as u64),
    }
}

/// "128 clips" (or "200+ clips" while more pages wait) for the status line.
pub fn status_summary(n: usize, more: bool, lang: Lang) -> String {
    if n == 1 && !more {
        return t(lang, Msg::UiClipsOne).to_string();
    }
    let count = if more { format!("{n}+") } else { n.to_string() };
    t(lang, Msg::UiClipsMany).replace("{n}", &count)
}

/// The kit icon for a row's kind.
pub fn icon_name(k: KindIcon) -> IconName {
    match k {
        KindIcon::FileText => IconName::FileText,
        KindIcon::Link => IconName::Link,
        KindIcon::Code => IconName::Code,
        KindIcon::Image => IconName::Image,
        KindIcon::File => IconName::File,
        KindIcon::Type => IconName::Type,
    }
}

/// One popup row (Task 701): one preview line, source and age, the Alt+N number. A masked clip
/// shows only the fixed i18n text: the stored masked preview (the first characters of the secret)
/// is never drawn, searched for highlights or read aloud. The overline (`section`) is set by the
/// host, which sees the neighbouring rows.
pub fn popup_clip_row(item: &Item, index: usize, lang: Lang, query: &str) -> ClipRow {
    let text = popup_text(item, index, lang, crate::SystemClock.now());
    let line = if text.mono {
        MONO_LINE_CHARS
    } else {
        LINE_CHARS
    };
    let segs: Vec<Segment> = match popup_preview_text(item, lang) {
        Some(masked) => vec![Segment {
            text: masked.to_string(),
            hit: false,
        }],
        None => row_segments(&item.preview, query, line),
    };
    let label: String = segs.iter().map(|s| s.text.as_str()).collect();
    let segs = ModelRc::new(VecModel::from(
        segs.into_iter()
            .map(|s| Seg {
                text: s.text.into(),
                hit: s.hit,
            })
            .collect::<Vec<_>>(),
    ));
    ClipRow {
        segs,
        segs2: ModelRc::new(VecModel::<Seg>::default()),
        meta: text.meta.into(),
        label: label.into(),
        description: text.description.into(),
        icon: SharedString::new(),
        pin_badge: SharedString::new(),
        favorite: text.favorite,
        masked_badge: SharedString::new(),
        mono: text.mono,
        pinned: text.pinned,
        marked: false,
        kind_icon: icon_name(text.kind_icon),
        number: text.number.into(),
        section: 0,
        masked: text.masked,
    }
}

/// The pane with nothing in it: no text, picture or file names stay in a hidden window.
pub fn empty_detail() -> DetailState {
    DetailState {
        kind: 0,
        title: Default::default(),
        note: Default::default(),
        status: Default::default(),
        can_load_text: false,
        picture: Image::default(),
        has_picture: false,
        picture_info: Default::default(),
        can_load_image: false,
        files: ModelRc::new(VecModel::<FileRow>::default()),
        files_more: Default::default(),
        mono: false,
    }
}

/// The pane's Slint state from the shared properties (the text is set apart, see
/// `set_text_if_changed`).
fn detail_state(p: DetailProps<Px>) -> DetailState {
    let files: Vec<FileRow> = p
        .files
        .into_iter()
        .map(|f| FileRow {
            name: f.name.into(),
            path: f.path.into(),
        })
        .collect();
    let (picture, has_picture) = match p.picture {
        Some(buf) => (Image::from_rgba8(buf), true),
        None => (Image::default(), false),
    };
    DetailState {
        kind: p.kind,
        title: p.title.into(),
        note: p.note.into(),
        status: p.status.into(),
        can_load_text: p.can_load_text,
        picture,
        has_picture,
        picture_info: p.picture_info.into(),
        can_load_image: p.can_load_image,
        files: ModelRc::new(VecModel::from(files)),
        files_more: p.files_more.into(),
        mono: p.mono,
    }
}

/// The pane's state for `item` (open) from the model's current view.
pub fn render_state(
    item: &Item,
    detail: &mut DetailModel<Px>,
    lang: Lang,
) -> (DetailState, String) {
    let p = props(item, detail.view(), lang);
    let text = p.text.clone();
    (detail_state(p), text)
}

/// The new pane text when it differs from `current`: a large text is laid out again whenever
/// it is set, so an unchanged one is left alone.
pub fn set_text_if_changed(current: &SharedString, new: String) -> Option<SharedString> {
    (current.as_str() != new).then(|| new.into())
}

/// A decoded picture as a pixel buffer plus what it weighs.
fn pic_of(p: PixelsView) -> (Pic<Px>, usize) {
    let bytes = p.rgba.len();
    let buf = Px::clone_from_slice(&p.rgba, p.width, p.height);
    (
        Pic {
            pixels: buf,
            width: p.width,
            height: p.height,
        },
        bytes,
    )
}

/// The app's detail answer as the model wants it (worker thread).
fn detail_data(d: ClipDetailView) -> (DetailData<Px>, usize) {
    let mut bytes = 0;
    let (dims, thumb) = match d.image {
        Some(img) => {
            let dims = img.width.zip(img.height);
            let thumb = img.thumbnail.map(|t| {
                let (p, b) = pic_of(t);
                bytes = b;
                p
            });
            (dims, thumb)
        }
        None => (None, None),
    };
    (
        DetailData {
            masked: d.masked,
            dims,
            thumb,
            files: file_entries(&d.file_uris),
            file_total: d.file_total,
        },
        bytes,
    )
}

/// What a detail load found, keyed by clip id (no generation: stale ones are dropped by the
/// model).
pub enum Answer {
    Detail {
        id: ClipId,
        res: Result<DetailData<Px>, ()>,
        bytes: usize,
    },
    Text {
        id: ClipId,
        res: Result<(String, bool), ()>,
    },
    Image {
        id: ClipId,
        res: Result<Pic<Px>, ()>,
        bytes: usize,
    },
}

/// Feeds an answer to the model; true when the pane must be redrawn.
pub fn apply_answer(detail: &mut DetailModel<Px>, a: Answer) -> bool {
    match a {
        Answer::Detail { id, res, bytes } => detail.on_detail(id, res, bytes),
        Answer::Text { id, res } => detail.on_text(id, res),
        Answer::Image { id, res, bytes } => detail.on_full_image(id, res, bytes),
    }
}

/// Runs one detail load on the worker (decoding and downscaling happen there). `wanted` names
/// the clip the pane wants now: a job for any other clip is skipped before it does its work.
/// `deliver` runs on the worker; it must hop to the event loop itself.
pub fn run_detail(
    poster: &Poster,
    wanted: &Arc<Mutex<Option<ClipId>>>,
    r: DetailRequest,
    deliver: impl FnOnce(Answer) + Send + 'static,
) {
    let DetailRequest { id, want } = r;
    if let Ok(mut w) = wanted.lock() {
        *w = Some(id);
    }
    let wanted = Arc::clone(wanted);
    poster.post_quiet(move |app| {
        if !wanted.lock().is_ok_and(|w| *w == Some(id)) {
            return;
        }
        let answer = match want {
            Want::Detail => match app.ui_clip_detail(&Caller::Ui, id) {
                Ok(d) => {
                    let (data, bytes) = detail_data(d);
                    Answer::Detail {
                        id,
                        res: Ok(data),
                        bytes,
                    }
                }
                Err(_) => Answer::Detail {
                    id,
                    res: Err(()),
                    bytes: 0,
                },
            },
            Want::FullText => Answer::Text {
                id,
                res: app
                    .ui_clip_text(&Caller::Ui, id)
                    .map(|c| (c.text, c.truncated))
                    .map_err(|_| ()),
            },
            Want::FullImage => match app.ui_clip_image(&Caller::Ui, id, FULL_IMAGE_SIDE) {
                Ok(p) => {
                    let (pic, bytes) = pic_of(p);
                    Answer::Image {
                        id,
                        res: Ok(pic),
                        bytes,
                    }
                }
                Err(_) => Answer::Image {
                    id,
                    res: Err(()),
                    bytes: 0,
                },
            },
        };
        deliver(answer);
    });
}

/// Sets the strings both windows share (called with every status refresh).
pub fn set_shared_strings<'a, C: ComponentHandle>(w: &'a C, lang: Lang)
where
    Strings<'a>: slint::Global<'a, C>,
{
    let s = w.global::<Strings>();
    s.set_search_placeholder(t(lang, Msg::UiSearchPlaceholder).into());
    s.set_search_label(t(lang, Msg::UiSearchLabel).into());
    s.set_filters_label(t(lang, Msg::UiFiltersLabel).into());
    s.set_list_label(t(lang, Msg::UiClipListLabel).into());
    s.set_empty_history(t(lang, Msg::UiEmptyHistory).into());
    s.set_no_results(t(lang, Msg::UiNoResults).into());
    s.set_loading(t(lang, Msg::UiLoading).into());
    s.set_load_failed(t(lang, Msg::UiLoadFailed).into());
    s.set_copy(t(lang, Msg::UiCopy).into());
    s.set_copy_plain(t(lang, Msg::UiCopyPlain).into());
    s.set_footer_copy_plain(t(lang, Msg::UiFooterCopyPlain).into());
    s.set_footer_copy_as(t(lang, Msg::UiFooterCopyAs).into());
    s.set_pin(t(lang, Msg::UiPin).into());
    s.set_unpin(t(lang, Msg::UiUnpin).into());
    s.set_delete(t(lang, Msg::UiDelete).into());
    s.set_undo(t(lang, Msg::UiUndo).into());
    s.set_close(t(lang, Msg::UiClose).into());
    s.set_tip_close(t(lang, Msg::UiTipClose).into());
    s.set_tip_settings(t(lang, Msg::UiTipSettings).into());
    s.set_tip_favorite(t(lang, Msg::UiTipFavorite).into());
    s.set_key_esc(t(lang, Msg::UiKeyEsc).into());
    s.set_key_ctrl_d(t(lang, Msg::UiKeyCtrlD).into());
    s.set_detail_label(t(lang, Msg::UiDetailLabel).into());
    s.set_load_full_text(t(lang, Msg::UiLoadFullText).into());
    s.set_load_larger_image(t(lang, Msg::UiLoadLargerImage).into());
    s.set_no_image_preview(t(lang, Msg::UiNoImagePreview).into());
    s.set_masked_detail(t(lang, Msg::UiMaskedDetail).into());
    s.set_text_cut(fill(t(lang, Msg::UiTextCut), (TEXT_DISPLAY_CAP / 1024) as u64).into());
    s.set_copy_as(t(lang, Msg::TfCopyAs).into());
    s.set_tf_result(t(lang, Msg::TfPreviewLabel).into());
    s.set_tf_hint(t(lang, Msg::TfPickHint).into());
    s.set_edit(t(lang, Msg::TfEdit).into());
    s.set_edit_hint(t(lang, Msg::TfEditHint).into());
    s.set_save_new(t(lang, Msg::TfSaveNew).into());
}

/// The "Copy as..." panel's result area, from the worker's answer (Task 404).
pub struct PreviewView {
    pub text: SharedString,
    pub note: SharedString,
    pub is_error: bool,
    pub can_copy: bool,
}

/// What the panel shows while a preview is on its way (or after the panel closed).
pub fn empty_preview() -> PreviewView {
    PreviewView {
        text: SharedString::new(),
        note: SharedString::new(),
        is_error: false,
        can_copy: false,
    }
}

/// A transform that cannot run says why (static text, no content); a cut result says so.
pub fn preview_view(
    lang: Lang,
    r: &Result<darkclip_app::api::TransformPreview, darkclip_app::ApiError>,
) -> PreviewView {
    match r {
        Ok(p) => match p.error {
            Some(e) => PreviewView {
                text: SharedString::new(),
                note: t(lang, e.message()).into(),
                is_error: true,
                can_copy: false,
            },
            None if p.text.is_empty() => PreviewView {
                text: SharedString::new(),
                note: t(lang, Msg::TfEmptyResult).into(),
                is_error: false,
                can_copy: false,
            },
            None => PreviewView {
                text: p.text.as_str().into(),
                note: if p.truncated {
                    fill(t(lang, Msg::UiTextCut), (TEXT_DISPLAY_CAP / 1024) as u64).into()
                } else {
                    SharedString::new()
                },
                is_error: false,
                can_copy: true,
            },
        },
        Err(_) => PreviewView {
            text: SharedString::new(),
            note: t(lang, Msg::UiActionFailed).into(),
            is_error: true,
            can_copy: false,
        },
    }
}

/// The transforms' menu labels, in `Transform::ALL` order.
pub fn transform_labels(lang: Lang) -> ModelRc<SharedString> {
    ModelRc::new(VecModel::from(
        darkclip_core::transform::Transform::ALL
            .iter()
            .map(|tf| SharedString::from(t(lang, tf.label())))
            .collect::<Vec<_>>(),
    ))
}

#[cfg(test)]
mod preview_tests {
    use super::*;
    use darkclip_app::api::TransformPreview;

    fn ok(text: &str) -> Result<TransformPreview, darkclip_app::ApiError> {
        Ok(TransformPreview {
            text: text.to_string(),
            truncated: false,
            error: None,
        })
    }

    #[test]
    fn an_empty_result_has_a_note_and_cannot_be_copied() {
        let v = preview_view(Lang::En, &ok(""));
        assert!(!v.can_copy && !v.is_error);
        assert_eq!(v.note.as_str(), t(Lang::En, Msg::TfEmptyResult));
        let v = preview_view(Lang::En, &ok("x"));
        assert!(v.can_copy && v.note.is_empty());
    }
}

#[cfg(test)]
mod history_row_tests {
    use super::*;
    use darkclip_core::{ClipId, ClipType, Timestamp};
    use slint::Model;

    /// What storage returns as the preview of a masked clip: the first characters of the secret.
    const STORED: &str = "abc\u{2026}\u{2022}\u{2022}\u{2022}\u{2022} (Password)";

    fn item(preview: &str, masked: bool) -> Item {
        Item {
            id: ClipId::new_v7(Timestamp(1_795_320_000_000)),
            kind: ClipType::Text,
            preview: preview.to_string(),
            pinned: true,
            masked,
            source: Some("KeePass".into()),
            at: Timestamp(0),
            favorite: true,
            tags: vec!["work".into()],
        }
    }

    fn text_of(m: &ModelRc<Seg>) -> String {
        m.iter().map(|s| s.text.to_string()).collect()
    }

    #[test]
    fn a_masked_row_shows_only_the_fixed_text_in_both_languages() {
        for lang in [Lang::En, Lang::Vi] {
            // A search that matches the secret's first characters must not highlight anything.
            let r = history_clip_row(&item(STORED, true), 0, lang, "abc");
            let fixed = t(lang, Msg::UiMaskedRowText);
            assert_eq!(text_of(&r.segs), fixed);
            assert_eq!(text_of(&r.segs2), "");
            assert_eq!(r.label.as_str(), fixed);
            assert!(r.segs.iter().all(|s| !s.hit));
            assert!(r.masked);
            for (name, s) in [
                ("segs", text_of(&r.segs)),
                ("segs2", text_of(&r.segs2)),
                ("label", r.label.to_string()),
                ("description", r.description.to_string()),
                ("meta", r.meta.to_string()),
                ("icon", r.icon.to_string()),
                ("pin_badge", r.pin_badge.to_string()),
                ("masked_badge", r.masked_badge.to_string()),
            ] {
                for secret in ["abc", "\u{2022}", "Password", STORED] {
                    assert!(!s.contains(secret), "{lang:?} {name} has {secret:?}: {s}");
                }
            }
            // The row still says what it is for screen readers.
            assert!(r.description.contains(t(lang, Msg::UiMaskedBadge)));
        }
    }

    #[test]
    fn a_normal_row_keeps_its_preview_highlight_and_meta() {
        let r = history_clip_row(
            &item("select abc from t where abc > 1", false),
            0,
            Lang::En,
            "abc",
        );
        assert!(!r.masked);
        let first = text_of(&r.segs);
        assert!(first.starts_with("select abc"), "{first}");
        assert!(r.segs.iter().any(|s| s.hit && s.text.as_str() == "abc"));
        assert!(r.meta.starts_with("KeePass \u{b7} "));
        assert!(r.meta.ends_with(" \u{b7} work"));
        assert!(!r.meta.contains("Alt+"));
        assert!(r.pinned && r.favorite);
    }

    #[test]
    fn a_long_preview_goes_on_a_second_line() {
        let long = "word ".repeat(30);
        let r = history_clip_row(&item(long.trim(), false), 0, Lang::En, "");
        assert!(!text_of(&r.segs).is_empty());
        assert!(!text_of(&r.segs2).is_empty());
    }

    #[test]
    fn the_delete_question_never_holds_clip_text() {
        for lang in [Lang::En, Lang::Vi] {
            for n in [1, 2, 40] {
                let q = delete_question(n, lang);
                assert!(!q.is_empty());
                assert!(!q.contains(STORED) && !q.contains("abc"));
            }
        }
        assert_eq!(
            delete_question(1, Lang::En),
            t(Lang::En, Msg::UiConfirmDeleteOne)
        );
        assert_eq!(
            delete_question(3, Lang::En),
            "Delete 3 clips? This cannot be undone."
        );
    }

    #[test]
    fn a_masked_clips_detail_has_the_note_and_no_secret_characters() {
        for lang in [Lang::En, Lang::Vi] {
            let it = item(STORED, true);
            let mut detail = DetailModel::<Px>::default();
            detail.set_open(true, None);
            let _ = detail.sync(Some(&it));
            let (state, text) = render_state(&it, &mut detail, lang);
            assert_eq!(state.kind, 3);
            assert_eq!(state.note.as_str(), t(lang, Msg::UiMaskedDetail));
            assert!(text.is_empty());
            assert!(!state.can_load_text && !state.can_load_image);
            assert_eq!(state.files.row_count(), 0);
            for s in [
                state.title.to_string(),
                state.note.to_string(),
                state.status.to_string(),
                state.picture_info.to_string(),
                state.files_more.to_string(),
                text,
            ] {
                assert!(!s.contains("abc") && !s.contains('\u{2022}'), "{s}");
            }
        }
    }

    #[test]
    fn the_fact_lines_are_type_source_and_time_only() {
        let it = item(STORED, true);
        let f = history_facts(&it, Lang::En, Timestamp(300_000));
        assert_eq!(f.kind, "Text");
        assert_eq!(f.from, "KeePass");
        assert_eq!(f.copied, "5 min ago");
        let none = Item {
            source: None,
            ..item("x", false)
        };
        assert_eq!(history_facts(&none, Lang::En, Timestamp(0)).from, "");
        assert!(!format!("{f:?}").contains("KeePass"));
    }

    #[test]
    fn the_status_summary_counts_clips_in_both_languages() {
        assert_eq!(status_summary(1, false, Lang::En), "1 clip");
        assert_eq!(status_summary(128, false, Lang::En), "128 clips");
        assert_eq!(status_summary(200, true, Lang::En), "200+ clips");
        assert_eq!(status_summary(5, false, Lang::Vi), "5 m\u{1ee5}c");
        assert_eq!(status_summary(0, false, Lang::En), "0 clips");
    }
}
