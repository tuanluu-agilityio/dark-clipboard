//! Translate in place (Task 605): thin UI-only wrappers over the capture claim commands
//! (Tasks 603, 603b). The flow in `apps/desktop/src/translate/` calls these and nothing else;
//! the desktop never reads or writes the clipboard itself (GC-03). Nothing here logs, and the
//! text passed through is only ever wrapped in `Redacted` (GC-06).

use super::*;
use crate::capture::{ClaimError, ClaimedText};
use darkclip_core::{Platform, Redacted};
use std::sync::mpsc::Receiver;

/// The platform whose `app_id` form the translate flow passes (the target window's process,
/// resolved like `ClipboardSource.app_id` on this platform).
pub(super) fn translate_platform() -> Platform {
    if cfg!(windows) {
        Platform::Windows
    } else if cfg!(target_os = "macos") {
        Platform::Macos
    } else {
        Platform::LinuxX11
    }
}

impl App {
    /// Arms the capture claim for `app_id` (`ExpectForeignCopy`) and returns at once; the
    /// caller waits on the receiver for at most `CLAIM_WINDOW_MS + CLAIM_REPLY_SLACK_MS`.
    /// One slot: a second arm replaces a live one, which replies `Expired` (Task 610). UI only.
    pub fn ui_translate_claim(
        &self,
        c: &Caller,
        app_id: String,
    ) -> Result<Receiver<Result<ClaimedText, ClaimError>>, ApiError> {
        ui_only(c)?;
        let (reply, rx) = mpsc::channel();
        self.cap
            .send(ActorCommand::ExpectForeignCopy { app_id, reply })?;
        Ok(rx)
    }

    /// The stored clip the clipboard holds now, if any (`RestoreCandidate`). UI only.
    pub fn ui_restore_candidate(&self, c: &Caller) -> Result<Option<ClipId>, ApiError> {
        ui_only(c)?;
        let (reply, rx) = mpsc::channel();
        self.cap.send(ActorCommand::RestoreCandidate { reply })?;
        rx.recv_timeout(ACTOR_REPLY_TIMEOUT)
            .map_err(|_| ApiError::Internal)
    }

    /// Writes `text` as our own write only while the clipboard still holds `expect_seq`
    /// (`CopyTextIf`): the own write seq, or `None` when the clipboard moved (nothing
    /// written). The text is wiped on every path, including a refused caller. UI only.
    pub fn ui_copy_text_if(
        &self,
        c: &Caller,
        expect_seq: u64,
        text: Redacted<String>,
    ) -> Result<Option<u64>, ApiError> {
        let mut text = text;
        if let Err(e) = ui_only(c) {
            darkclip_core::sensitive::wipe_string(&mut text.0);
            return Err(e);
        }
        self.ask(|reply, deadline| ActorCommand::CopyTextIf {
            expect_seq,
            text,
            deadline,
            reply,
        })
    }

    /// Copies clip `id` back only while the clipboard still holds `expect_seq` (`RestoreIf`);
    /// `false` when it moved. Not a use of the clip. UI only.
    pub fn ui_restore_if(&self, c: &Caller, expect_seq: u64, id: ClipId) -> Result<bool, ApiError> {
        ui_only(c)?;
        self.ask(|reply, deadline| ActorCommand::RestoreIf {
            expect_seq,
            id,
            deadline,
            reply,
        })
    }

    /// The clipboard's current sequence number (not content). UI only.
    pub fn ui_clipboard_seq(&self, c: &Caller) -> Result<u64, ApiError> {
        ui_only(c)?;
        self.ask(|reply, deadline| ActorCommand::ClipboardSeq { deadline, reply })
    }

    /// True when `app_id` is in the current excluded-app list (built-ins included), with the
    /// same matching as the capture policy (`darkclip_core::policy::app_is_excluded`), so the
    /// translate flow and capture never disagree.
    pub fn is_excluded_app(&self, app_id: &str) -> bool {
        let settings = lock(&self.settings);
        darkclip_core::policy::app_is_excluded(
            app_id,
            translate_platform(),
            &settings.privacy.excluded_apps,
        )
    }
}
