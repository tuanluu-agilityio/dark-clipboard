//! The capture claim for translate in place (Task 603). The flow arms one slot for the app it
//! is about to send Ctrl+C to; the next change from that app is handed to the flow as text
//! and never stored, deduplicated, counted as a capture or announced as a new clip.
//!
//! Privacy order is unchanged: the settings gate lets a change through for the claim decision
//! only when it would drop it as Paused or Disabled (those stop storing; nothing is stored
//! here) and its signal names the target app, or it is an ownerless change following the
//! target's (its possible OLE flush); never in Private mode and never for our own write.
//! Every other change is dropped at step 1 as without a claim (privacy-design section 5). A
//! change let through and not claimed is dropped, whatever the gate says later. The claim is
//! decided from the snapshot's metadata (source, markers, format names), then
//! `decide_translate_read` runs with every privacy step, and only `Decision::Read` reads
//! text/plain (GC-03). A late ownerless flush of a claimed change is skipped too
//! (`recent_claim_flush`). No logging (GC-06): replies carry skip reasons only.

use super::actor::Actor;
use super::pending::{has_ole_signature, within_ms_or_earlier, ExcludedRecent, MAX_HEIR_SEQ_GAP};
use super::wipe::{wipe_bytes, wipe_reps, wipe_string};
use darkclip_core::markers::PrivacyMarker;
use darkclip_core::policy::{Decision, SkipReason};
use darkclip_core::{CaptureMode, ClipId, ClipboardSource, Mime, Platform, Redacted, Timestamp};
use darkclip_platform::{ChangeSignal, ClipboardMeta, PlatformError};
#[cfg(test)]
use std::sync::atomic::Ordering;
use std::sync::mpsc::Sender;
use std::sync::PoisonError;
use std::time::{Duration, Instant};

/// How long an armed slot waits for the target's change (covers the Ctrl+A, Ctrl+C retry
/// and a slow app; Task 603b, I1 R1).
pub const CLAIM_WINDOW_MS: u64 = 1_000;

/// The flow's receive timeout for a claim reply is `CLAIM_WINDOW_MS` plus this: a change seen
/// just before the deadline still waits for its burst cap (500 ms) and the read retries.
pub const CLAIM_REPLY_SLACK_MS: u64 = 800;

/// The text of a claimed change and the seq it was read at (the snapshot's seq), so the flow
/// can write only over that exact change (`CopyTextIf`). `Debug` prints the seq only.
pub struct ClaimedText {
    pub text: Redacted<String>,
    pub seq: u64,
}

impl std::fmt::Debug for ClaimedText {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ClaimedText")
            .field("seq", &self.seq)
            .finish()
    }
}

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

pub(super) type ClaimReply = Sender<Result<ClaimedText, ClaimError>>;

/// The one armed claim.
pub(super) struct ClaimSlot {
    app_id: String,
    armed_at: Instant,
    deadline: Instant,
    reply: ClaimReply,
}

impl ClaimSlot {
    pub(super) fn new(
        app_id: String,
        armed_at: Instant,
        window: Duration,
        reply: ClaimReply,
    ) -> Self {
        Self {
            app_id,
            armed_at,
            deadline: armed_at.checked_add(window).unwrap_or(armed_at),
            reply,
        }
    }

    /// A change whose first signal reached the actor at `at` may be claimed by this slot:
    /// after arming (a change already pending before is never claimed) and before the
    /// deadline.
    pub(super) fn covers(&self, at: Instant) -> bool {
        self.armed_at <= at && at < self.deadline
    }
}

/// The last claimed change, for its late ownerless OLE flush and the same app's follow-up
/// copy (skip only, never stored).
pub(super) struct ClaimedRecent {
    seq: u64,
    signal_at: Timestamp,
    app_id: String,
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
        self.claim = Some(ClaimSlot::new(app_id, now, self.claim_window(), reply));
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

    /// `claim_lets_through` for a signal: only a change that can be the target's gets past
    /// step 1. Its signal names the target app; or it is ownerless and the pending change it
    /// follows is the target's (the only way the F-14 heir can name the target); or it is one
    /// more signal of a change already let through.
    pub(super) fn claim_admits(&self, reason: SkipReason, now: Instant, s: &ChangeSignal) -> bool {
        if !self.claim_lets_through(reason, now) {
            return false;
        }
        let Some(c) = &self.claim else {
            return false;
        };
        let pending = self.pending.as_ref();
        if pending.is_some_and(|p| p.seq == s.seq && p.held.is_some()) {
            return true;
        }
        match &s.source {
            Some(src) => same_app(src, &c.app_id),
            None => pending
                .and_then(|p| p.sources.first())
                .is_some_and(|src| same_app(src, &c.app_id)),
        }
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
        self.claimed_recent = Some(ClaimedRecent {
            seq: change.seq,
            signal_at: change.signal_at,
            app_id: slot.app_id.clone(),
        });
        let seq = change.seq;
        let r = self
            .claim_outcome(change)
            .map(|text| ClaimedText { text, seq });
        if let Err(e) = slot.reply.send(r) {
            // The requester is gone: the text goes nowhere and is wiped.
            if let Ok(mut claimed) = e.0 {
                wipe_string(&mut claimed.text.0);
            }
        }
    }

    /// Task 603b (I1 M4): a change whose primary source is the app of the last claimed
    /// change, 1..=`MAX_HEIR_SEQ_GAP` seqs after it, at most `max_burst_wait` plus
    /// `CLAIM_WINDOW_MS` after its first signal (signal time), with no self-write in between:
    /// the select-all retry's second copy of the same source text. Skip only; never stored.
    pub(super) fn recent_claim_follow_up(
        &self,
        source: Option<&ClipboardSource>,
        seq: u64,
        signal_at: Timestamp,
    ) -> bool {
        let (Some(r), Some(source)) = (&self.claimed_recent, source) else {
            return false;
        };
        let cap_ms = u64::try_from(self.max_burst_wait.as_millis())
            .unwrap_or(u64::MAX)
            .saturating_add(CLAIM_WINDOW_MS);
        same_app(source, &r.app_id)
            && seq
                .checked_sub(r.seq)
                .is_some_and(|gap| (1..=MAX_HEIR_SEQ_GAP).contains(&gap))
            && !self
                .last_self_write_seq
                .is_some_and(|w| w > r.seq && w <= seq)
            && within_ms_or_earlier(r.signal_at, signal_at, cap_ms)
    }

    /// An ownerless change with the OLE signature 1..=`MAX_HEIR_SEQ_GAP` seqs after the last
    /// claimed change, at most `max_burst_wait` after it by signal time, with no self-write
    /// in between: the claimed copy's late flush (Notepad shape). Skip only; the caller marks
    /// it processed and never stores it.
    pub(super) fn recent_claim_flush(
        &self,
        seq: u64,
        signal_at: Timestamp,
        meta: &ClipboardMeta,
    ) -> bool {
        let Some(r) = &self.claimed_recent else {
            return false;
        };
        let cap_ms = u64::try_from(self.max_burst_wait.as_millis()).unwrap_or(u64::MAX);
        seq.checked_sub(r.seq)
            .is_some_and(|gap| (1..=MAX_HEIR_SEQ_GAP).contains(&gap))
            && !self
                .last_self_write_seq
                .is_some_and(|w| w > r.seq && w <= seq)
            && within_ms_or_earlier(r.signal_at, signal_at, cap_ms)
            && has_ole_signature(meta)
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
