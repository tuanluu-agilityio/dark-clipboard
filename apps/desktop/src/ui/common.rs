//! Pieces the popup host and the history host share (Task 211): rows, the detail pane's state,
//! the worker side of detail loads and the strings both windows show. No business logic lives
//! here: the decisions are in `crate::popup` (pure view-models). Nothing is logged; previews,
//! queries and paths are never formatted into messages (GC-06, SR-11).

use super::{ClipRow, DetailState, FileRow, Poster, Seg, Strings};
use crate::popup::detail::{
    file_entries, props, DetailData, DetailModel, DetailProps, DetailRequest, Pic, Want,
    FULL_IMAGE_SIDE, TEXT_DISPLAY_CAP,
};
use crate::popup::highlight::{row_segments, Segment};
use crate::popup::list::Item;
use crate::popup::row::{fill, row_text, split_two_lines, LINE_CHARS, MONO_LINE_CHARS};
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

/// One list row (previews only; the search text is highlighted).
pub fn clip_row(item: &Item, index: usize, lang: Lang, query: &str) -> ClipRow {
    let text = row_text(item, index, lang, crate::SystemClock.now());
    let line = if text.mono {
        MONO_LINE_CHARS
    } else {
        LINE_CHARS
    };
    let all = row_segments(&item.preview, query, line * 2);
    let label: String = all.iter().map(|s| s.text.as_str()).collect();
    let (first, second) = split_two_lines(all, line);
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
        icon: text.icon.into(),
        pin_badge: text.pin_badge.unwrap_or("").into(),
        favorite: text.favorite,
        masked_badge: text.masked_badge.unwrap_or("").into(),
        mono: text.mono,
        pinned: item.pinned,
        marked: false,
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
