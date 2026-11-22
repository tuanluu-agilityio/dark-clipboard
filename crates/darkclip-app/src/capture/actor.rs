//! The actor thread: start-up, the run loop, the settings/self-write gate, platform retries
//! and command handling. It owns the platform access, the monitor and the storage handle.

use super::pending::{Carried, ExcludedRecent, Pending};
use super::pipeline::gate_without_formats;
use super::{ActorCommand, Msg, RetentionHooks};
use crate::config::CaptureConfig;
use crate::events::{map_storage, ApiError, AppEvent, SkipCounters, UNAVAILABLE_READ};
use darkclip_core::policy::{Decision, SizeCaps, SkipReason};
use darkclip_core::{ClipId, Clock, PrivacyPolicy, RetentionPolicy, SensitivityLevel, Timestamp};
use darkclip_platform::{
    ChangeSignal, ClipboardAccess, ClipboardMonitor, PlatformError, SourceResolver,
};
use darkclip_storage::Storage;
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};

pub(super) fn size_caps(r: &RetentionPolicy) -> SizeCaps {
    SizeCaps {
        text_max_bytes: u64::from(r.text_max_kb).saturating_mul(1024),
        image_max_bytes: u64::from(r.image_max_mb).saturating_mul(1024 * 1024),
    }
}

/// The caller of a reply-bearing command has stopped waiting.
pub(super) fn late(deadline: Instant) -> bool {
    Instant::now() >= deadline
}

pub(super) fn map_platform(e: &PlatformError) -> ApiError {
    match e {
        PlatformError::Busy => ApiError::ClipboardBusy,
        PlatformError::Unsupported(_) => ApiError::UnsupportedPlatform,
        PlatformError::PermissionDenied(what) => ApiError::PermissionDenied(what),
        PlatformError::TooLarge { .. } | PlatformError::Backend { .. } => ApiError::Internal,
    }
}

pub(super) struct Actor {
    pub(super) monitor: Box<dyn ClipboardMonitor>,
    pub(super) access: Box<dyn ClipboardAccess>,
    pub(super) source: Box<dyn SourceResolver>,
    pub(super) storage: Storage,
    pub(super) clock: Arc<dyn Clock>,
    pub(super) cfg: CaptureConfig,
    pub(super) max_burst_wait: Duration,
    pub(super) policy: PrivacyPolicy,
    pub(super) retention: RetentionPolicy,
    pub(super) events: Sender<AppEvent>,
    pub(super) counters: Arc<Mutex<SkipCounters>>,
    /// Test instrumentation; unused in release builds.
    #[cfg_attr(not(test), allow(dead_code))]
    pub(super) hooks: Arc<RetentionHooks>,
    pub(super) last_processed_seq: Option<u64>,
    pub(super) last_self_write_seq: Option<u64>,
    pub(super) pending: Option<Pending>,
    /// Source of a change that had nothing readable yet (OLE delayed rendering), kept for a
    /// later burst with the same sequence number.
    pub(super) carried: Option<Carried>,
    /// F-14 rule 3: the last excluded-app skip, for a following ownerless flush.
    pub(super) excluded_recent: Option<ExcludedRecent>,
    pub(super) next_retention: Instant,
    /// Earliest `expires_at` in storage (clock time); retention runs when it falls due so a
    /// masked clip does not outlive its TTL.
    pub(super) next_expiry: Option<Timestamp>,
    /// After a failed retention: no expiry-driven retry before this instant.
    pub(super) retention_retry_at: Option<Instant>,
    /// Current back-off step; `None` after a success.
    pub(super) retention_backoff: Option<Duration>,
}

impl Actor {
    pub(super) fn start(
        mut self,
        fwd_tx: Sender<Msg>,
        rx: Receiver<Msg>,
        ready: Sender<Result<(), ApiError>>,
    ) {
        let (sig_tx, sig_rx) = mpsc::channel::<ChangeSignal>();
        if let Err(e) = self.monitor.start(sig_tx) {
            let _ = ready.send(Err(map_platform(&e)));
            return;
        }
        let fwd = std::thread::Builder::new()
            .name("darkclip-signal-fwd".into())
            .spawn(move || {
                for s in sig_rx {
                    if fwd_tx.send(Msg::Signal(s)).is_err() {
                        return;
                    }
                }
                let _ = fwd_tx.send(Msg::MonitorGone);
            });
        if fwd.is_err() {
            self.monitor.stop();
            let _ = ready.send(Err(ApiError::Internal));
            return;
        }
        let _ = ready.send(Ok(()));
        self.run_retention();
        self.run(&rx);
    }

    fn run(&mut self, rx: &Receiver<Msg>) {
        loop {
            let mut wake = self.next_retention.min(self.expiry_wake());
            if let Some(p) = &self.pending {
                wake = wake.min(p.due);
            }
            match rx.recv_timeout(wake.saturating_duration_since(Instant::now())) {
                Ok(Msg::Signal(s)) => self.on_signal(s),
                Ok(Msg::Cmd(ActorCommand::Shutdown { reply })) => {
                    self.monitor.stop();
                    let _ = reply.send(());
                    return;
                }
                Ok(Msg::Cmd(cmd)) => self.on_command(cmd),
                Ok(Msg::MonitorGone) => {
                    self.emit(AppEvent::Unavailable("clipboard monitor stopped"))
                }
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => {
                    self.monitor.stop();
                    return;
                }
            }
            let now = Instant::now();
            if self.pending.as_ref().is_some_and(|p| p.due <= now) {
                if let Some(p) = self.pending.take() {
                    self.process(p);
                }
            }
            if self.next_retention <= now || self.expiry_due() {
                self.run_retention();
            }
        }
    }

    pub(super) fn emit(&self, e: AppEvent) {
        // A closed receiver means nobody listens; capture continues.
        let _ = self.events.send(e);
    }

    pub(super) fn skip(&self, reason: SkipReason) {
        self.counters
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .increment(reason);
        self.emit(AppEvent::Skipped(reason));
    }

    pub(super) fn gate(&self, seq: u64) -> Option<SkipReason> {
        match gate_without_formats(
            &self.policy,
            self.clock.now(),
            seq,
            self.last_self_write_seq,
            size_caps(&self.retention),
        ) {
            Some(Decision::Skip(r)) => Some(r),
            _ => None,
        }
    }

    /// Drops a change without looking at the clipboard; one event per sequence number.
    pub(super) fn drop_gated(&mut self, seq: u64, reason: SkipReason) {
        self.pending = None;
        if self.last_processed_seq != Some(seq) {
            self.last_processed_seq = Some(seq);
            self.skip(reason);
        }
    }

    /// Calls `f` and retries while the clipboard is `Busy`, at most `open_retries` times.
    pub(super) fn with_retries<T>(
        &mut self,
        mut f: impl FnMut(&mut dyn ClipboardAccess) -> Result<T, PlatformError>,
    ) -> Result<T, PlatformError> {
        let mut retries = 0u32;
        loop {
            match f(self.access.as_mut()) {
                Err(PlatformError::Busy) if retries < self.cfg.open_retries => {
                    retries += 1;
                    std::thread::sleep(Duration::from_millis(self.cfg.retry_delay_ms));
                }
                r => return r,
            }
        }
    }

    /// A platform failure while capturing: counted as a skip where a reason fits.
    pub(super) fn capture_failed(&self, e: &PlatformError) {
        match e {
            PlatformError::Busy => self.skip(SkipReason::Busy),
            PlatformError::TooLarge { .. } => self.skip(SkipReason::TooLarge),
            PlatformError::Unsupported(what) | PlatformError::PermissionDenied(what) => {
                self.emit(AppEvent::Unavailable(what));
            }
            PlatformError::Backend { .. } => {
                self.emit(AppEvent::Unavailable(UNAVAILABLE_READ));
            }
        }
    }

    fn capture_blocked(&self) -> bool {
        // Any sequence number that is not our last write: only settings decide.
        let seq = self.last_self_write_seq.map_or(0, |s| s.wrapping_add(1));
        self.gate(seq).is_some()
    }

    fn on_command(&mut self, cmd: ActorCommand) {
        match cmd {
            ActorCommand::CopyBack {
                id,
                plain,
                deadline,
                reply,
            } => {
                let r = if late(deadline) {
                    Err(ApiError::Internal)
                } else {
                    self.copy_back(&id, plain)
                };
                let _ = reply.send(r);
            }
            ActorCommand::SetLang(lang) => self.cfg.lang = lang,
            ActorCommand::SetPinned {
                id,
                pinned,
                deadline,
                reply,
            } => {
                let r = if late(deadline) {
                    Err(ApiError::Internal)
                } else {
                    self.set_pinned(&id, pinned)
                };
                let _ = reply.send(r);
            }
            ActorCommand::DeleteClip {
                id,
                deadline,
                reply,
            } => {
                let r = if late(deadline) {
                    Err(ApiError::Internal)
                } else {
                    self.storage.delete_clip(&id).map_err(|e| map_storage(&e))
                };
                let _ = reply.send(r);
            }
            ActorCommand::ClearHistory {
                keep_pinned,
                deadline,
                reply,
            } => {
                let r = if late(deadline) {
                    Err(ApiError::Internal)
                } else {
                    self.storage
                        .clear_history(keep_pinned)
                        .map_err(|e| map_storage(&e))
                };
                let _ = reply.send(r);
            }
            ActorCommand::SetMode(mode) => {
                let p = PrivacyPolicy {
                    mode,
                    ..self.policy.clone()
                };
                self.apply_policy(p);
                self.emit(AppEvent::ModeChanged(mode));
            }
            ActorCommand::UpdatePolicy(p, r) => {
                let mode_changed = p.mode != self.policy.mode;
                let mode = p.mode;
                self.retention = r;
                self.apply_policy(p);
                if mode_changed {
                    self.emit(AppEvent::ModeChanged(mode));
                }
                self.run_retention();
            }
            ActorCommand::Shutdown { reply } => {
                // Handled in `run`; kept for exhaustiveness.
                let _ = reply.send(());
            }
        }
    }

    /// Entering a blocked mode drops the pending burst; leaving one marks the clipboard as
    /// it is now as seen, so resuming never backfills (privacy-design §5).
    fn apply_policy(&mut self, p: PrivacyPolicy) {
        let was_blocked = self.capture_blocked();
        self.policy = p;
        let blocked = self.capture_blocked();
        if blocked {
            self.pending = None;
            self.carried = None;
        } else if was_blocked {
            if let Ok(seq) = self.access.sequence() {
                self.last_processed_seq = Some(seq);
            }
        }
    }

    fn set_pinned(&mut self, id: &ClipId, pinned: bool) -> Result<(), ApiError> {
        if pinned {
            let clip = self
                .storage
                .get_clip(id)
                .map_err(|e| map_storage(&e))?
                .ok_or(ApiError::NotFound)?;
            if clip.sensitivity.level == SensitivityLevel::Masked {
                return Err(ApiError::InvalidArgument("masked clips cannot be pinned"));
            }
        }
        self.storage
            .set_pinned(id, pinned)
            .map_err(|e| map_storage(&e))
    }
}
