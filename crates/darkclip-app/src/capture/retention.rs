//! Retention scheduling: the periodic run, expiry-driven runs for masked clips, the
//! over-limit run after an insert, and the back-off after a failure.

use super::actor::Actor;
use crate::events::{AppEvent, UNAVAILABLE_RETENTION};
use darkclip_storage::StorageError;
#[cfg(test)]
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

/// Retention runs on start, after an insert that exceeds a limit, and at this interval.
const RETENTION_EVERY: Duration = Duration::from_secs(10 * 60);

/// Wait after the first failed retention; doubles per further failure up to the cap.
const RETENTION_BACKOFF_FIRST: Duration = Duration::from_secs(30);
const RETENTION_BACKOFF_MAX: Duration = Duration::from_secs(10 * 60);

pub(super) fn next_backoff(prev: Option<Duration>) -> Duration {
    match prev {
        None => RETENTION_BACKOFF_FIRST,
        Some(d) => d.saturating_mul(2).min(RETENTION_BACKOFF_MAX),
    }
}

impl Actor {
    fn in_backoff(&self) -> bool {
        self.retention_retry_at.is_some_and(|t| Instant::now() < t)
    }

    pub(super) fn expiry_due(&self) -> bool {
        !self.in_backoff() && self.next_expiry.is_some_and(|t| t <= self.clock.now())
    }

    /// Real-time estimate of the next expiry (the clock may be a test clock: the loop also
    /// re-checks `expiry_due` after every message), never before the back-off ends.
    pub(super) fn expiry_wake(&self) -> Instant {
        let Some(next) = self.next_expiry else {
            return self.next_retention;
        };
        let now = Instant::now();
        let ms = u64::try_from(next.0.saturating_sub(self.clock.now().0)).unwrap_or(0);
        let at = now
            .checked_add(Duration::from_millis(ms))
            .unwrap_or(self.next_retention);
        match self.retention_retry_at {
            Some(retry) => at.max(retry),
            None => at,
        }
    }

    pub(super) fn retention_if_over_limit(&mut self) {
        let Ok(st) = self.storage.stats() else {
            return;
        };
        let r = &self.retention;
        let over_items = r.max_items > 0 && st.clips > u64::from(r.max_items);
        let limit = u64::from(r.max_storage_mb).saturating_mul(1024 * 1024);
        let over_size = r.max_storage_mb > 0 && st.db_bytes.saturating_add(st.blob_bytes) > limit;
        if (over_items || over_size) && !self.in_backoff() {
            self.run_retention();
        }
    }

    pub(super) fn run_retention(&mut self) {
        #[cfg(test)]
        self.hooks.attempts.fetch_add(1, Ordering::Relaxed);
        #[cfg(test)]
        let forced = self.hooks.fail.load(Ordering::SeqCst);
        #[cfg(not(test))]
        let forced = false;
        let result = if forced {
            Err(StorageError::ReadOnly)
        } else {
            self.storage.apply_retention(&self.retention).map(|_| ())
        };
        if result.is_ok() {
            if self.retention_backoff.is_some() {
                self.emit(AppEvent::RetentionOk);
            }
            self.retention_backoff = None;
            self.retention_retry_at = None;
            // On error fall back to the periodic timer rather than keep a stale schedule.
            #[cfg(test)]
            let expiry = if self.hooks.expiry_fail.load(Ordering::SeqCst) {
                Err(StorageError::ReadOnly)
            } else {
                self.storage.next_expiry()
            };
            #[cfg(not(test))]
            let expiry = self.storage.next_expiry();
            self.next_expiry = expiry.unwrap_or(None);
        } else {
            // One attempt, hence at most one event, per back-off step (no busy loop on a
            // past expiry).
            let step = next_backoff(self.retention_backoff);
            self.retention_backoff = Some(step);
            let now = Instant::now();
            self.retention_retry_at = Some(now.checked_add(step).unwrap_or(now));
            self.emit(AppEvent::Unavailable(UNAVAILABLE_RETENTION));
        }
        #[cfg(test)]
        let every = match self.hooks.tick_ms.load(Ordering::SeqCst) {
            0 => RETENTION_EVERY,
            ms => Duration::from_millis(ms),
        };
        #[cfg(not(test))]
        let every = RETENTION_EVERY;
        let now = Instant::now();
        self.next_retention = now.checked_add(every).unwrap_or(now);
    }
}

/// Longest a word of an automatically evicted clip (or of an edited note or tag) stays in the
/// search index after it became pending.
const FTS_MERGE_WITHIN: Duration = Duration::from_secs(10 * 60);

impl Actor {
    /// Real-time estimate of the merge deadline (the clock may be a test clock: the loop also
    /// re-checks after every message).
    pub(super) fn fts_wake(&self) -> Instant {
        let Some(due) = self.fts_merge_due else {
            return self.next_retention;
        };
        let ms = u64::try_from(due.0.saturating_sub(self.clock.now().0)).unwrap_or(0);
        let now = Instant::now();
        now.checked_add(Duration::from_millis(ms))
            .unwrap_or(self.next_retention)
    }

    /// Arms the deadline when changes became pending, clears it when none are, merges when it
    /// is due. Called on every loop iteration; later evictions and retention runs never move
    /// the deadline, so steady captures cannot postpone the merge.
    pub(super) fn fts_housekeeping(&mut self) {
        let now = self.clock.now();
        self.sync_fts_deadline(now);
        if self.fts_merge_due.is_some_and(|due| due <= now) {
            self.storage.optimize_fts_if_dirty();
            self.sync_fts_deadline(now);
        }
    }

    fn sync_fts_deadline(&mut self, now: darkclip_core::Timestamp) {
        let ms = i64::try_from(FTS_MERGE_WITHIN.as_millis()).unwrap_or(i64::MAX);
        match (self.storage.fts_pending() > 0, self.fts_merge_due) {
            (true, None) => {
                self.fts_merge_due = Some(darkclip_core::Timestamp(now.0.saturating_add(ms)));
            }
            (false, Some(_)) => self.fts_merge_due = None,
            _ => {}
        }
    }
}
