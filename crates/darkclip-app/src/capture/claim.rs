//! The capture claim for translate in place (Task 603). The flow arms one slot for the app it
//! is about to send Ctrl+C to; the next change from that app is handed to the flow as text
//! and never stored, deduplicated, counted as a capture or announced as a new clip.
//!
//! Privacy order is unchanged: the settings gate lets a change through for the claim decision
//! only when it would drop it as Paused or Disabled (those stop storing; nothing is stored
//! here), never in Private mode and never for our own write. The claim is decided from the
//! snapshot's metadata (source, markers, format names), then `decide_translate_read` runs
//! with every privacy step, and only `Decision::Read` reads text/plain (GC-03). No logging
//! (GC-06): replies carry skip reasons only.

use super::actor::Actor;
use super::pending::ExcludedRecent;
use super::wipe::{wipe_bytes, wipe_reps};
use darkclip_core::markers::PrivacyMarker;
use darkclip_core::policy::{Decision, SkipReason};
use darkclip_core::{CaptureMode, ClipId, ClipboardSource, Mime, Platform, Redacted, Timestamp};
use darkclip_platform::PlatformError;
#[cfg(test)]
use std::sync::atomic::Ordering;
use std::sync::mpsc::Sender;
use std::sync::PoisonError;
use std::time::{Duration, Instant};

/// How long an armed slot waits for the target's change (covers the Ctrl+A, Ctrl+C retry).
pub const CLAIM_WINDOW_MS: u64 = 600;

/// Why a claim gave no text. Carries no content.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClaimError {
    /// A privacy or capture skip won (markers, transient, excluded app, Private, ...).
    /// `Skipped(Busy)` also covers a clipboard that could not be read, or that changed
    /// between the snapshot and the read.
    Skipped(SkipReason),
    /// The source app marked the content as not for the cloud (D-093): never sent.
    NoCloud,
    /// The change offers no plain text.
    NotText,
    /// The text is longer than the text size cap.
    TooLong,
    /// No change from the target app within the window.
    Expired,
    /// Another claim is armed.
    Busy,
}

pub(super) type ClaimReply = Sender<Result<Redacted<String>, ClaimError>>;

/// The one armed claim.
pub(super) struct ClaimSlot {
    app_id: String,
    armed_at: Instant,
    deadline: Instant,
    reply: ClaimReply,
}

impl ClaimSlot {
    /// A change whose first signal reached the actor at `at` may be claimed by this slot:
    /// after arming (a change already pending before is never claimed) and before the
    /// deadline.
    fn covers(&self, at: Instant) -> bool {
        self.armed_at <= at && at < self.deadline
    }
}

/// `app_id` match: case-insensitive for Windows exe names (as the excluded-app match),
/// exact elsewhere. An empty id never matches.
fn same_app(source: &ClipboardSource, app_id: &str) -> bool {
    if app_id.is_empty() {
        return false;
    }
    match source.platform {
        Platform::Windows => source.app_id.to_lowercase() == app_id.to_lowercase(),
        Platform::Macos | Platform::LinuxX11 | Platform::LinuxWayland => source.app_id == app_id,
    }
}

/// What the claim path needs from the change being processed.
pub(super) struct ClaimedChange<'a> {
    pub(super) seq: u64,
    pub(super) decision: Decision,
    /// The excluded sources, when the decision is `ExcludedApp` (F-14 rule 3).
    pub(super) excluded_now: Vec<ClipboardSource>,
    pub(super) markers: &'a [PrivacyMarker],
    pub(super) signal_at: Timestamp,
}

impl Actor {
    fn claim_window(&self) -> Duration {
        #[cfg(test)]
        {
            let ms = self.hooks.claim_window_ms.load(Ordering::SeqCst);
            if ms > 0 {
                return Duration::from_millis(ms);
            }
        }
        Duration::from_millis(CLAIM_WINDOW_MS)
    }

    /// `ExpectForeignCopy`: arms the slot, or replies `Busy` while another one is live.
    pub(super) fn arm_claim(&mut self, app_id: String, reply: ClaimReply) {
        let now = Instant::now();
        self.expire_claim_if_due(now);
        if self.claim.is_some() {
            let _ = reply.send(Err(ClaimError::Busy));
            return;
        }
        self.claim = Some(ClaimSlot {
            app_id,
            armed_at: now,
            deadline: now.checked_add(self.claim_window()).unwrap_or(now),
            reply,
        });
    }

    /// The slot cannot expire while a change it covers is still pending: that change is
    /// decided first (its debounce is bounded by the burst cap).
    fn claim_held(&self) -> bool {
        match (&self.claim, &self.pending) {
            (Some(c), Some(p)) => c.covers(p.seen_at),
            _ => false,
        }
    }

    /// When the run loop must wake for the slot's deadline (`None` while a covered change is
    /// pending: its own due time wakes the loop).
    pub(super) fn claim_wake(&self) -> Option<Instant> {
        if self.claim_held() {
            return None;
        }
        self.claim.as_ref().map(|c| c.deadline)
    }

    /// Replies `Expired` and disarms once the deadline has passed with nothing claimed.
    pub(super) fn expire_claim_if_due(&mut self, now: Instant) {
        let due = self.claim.as_ref().is_some_and(|c| now >= c.deadline) && !self.claim_held();
        if due {
            if let Some(c) = self.claim.take() {
                let _ = c.reply.send(Err(ClaimError::Expired));
            }
        }
    }

    /// The settings gate dropped a change for `reason`: may it pass for the claim decision?
    /// Only Paused or Disabled, never in Private mode, and only while a live slot covers a
    /// change first seen at `at`. Our own write never passes.
    pub(super) fn claim_lets_through(&self, reason: SkipReason, at: Instant) -> bool {
        matches!(reason, SkipReason::Paused | SkipReason::Disabled)
            && self.policy.mode != CaptureMode::Private
            && self.claim.as_ref().is_some_and(|c| c.covers(at))
    }

    /// The gate dropped a change while a slot is armed: in Private mode the slot fails now
    /// with `Skipped(Private)` (nothing is ever read in Private mode). Otherwise it stays.
    pub(super) fn claim_gated(&mut self) {
        if self.policy.mode == CaptureMode::Private {
            if let Some(c) = self.claim.take() {
                let _ = c.reply.send(Err(ClaimError::Skipped(SkipReason::Private)));
            }
        }
    }

    /// The change (its primary source, or for an ownerless change the F-14 rule 2 heir,
    /// first seen at `at`) belongs to the armed slot's app.
    pub(super) fn claims(&self, source: Option<&ClipboardSource>, at: Instant) -> bool {
        match (&self.claim, source) {
            (Some(c), Some(s)) => c.covers(at) && same_app(s, &c.app_id),
            _ => false,
        }
    }

    /// A claimed change: decided by `decide_translate_read` (taken by the caller), never
    /// stored. In every outcome the seq is processed and the slot disarms.
    pub(super) fn finish_claim(&mut self, change: ClaimedChange<'_>) {
        let Some(slot) = self.claim.take() else {
            return;
        };
        self.last_processed_seq = Some(change.seq);
        let r = self.claim_outcome(change);
        let _ = slot.reply.send(r);
    }

    fn claim_outcome(&mut self, change: ClaimedChange<'_>) -> Result<Redacted<String>, ClaimError> {
        let plan = match change.decision {
            Decision::Read(plan) => plan,
            Decision::Skip(SkipReason::NothingReadable) => {
                self.skip(SkipReason::NothingReadable);
                return Err(ClaimError::NotText);
            }
            Decision::Skip(reason) => {
                if reason == SkipReason::ExcludedApp {
                    self.excluded_recent = Some(ExcludedRecent {
                        seq: change.seq,
                        sources: change.excluded_now,
                        signal_at: change.signal_at,
                    });
                }
                self.skip(reason);
                return Err(ClaimError::Skipped(reason));
            }
        };
        // D-093: content its app keeps off the cloud is never sent; not even read.
        if change.markers.contains(&PrivacyMarker::NoCloud) {
            return Err(ClaimError::NoCloud);
        }
        // Policy said Read: text/plain may be read now (GC-03).
        let mut reps = match self.with_retries(|a| a.read(&plan)) {
            Ok(r) => r,
            Err(e) => {
                self.capture_failed(&e);
                return Err(match e {
                    PlatformError::TooLarge { .. } => ClaimError::TooLong,
                    _ => ClaimError::Skipped(SkipReason::Busy),
                });
            }
        };
        // As in capture: bytes read after the clipboard changed may belong to a change
        // nobody checked. Discarded; the newer change brings its own signal.
        if self.access.sequence().ok() != Some(change.seq) {
            wipe_reps(&mut reps);
            self.counters
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .count_changed_during_read();
            return Err(ClaimError::Skipped(SkipReason::Busy));
        }
        if reps.is_empty() {
            self.skip(SkipReason::NothingReadable);
            return Err(ClaimError::NotText);
        }
        let out = match reps.iter_mut().find(|r| r.mime == Mime::TextPlain) {
            None => Err(ClaimError::NotText),
            Some(r) if r.truncated => Err(ClaimError::TooLong),
            Some(r) if r.bytes.is_empty() => Err(ClaimError::NotText),
            Some(r) => match String::from_utf8(std::mem::take(&mut r.bytes)) {
                Ok(text) => Ok(Redacted(text)),
                Err(e) => {
                    let mut bytes = e.into_bytes();
                    wipe_bytes(&mut bytes);
                    Err(ClaimError::NotText)
                }
            },
        };
        wipe_reps(&mut reps);
        out
    }

    /// `RestoreCandidate`: the clip to copy back after the flow, only if the clipboard still
    /// holds the last processed change and that change was stored (inserted or deduplicated).
    pub(super) fn restore_candidate(&self) -> Option<ClipId> {
        let now = self.access.sequence().ok()?;
        match self.last_stored {
            Some((seq, id)) if seq == now && self.last_processed_seq == Some(seq) => Some(id),
            _ => None,
        }
    }
}
