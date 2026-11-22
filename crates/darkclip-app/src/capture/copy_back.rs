//! Copy-back: writes a stored clip to the clipboard and records the write as our own, so it
//! is never captured again (rule 5).

use super::actor::{map_platform, Actor};
use super::wipe::wipe_reps;
use crate::events::{map_storage, ApiError};
use darkclip_core::{ClipId, Mime, RawRepresentation};

impl Actor {
    pub(super) fn copy_back(&mut self, id: &ClipId, plain: bool) -> Result<(), ApiError> {
        let clip = self
            .storage
            .get_clip(id)
            .map_err(|e| map_storage(&e))?
            .ok_or(ApiError::NotFound)?;
        if clip.expires_at.is_some_and(|t| t <= self.clock.now()) {
            return Err(ApiError::NotFound);
        }
        let mut reps = Vec::new();
        for (ord, rep) in clip.representations.iter().enumerate() {
            if plain && rep.mime != Mime::TextPlain {
                continue;
            }
            let ord = u32::try_from(ord).map_err(|_| ApiError::Internal)?;
            let bytes = self
                .storage
                .read_representation(id, ord)
                .map_err(|e| map_storage(&e))?;
            reps.push(RawRepresentation {
                mime: rep.mime,
                platform_format: rep.platform_format.clone().unwrap_or_default(),
                bytes,
                width: rep.width,
                height: rep.height,
                truncated: false,
            });
        }
        if reps.is_empty() {
            return Err(ApiError::InvalidArgument("clip has no plain text"));
        }
        let written = self.with_retries(|a| a.write(&reps));
        wipe_reps(&mut reps);
        if written.is_err() {
            // Whatever the adapter kept for the retries (converted blocks) goes now: nothing
            // of a failed copy-back may wait in memory for a later clipboard event.
            self.access.discard_prepared();
        }
        let seq = written.map_err(|e| map_platform(&e))?;
        // Rule 5: exactly this change is ours and must not be captured again.
        self.last_self_write_seq = Some(seq);
        // Rule 3: nothing after our own write is a flush of the excluded change.
        self.excluded_recent = None;
        self.storage.mark_used(id).map_err(|e| map_storage(&e))
    }
}
