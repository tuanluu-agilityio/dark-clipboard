//! UI-only text transforms and "save edited copy" (Task 404).

use super::*;
use darkclip_core::transform::{Transform, TransformError};

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

impl App {
    pub fn ui_transform_preview(
        &self,
        _c: &Caller,
        _id: ClipId,
        _t: Transform,
    ) -> Result<TransformPreview, ApiError> {
        Err(ApiError::Internal)
    }

    pub fn ui_copy_transformed(
        &self,
        _c: &Caller,
        _id: ClipId,
        _t: Transform,
    ) -> Result<(), ApiError> {
        Err(ApiError::Internal)
    }

    pub fn ui_edit_text(&self, _c: &Caller, _id: ClipId) -> Result<String, ApiError> {
        Err(ApiError::Internal)
    }

    pub fn ui_save_edited(&self, _c: &Caller, _text: String) -> Result<SaveOutcome, ApiError> {
        Err(ApiError::Internal)
    }
}
