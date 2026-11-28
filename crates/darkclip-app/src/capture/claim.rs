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
//!
//! Task 610 (Q-08): a slot that ends without a claim (expired, replaced by a newer arm, or
//! failed in Private mode) leaves an unclaimed tail for its app (at most `MAX_TAILS`, one per
//! app). The target's copy arriving after the deadline (a slow app answering the synthetic
//! Ctrl+C late), and its ownerless flush, are skipped as `TranslateSource` and never stored,
//! for `UNCLAIMED_TAIL_MS` after the deadline and until our own next write. Decided from
//! metadata only, like the claim.

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
/// 3000 since Task 610: Chrome answers the synthetic Ctrl+C later than 1 s (Q-08). A claimed
/// change still replies as soon as it is read.
pub const CLAIM_WINDOW_MS: u64 = 3_000;

/// How long after an unclaimed slot's deadline (signal time) the target's late copy is
/// still skipped (Task 610).
pub const UNCLAIMED_TAIL_MS: u64 = 5_000;

/// The most unclaimed tails kept (one per app); the oldest is dropped.
const MAX_TAILS: usize = 4;

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
    /// The claim could not be armed (the host's answer when the app does not reply). The
    /// actor no longer replies it: a newer arm replaces a live slot (Task 610).
    Busy,
}

pub(super) type ClaimReply = Sender<Result<ClaimedText, ClaimError>>;

/// The one armed claim.
pub(super) struct ClaimSlot {
    app_id: String,
    armed_at: Instant,
    deadline: Instant,
    /// The actor clock at arming (signal-time domain) and the window in ms, for the tail.
    armed_signal_at: Timestamp,
    window_ms: u64,
    /// `last_self_write_seq` at arming: a later own write ends the tail.
    self_write_at_arm: Option<u64>,
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
            armed_signal_at: Timestamp(0),
            window_ms: u64::try_from(window.as_millis()).unwrap_or(u64::MAX),
            self_write_at_arm: None,
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

/// Task 610: what is left of a slot that ended without a claim. The target app's change
/// first seen after arming, at most `window + UNCLAIMED_TAIL_MS` after arming by signal time
/// (and by actor time, so a wall-clock jump cannot stretch it), with no own write since
/// arming, is skipped and never stored.
pub(super) struct UnclaimedTail {
    app_id: String,
    armed_at: Instant,
    armed_signal_at: Timestamp,
    /// Signal-time bound: ms after `armed_signal_at`.
    span_ms: u64,
    /// Actor-time bound on the change's first-seen time.
    until: Instant,
    self_write_at_arm: Option<u64>,
    /// The last change this tail skipped (seq, signal time), for its late ownerless flush.
    skipped: Option<(u64, Timestamp)>,
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

    fn tail_ms(&self) -> u64 {
        #[cfg(test)]
        {
            let ms = self.hooks.unclaimed_tail_ms.load(Ordering::SeqCst);
            if ms > 0 {
                return ms;
            }
        }
        UNCLAIMED_TAIL_MS
    }

    /// A slot ended without a claim at `now`: its app's tail starts (Task 610). An earlier
    /// tail of the same app with no own write since is merged into it (keeping the earlier
    /// start, so that press's late copy is still covered). Past `MAX_TAILS` the oldest goes.
    fn start_tail(&mut self, c: &ClaimSlot, now: Instant) {
        let tail_ms = self.tail_ms();
        let mut tail = UnclaimedTail {
            app_id: c.app_id.clone(),
            armed_at: c.armed_at,
            armed_signal_at: c.armed_signal_at,
            span_ms: c.window_ms.saturating_add(tail_ms),
            until: now
                .max(c.deadline)
                .checked_add(Duration::from_millis(tail_ms))
                .unwrap_or(now),
            self_write_at_arm: c.self_write_at_arm,
            skipped: None,
        };
        let same = self
            .unclaimed_tails
            .iter()
            .position(|t| t.app_id.to_lowercase() == c.app_id.to_lowercase());
        if let Some(i) = same {
            let old = self.unclaimed_tails.remove(i);
            if old.self_write_at_arm == tail.self_write_at_arm && old.armed_at <= tail.armed_at {
                let ahead = tail.armed_signal_at.0.saturating_sub(old.armed_signal_at.0);
                let ahead = u64::try_from(ahead.max(0)).unwrap_or(0);
                tail.span_ms = tail.span_ms.saturating_add(ahead).max(old.span_ms);
                tail.armed_at = old.armed_at;
                tail.armed_signal_at = old.armed_signal_at;
                tail.until = tail.until.max(old.until);
                tail.skipped = old.skipped;
            }
        }
        if self.unclaimed_tails.len() >= MAX_TAILS {
            self.unclaimed_tails.remove(0);
        }
        self.unclaimed_tails.push(tail);
    }

    /// `ExpectForeignCopy`: arms the slot. A live slot is replaced: the host runs one press
    /// at a time, so it belongs to a finished flow (Esc, or a reply it gave up on). Its
    /// requester gets `Expired` and its app's tail starts, so its late copy is never stored
    /// and there are never two live claims (Task 610 review).
    pub(super) fn arm_claim(&mut self, app_id: String, reply: ClaimReply) {
        let now = Instant::now();
        self.expire_claim_if_due(now);
        if let Some(old) = self.claim.take() {
            self.start_tail(&old, now);
            let _ = old.reply.send(Err(ClaimError::Expired));
        }
        let mut slot = ClaimSlot::new(app_id, now, self.claim_window(), reply);
        slot.armed_signal_at = self.clock.now();
        slot.self_write_at_arm = self.last_self_write_seq;
        self.claim = Some(slot);
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
                self.start_tail(&c, now);
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
                self.start_tail(&c, Instant::now());
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

    /// Task 610: the change (its primary source, or for an ownerless change the F-14 rule 2
    /// heir), first seen at `seen_at` and first signalled at `signal_at`, falls in the
    /// unclaimed tail: the target app's late copy, or the ownerless OLE flush of a change the
    /// tail skipped (as `recent_claim_flush`). Decided from metadata only. Skip only: the
    /// caller marks it processed and never stores it. Asked only for a change that is not
    /// claimed, so a live slot always wins.
    pub(super) fn unclaimed_tail_skips(
        &mut self,
        source: Option<&ClipboardSource>,
        seq: u64,
        seen_at: Instant,
        signal_at: Timestamp,
        meta: &ClipboardMeta,
    ) -> bool {
        if self.unclaimed_tails.is_empty() {
            return false;
        }
        // A tail ends after an own write. Time bounds the skip itself (`seen_at <= until`), so
        // a change first seen inside the tail is skipped however late it is processed;
        // `MAX_TAILS` bounds the memory.
        let self_write = self.last_self_write_seq;
        self.unclaimed_tails
            .retain(|t| self_write == t.self_write_at_arm);
        let cap_ms = u64::try_from(self.max_burst_wait.as_millis()).unwrap_or(u64::MAX);
        let hit = self.unclaimed_tails.iter().position(|t| match source {
            Some(s) => {
                seen_at >= t.armed_at
                    && seen_at <= t.until
                    && within_ms_or_earlier(t.armed_signal_at, signal_at, t.span_ms)
                    && same_app(s, &t.app_id)
            }
            None => {
                t.skipped.is_some_and(|(s, at)| {
                    seq.checked_sub(s)
                        .is_some_and(|gap| (1..=MAX_HEIR_SEQ_GAP).contains(&gap))
                        && within_ms_or_earlier(at, signal_at, cap_ms)
                }) && has_ole_signature(meta)
            }
        });
        match hit.and_then(|i| self.unclaimed_tails.get_mut(i)) {
            Some(t) => {
                t.skipped = Some((seq, signal_at));
                true
            }
            None => false,
        }
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
