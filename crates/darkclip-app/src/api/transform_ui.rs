//! UI-only text transforms and "save edited copy" (Task 404).

use super::*;
use darkclip_core::transform::{Transform, TransformError, MAX_INPUT_BYTES};
use darkclip_core::{ClipType, Redacted};

/// The output of a transform shown before Copy, cut like the other UI text (D-051).
#[derive(Clone, PartialEq, Eq)]
pub struct TransformPreview {
    pub text: String,
    pub truncated: bool,
    pub error: Option<TransformError>,
}

/// Never prints the text (GC-06).
impl std::fmt::Debug for TransformPreview {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TransformPreview")
            .field("text", &"<redacted>")
            .field("truncated", &self.truncated)
            .field("error", &self.error)
            .finish()
    }
}

/// Largest text the editor loads.
pub const EDIT_MAX_BYTES: usize = 256 * 1024;

impl App {
    /// The clip's whole plain text (up to `cap` bytes; `None` when longer) for a transform
    /// or the editor. Images, files, masked, expired and missing clips answer `NotFound`.
    fn text_for_transform(
        &self,
        c: &Caller,
        id: ClipId,
        cap: usize,
    ) -> Result<Option<String>, ApiError> {
        ui_only(c)?;
        let st = self.reader();
        let clip = self.visible_clip_in(&st, c, &id)?;
        if matches!(clip.kind, ClipType::Image | ClipType::Files) {
            return Err(ApiError::NotFound);
        }
        let content = self
            .read_text(&st, c, &clip, cap.saturating_add(1))?
            .ok_or(ApiError::NotFound)?;
        Ok((!content.truncated).then_some(content.text))
    }

    /// The output of `t` on the clip's plain text, for the preview before Copy. UI only. A
    /// transform that cannot run (invalid JSON, too large, ...) is a normal answer with
    /// `error` set; a masked, missing or non-text clip is `NotFound`.
    pub fn ui_transform_preview(
        &self,
        c: &Caller,
        id: ClipId,
        t: Transform,
    ) -> Result<TransformPreview, ApiError> {
        let Some(text) = self.text_for_transform(c, id, MAX_INPUT_BYTES)? else {
            return Ok(TransformPreview {
                text: String::new(),
                truncated: false,
                error: Some(TransformError::TooLarge),
            });
        };
        Ok(match t.apply(&text) {
            Ok(out) => {
                let mut end = out.len().min(UI_TEXT_MAX_BYTES);
                while !out.is_char_boundary(end) {
                    end -= 1;
                }
                TransformPreview {
                    truncated: end < out.len(),
                    text: out[..end].to_string(),
                    error: None,
                }
            }
            Err(e) => TransformPreview {
                text: String::new(),
                truncated: false,
                error: Some(e),
            },
        })
    }

    /// Writes the output of `t` to the clipboard through the actor (a self-write: never
    /// captured again) and stores nothing.
    pub fn ui_copy_transformed(
        &self,
        c: &Caller,
        id: ClipId,
        t: Transform,
    ) -> Result<(), ApiError> {
        let text = self
            .text_for_transform(c, id, MAX_INPUT_BYTES)?
            .ok_or(ApiError::InvalidArgument("transform failed"))?;
        let out = t
            .apply(&text)
            .map_err(|_| ApiError::InvalidArgument("transform failed"))?;
        self.ask(|reply, deadline| ActorCommand::CopyText {
            text: Redacted(out),
            deadline,
            reply,
        })
    }

    /// The text the editor starts from: the whole plain text, at most `EDIT_MAX_BYTES`.
    pub fn ui_edit_text(&self, c: &Caller, id: ClipId) -> Result<String, ApiError> {
        self.text_for_transform(c, id, EDIT_MAX_BYTES)?
            .ok_or(ApiError::InvalidArgument("too large to edit"))
    }

    /// Saves edited text as a new clip. It meets capture's rules (classify, kind check,
    /// sensitive handling, size limit); the original clip is never touched.
    pub fn ui_save_edited(&self, c: &Caller, text: String) -> Result<SaveOutcome, ApiError> {
        ui_only(c)?;
        self.ask(|reply, deadline| ActorCommand::SaveText {
            text: Redacted(text),
            deadline,
            reply,
        })
    }
}
