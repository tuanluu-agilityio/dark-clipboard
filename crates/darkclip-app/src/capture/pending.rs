//! Debounce and burst handling: the pending change, the supersede chain (F-14 rule 1), the
//! heir of an ownerless OLE flush (rule 2), the carried seq (delayed rendering) and the last
//! excluded-app skip (rule 3).

use super::actor::Actor;
use darkclip_core::policy::SkipReason;
use darkclip_core::{ClipboardSource, Timestamp};
use darkclip_platform::{ChangeSignal, ClipboardMeta};
#[cfg(test)]
use std::sync::PoisonError;
use std::time::{Duration, Instant};

/// Longest wait from the first signal of a burst, so a noisy writer cannot starve capture.
pub(super) const MAX_BURST_WAIT: Duration = Duration::from_millis(500);

/// A burst of change signals that is waiting for its quiet period.
pub(super) struct Pending {
    pub(super) seq: u64,
    /// Every distinct source the signals for `seq` carried, in order; the first is the
    /// primary (the owner may be gone by the last signal).
    pub(super) sources: Vec<ClipboardSource>,
    /// First signal of the burst, for the `max_burst_wait` cap.
    pub(super) first_at: Instant,
    pub(super) due: Instant,
    /// Signal time (`ChangeSignal::at`) of the burst's first signal.
    pub(super) burst_signal_at: Timestamp,
    /// Signal time of this change's first signal.
    pub(super) signal_at: Timestamp,
    /// Signal time of this change's latest signal.
    pub(super) last_signal_at: Timestamp,
    /// When the actor dequeued this change's first signal (Task 603: a claim covers only
    /// changes first seen while it is armed).
    pub(super) seen_at: Instant,
    /// Task 603: the gate reason this change was let through for, only for a claim decision.
    /// If it is not claimed it is dropped with this reason, whatever the gate says later.
    pub(super) held: Option<SkipReason>,
    /// F-14 rule 1: every source of the changes this burst superseded before they were
    /// inspected. Checked against the excluded apps when this change has no source of its own.
    pub(super) superseded: Vec<ClipboardSource>,
    /// F-14 rule 2: the latest superseded change that had a source; the only candidate for
    /// the stored source of an ownerless OLE flush.
    pub(super) heir: Option<Superseded>,
}

/// A change replaced by a newer seq before it was inspected.
pub(super) struct Superseded {
    pub(super) seq: u64,
    /// Its primary source.
    pub(super) source: ClipboardSource,
    pub(super) last_signal_at: Timestamp,
    /// A snapshot of this seq completed earlier (delayed rendering, `carried`).
    pub(super) inspected: bool,
}

/// A seq that had nothing readable yet (OLE delayed rendering): its primary source and every
/// other source it was checked against, so a retry of the same seq checks them again.
pub(super) struct Carried {
    pub(super) seq: u64,
    pub(super) primary: Option<ClipboardSource>,
    pub(super) others: Vec<ClipboardSource>,
}

/// F-14 rule 3 (D-042): the excluded sources of the last change skipped as `ExcludedApp`.
/// A later ownerless change close to it is checked against them (skip only, never stored).
pub(super) struct ExcludedRecent {
    pub(super) seq: u64,
    pub(super) sources: Vec<ClipboardSource>,
    pub(super) signal_at: Timestamp,
}

/// Largest seq distance between a superseded change and the ownerless flush that may inherit
/// its source (F-14 rule 2), and between an excluded skip and a later ownerless change
/// checked against it (rule 3).
pub(super) const MAX_HEIR_SEQ_GAP: u64 = 8;

/// Windows OLE placeholders; their presence in the final snapshot is the flush signature.
const OLE_SIGNATURE_FORMATS: [&str; 2] = ["DataObject", "Ole Private Data"];

/// The snapshot still offers an OLE placeholder (reported with no MIME).
pub(super) fn has_ole_signature(meta: &ClipboardMeta) -> bool {
    meta.formats.iter().any(|f| {
        f.mime.is_none()
            && OLE_SIGNATURE_FORMATS
                .iter()
                .any(|n| f.name.eq_ignore_ascii_case(n))
    })
}

/// Skip-only checks: like `within_ms`, but a `later` before `earlier` (wall clock stepped
/// back) counts as no gap, so a clock step cannot disable the check.
pub(super) fn within_ms_or_earlier(earlier: Timestamp, later: Timestamp, max_ms: u64) -> bool {
    u64::try_from(later.0.saturating_sub(earlier.0).max(0)).is_ok_and(|d| d <= max_ms)
}

/// `later - earlier` in ms, if `later` is not before `earlier` and at most `max_ms` after it.
pub(super) fn within_ms(earlier: Timestamp, later: Timestamp, max_ms: u64) -> bool {
    later
        .0
        .checked_sub(earlier.0)
        .and_then(|d| u64::try_from(d).ok())
        .is_some_and(|d| d <= max_ms)
}

impl Actor {
    pub(super) fn on_signal(&mut self, s: ChangeSignal) {
        #[cfg(test)]
        {
            let stall = *self
                .hooks
                .stall_on_signal
                .lock()
                .unwrap_or_else(PoisonError::into_inner);
            if let Some((seq, d)) = stall {
                if seq == s.seq {
                    std::thread::sleep(d);
                }
            }
        }
        #[cfg(test)]
        {
            let mut lost = self
                .hooks
                .drop_signal
                .lock()
                .unwrap_or_else(PoisonError::into_inner);
            if let Some(i) = lost.iter().position(|q| *q == s.seq) {
                lost.remove(i);
                return;
            }
        }
        let now = Instant::now();
        // Paused, private, disabled or our own write: no clipboard access at all. A live
        // claim (Task 603) lets a Paused or Disabled change through to its decision only when
        // it can be the target's (`claim_admits`); never in Private mode.
        let mut admitted = None;
        if let Some(reason) = self.gate(s.seq) {
            if !self.claim_admits(reason, now, &s) {
                if self.claim.is_some() {
                    self.claim_gated();
                }
                self.drop_gated(s.seq, reason);
                return;
            }
            admitted = Some(reason);
        }
        if let Some(p) = &self.pending {
            if p.seq == s.seq {
                let due = self.due_at(now, p.first_at);
                if let Some(p) = &mut self.pending {
                    p.due = due;
                    p.last_signal_at = s.at;
                    // A later signal of the same change may name another owner: it is
                    // checked too (`decide_all_sources`), never made primary.
                    if let Some(src) = s.source {
                        if !p.sources.contains(&src) {
                            p.sources.push(src);
                        }
                    }
                }
                return;
            }
        }
        // A stale signal for a change already handled must not discard a pending one.
        if self.last_processed_seq == Some(s.seq) {
            return;
        }
        // A different change: the previous change's sources never become this change's own
        // sources. The burst start is kept so the cap still bounds the wait.
        let prev = self.pending.take();
        let first_at = prev.as_ref().map_or(now, |p| p.first_at);
        // Task 603: an ownerless change replacing one the claim let through may be its flush:
        // it is held as well, so it is never stored unless claimed.
        let held = admitted.or_else(|| {
            if s.source.is_none() {
                prev.as_ref().and_then(|p| p.held)
            } else {
                None
            }
        });
        // F-14: a replaced pending change was never processed, so it always joins the chain,
        // whatever the actor's own timing (rule 1: its sources are checked for exclusion,
        // which only adds skips; rule 2: it may name an ownerless flush's source, bounded by
        // signal times in `heir_source`).
        let (superseded, heir, burst_signal_at) = match prev {
            Some(p) => self.supersede(p),
            None => (Vec::new(), None, s.at),
        };
        // The signal's source (resolved at notification time) is primary; resolving now is
        // only the fallback for backends that cannot resolve at signal time (D-031).
        let source = s.source.or_else(|| self.source.resolve());
        self.pending = Some(Pending {
            seq: s.seq,
            sources: source.into_iter().collect(),
            first_at,
            due: self.due_at(now, first_at),
            burst_signal_at,
            signal_at: s.at,
            last_signal_at: s.at,
            seen_at: now,
            held,
            superseded,
            heir,
        });
    }

    /// The chain a superseded, never-processed change hands to the change that replaces it:
    /// every source seen in the burst so far, the latest change with a source, and the
    /// burst's first signal time.
    fn supersede(&self, p: Pending) -> (Vec<ClipboardSource>, Option<Superseded>, Timestamp) {
        let mut superseded = p.superseded;
        for s in &p.sources {
            if !superseded.contains(s) {
                superseded.push(s.clone());
            }
        }
        let inspected = self.last_processed_seq == Some(p.seq)
            || self.carried.as_ref().is_some_and(|c| c.seq == p.seq);
        let heir = match p.sources.into_iter().next() {
            Some(source) => Some(Superseded {
                seq: p.seq,
                source,
                last_signal_at: p.last_signal_at,
                inspected,
            }),
            None => p.heir,
        };
        (superseded, heir, p.burst_signal_at)
    }

    /// F-14 rule 3: the excluded sources an ownerless change `seq` (first signal at
    /// `signal_at`) must also be checked against: those of the last excluded-app skip, when
    /// it is 1..=`MAX_HEIR_SEQ_GAP` seqs and at most `max_burst_wait` before this change,
    /// with no self-write in between. Skip only; never a stored source.
    pub(super) fn recent_excluded(&self, seq: u64, signal_at: Timestamp) -> &[ClipboardSource] {
        let Some(r) = &self.excluded_recent else {
            return &[];
        };
        let cap_ms = u64::try_from(self.max_burst_wait.as_millis()).unwrap_or(u64::MAX);
        let applies = seq
            .checked_sub(r.seq)
            .is_some_and(|gap| (1..=MAX_HEIR_SEQ_GAP).contains(&gap))
            && !self
                .last_self_write_seq
                .is_some_and(|w| w > r.seq && w <= seq)
            && within_ms_or_earlier(r.signal_at, signal_at, cap_ms);
        if applies {
            &r.sources
        } else {
            &[]
        }
    }

    /// F-14 rule 2: the stored source of an ownerless change, taken from the change it
    /// superseded, only for the OLE flush shape: never inspected, same seq as the snapshot,
    /// at most `MAX_HEIR_SEQ_GAP` seqs and `debounce_ms` after it, inside the burst cap, no
    /// self-write in between, and the snapshot still offers an OLE placeholder. The caller
    /// asks only when the change has no source of its own.
    pub(super) fn heir_source(&self, p: &Pending, meta: &ClipboardMeta) -> Option<ClipboardSource> {
        let heir = p.heir.as_ref()?;
        let cap_ms = u64::try_from(self.max_burst_wait.as_millis()).unwrap_or(u64::MAX);
        let eligible = meta.seq == p.seq
            && !heir.inspected
            && self.last_processed_seq != Some(heir.seq)
            && p.seq
                .checked_sub(heir.seq)
                .is_some_and(|gap| (1..=MAX_HEIR_SEQ_GAP).contains(&gap))
            && !self
                .last_self_write_seq
                .is_some_and(|w| w > heir.seq && w <= p.seq)
            && within_ms(heir.last_signal_at, p.signal_at, self.cfg.debounce_ms)
            && within_ms(p.burst_signal_at, p.signal_at, cap_ms)
            && has_ole_signature(meta);
        eligible.then(|| heir.source.clone())
    }

    /// Debounce deadline, capped at `max_burst_wait` after the burst's first signal.
    fn due_at(&self, now: Instant, first_at: Instant) -> Instant {
        let quiet = now
            .checked_add(Duration::from_millis(self.cfg.debounce_ms))
            .unwrap_or(now);
        let cap = first_at
            .checked_add(self.max_burst_wait)
            .unwrap_or(first_at);
        quiet.min(cap)
    }
}
