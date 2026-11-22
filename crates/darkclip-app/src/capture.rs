//! CaptureActor: the single serialized clipboard worker (system-overview §2 rules 1–7).
//!
//! One thread owns the platform's `ClipboardAccess`, the monitor and the `Storage` handle.
//! It is the only code that calls `ClipboardAccess::read` (after `Decision::Read`) and
//! `ClipboardAccess::write` (GC-03). A small forwarder thread moves the monitor's change
//! signals into the actor's queue so that one `recv_timeout` loop serves both signals and
//! commands; the forwarder never touches the clipboard.
//!
//! Privacy order per change: settings and self-write gate (no clipboard access at all) →
//! `snapshot_meta` (names, markers, source; never content) → `decide_pre_read` → `read` →
//! `classify` → kind check → sensitive scan → hash → store. This module never logs (GC-06):
//! events carry ids, skip reasons and static text only.

use crate::config::CaptureConfig;
use crate::events::{ApiError, AppEvent, SkipCounters};
use darkclip_core::policy::{
    decide_pre_read, AvailableFormat, Decision, PreReadInput, SizeCaps, SkipReason,
};
use darkclip_core::sensitive::{masked_preview, scan, MaskHint, ScanResult};
use darkclip_core::{
    canonical_hash, classify, CaptureMode, ClipId, ClipboardSource, Clock, Lang, Mime,
    PrivacyPolicy, RawRepresentation, RetentionPolicy, SensitiveHandling, Sensitivity,
    SensitivityLevel, Timestamp,
};
use darkclip_platform::{
    ChangeSignal, ClipboardAccess, ClipboardMeta, ClipboardMonitor, PlatformBundle, PlatformError,
    SourceResolver,
};
use darkclip_storage::{NewClip, Storage, StorageError, UpsertOutcome};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex, PoisonError};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

/// Retention runs on start, after an insert that exceeds a limit, and at this interval.
const RETENTION_EVERY: Duration = Duration::from_secs(10 * 60);

/// Language of stored masked previews. The UI language arrives with settings (Task 011).
const MASK_LANG: Lang = Lang::En;

#[derive(Debug)]
pub enum ActorCommand {
    /// Writes a stored clip back to the clipboard (`plain`: text/plain only).
    CopyBack {
        id: ClipId,
        plain: bool,
        reply: Sender<Result<(), ApiError>>,
    },
    SetMode(CaptureMode),
    UpdatePolicy(PrivacyPolicy, RetentionPolicy),
    /// Processes every command queued before it, stops the monitor and ends the actor.
    Shutdown {
        reply: Sender<()>,
    },
}

/// What the actor's queue carries.
enum Msg {
    Cmd(ActorCommand),
    Signal(ChangeSignal),
    MonitorGone,
}

/// Owner side of the capture actor.
pub struct CaptureHandle {
    tx: Option<Sender<Msg>>,
    join: Option<JoinHandle<()>>,
    counters: Arc<Mutex<SkipCounters>>,
}

impl CaptureHandle {
    /// Starts the actor thread, which starts the monitor and runs retention once.
    pub fn spawn(
        bundle: PlatformBundle,
        storage: Storage,
        clock: Arc<dyn Clock>,
        cfg: CaptureConfig,
        policy: PrivacyPolicy,
        retention: RetentionPolicy,
        events: Sender<AppEvent>,
    ) -> Result<Self, ApiError> {
        let (tx, rx) = mpsc::channel::<Msg>();
        let (ready_tx, ready_rx) = mpsc::channel::<Result<(), ApiError>>();
        let counters = Arc::new(Mutex::new(SkipCounters::default()));
        let PlatformBundle {
            monitor,
            access,
            source,
            ..
        } = bundle;
        let actor = Actor {
            monitor,
            access,
            source,
            storage,
            clock,
            cfg,
            policy,
            retention,
            events,
            counters: Arc::clone(&counters),
            last_processed_seq: None,
            last_self_write_seq: None,
            pending: None,
            carried: None,
            next_retention: Instant::now(),
        };
        let fwd_tx = tx.clone();
        let join = std::thread::Builder::new()
            .name("darkclip-capture".into())
            .spawn(move || actor.start(fwd_tx, rx, ready_tx))
            .map_err(|_| ApiError::Internal)?;
        match ready_rx.recv() {
            Ok(Ok(())) => Ok(Self {
                tx: Some(tx),
                join: Some(join),
                counters,
            }),
            Ok(Err(e)) => {
                let _ = join.join();
                Err(e)
            }
            Err(_) => {
                let _ = join.join();
                Err(ApiError::Internal)
            }
        }
    }

    pub fn send(&self, cmd: ActorCommand) -> Result<(), ApiError> {
        let tx = self.tx.as_ref().ok_or(ApiError::AppNotRunning)?;
        tx.send(Msg::Cmd(cmd)).map_err(|_| ApiError::AppNotRunning)
    }

    /// A copy of the in-memory skip counters (for `get_storage_stats`, Task 011).
    pub fn skip_counters(&self) -> SkipCounters {
        self.counters
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    /// Drains queued commands, stops the monitor and joins the actor within `deadline`
    /// (callers use 2 s). On timeout the actor thread is left to finish on its own.
    pub fn shutdown(mut self, deadline: Duration) -> Result<(), ApiError> {
        let (reply, done) = mpsc::channel();
        let sent = self.send(ActorCommand::Shutdown { reply });
        self.tx = None;
        sent?;
        done.recv_timeout(deadline)
            .map_err(|_| ApiError::Internal)?;
        match self.join.take() {
            Some(j) => j.join().map_err(|_| ApiError::Internal),
            None => Ok(()),
        }
    }
}

impl Drop for CaptureHandle {
    /// Dropped without `shutdown`: ask the actor to stop, without blocking the caller.
    fn drop(&mut self) {
        if let Some(tx) = self.tx.take() {
            let (reply, _done) = mpsc::channel();
            let _ = tx.send(Msg::Cmd(ActorCommand::Shutdown { reply }));
        }
    }
}

/// The policy decision for the steps that need no clipboard data (settings, self-write),
/// taken with empty markers and formats so the clipboard is not touched (D-021).
/// `Some(skip)` drops the change here; `None` means the clipboard may be inspected.
fn gate_without_formats(
    policy: &PrivacyPolicy,
    now: Timestamp,
    seq: u64,
    last_self_write_seq: Option<u64>,
    caps: SizeCaps,
) -> Option<Decision> {
    let d = decide_pre_read(&PreReadInput {
        seq,
        last_self_write_seq,
        mode: &policy.mode,
        now,
        capture_enabled: policy.capture_enabled,
        markers: &[],
        source: None,
        excluded: &policy.excluded_apps,
        available: &[],
        enabled_kinds: &policy.capture_kinds,
        caps,
    });
    match d {
        Decision::Skip(
            SkipReason::Disabled | SkipReason::Paused | SkipReason::Private | SkipReason::SelfWrite,
        ) => Some(d),
        _ => None,
    }
}

fn size_caps(r: &RetentionPolicy) -> SizeCaps {
    SizeCaps {
        text_max_bytes: u64::from(r.text_max_kb).saturating_mul(1024),
        image_max_bytes: u64::from(r.image_max_mb).saturating_mul(1024 * 1024),
    }
}

fn map_platform(e: &PlatformError) -> ApiError {
    match e {
        PlatformError::Busy => ApiError::ClipboardBusy,
        PlatformError::Unsupported(_) => ApiError::UnsupportedPlatform,
        PlatformError::PermissionDenied(what) => ApiError::PermissionDenied(what),
        PlatformError::TooLarge { .. } | PlatformError::Backend { .. } => ApiError::Internal,
    }
}

fn map_storage(e: &StorageError) -> ApiError {
    match e {
        StorageError::NotFound => ApiError::NotFound,
        StorageError::SchemaTooNew { .. } => ApiError::SchemaTooNew,
        _ => ApiError::Storage,
    }
}

/// Best-effort overwrite of content buffers on paths that discard them.
fn wipe_bytes(v: &mut [u8]) {
    v.fill(0);
    std::hint::black_box(&*v);
}

fn wipe_string(s: &mut String) {
    let mut v = std::mem::take(s).into_bytes();
    wipe_bytes(&mut v);
}

fn wipe_reps(reps: &mut [RawRepresentation]) {
    for r in reps {
        wipe_bytes(&mut r.bytes);
    }
}

/// A burst of change signals that is waiting for its quiet period.
struct Pending {
    seq: u64,
    /// Source at the first signal of the burst (the owner may be gone by the last one).
    source: Option<ClipboardSource>,
    due: Instant,
}

struct Actor {
    monitor: Box<dyn ClipboardMonitor>,
    access: Box<dyn ClipboardAccess>,
    source: Box<dyn SourceResolver>,
    storage: Storage,
    clock: Arc<dyn Clock>,
    cfg: CaptureConfig,
    policy: PrivacyPolicy,
    retention: RetentionPolicy,
    events: Sender<AppEvent>,
    counters: Arc<Mutex<SkipCounters>>,
    last_processed_seq: Option<u64>,
    last_self_write_seq: Option<u64>,
    pending: Option<Pending>,
    /// Source of a change that had nothing readable yet (OLE delayed rendering), kept for a
    /// later burst with the same sequence number.
    carried: Option<(u64, Option<ClipboardSource>)>,
    next_retention: Instant,
}

impl Actor {
    fn start(
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
            let wake = match &self.pending {
                Some(p) => p.due.min(self.next_retention),
                None => self.next_retention,
            };
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
            if self.next_retention <= now {
                self.run_retention();
            }
        }
    }

    fn emit(&self, e: AppEvent) {
        // A closed receiver means nobody listens; capture continues.
        let _ = self.events.send(e);
    }

    fn skip(&self, reason: SkipReason) {
        self.counters
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .increment(reason);
        self.emit(AppEvent::Skipped(reason));
    }

    fn gate(&self, seq: u64) -> Option<SkipReason> {
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
    fn drop_gated(&mut self, seq: u64, reason: SkipReason) {
        self.pending = None;
        if self.last_processed_seq != Some(seq) {
            self.last_processed_seq = Some(seq);
            self.skip(reason);
        }
    }

    fn on_signal(&mut self, s: ChangeSignal) {
        // Paused, private, disabled or our own write: no clipboard access at all.
        if let Some(reason) = self.gate(s.seq) {
            self.drop_gated(s.seq, reason);
            return;
        }
        let due = Instant::now() + Duration::from_millis(self.cfg.debounce_ms);
        if let Some(p) = &mut self.pending {
            p.seq = s.seq;
            p.due = due;
            return;
        }
        if self.last_processed_seq == Some(s.seq) {
            return;
        }
        self.pending = Some(Pending {
            seq: s.seq,
            source: self.source.resolve(),
            due,
        });
    }

    /// Calls `f` and retries while the clipboard is `Busy`, at most `open_retries` times.
    fn with_retries<T>(
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
    fn capture_failed(&self, e: &PlatformError) {
        match e {
            PlatformError::Busy => self.skip(SkipReason::Busy),
            PlatformError::TooLarge { .. } => self.skip(SkipReason::TooLarge),
            PlatformError::Unsupported(what) | PlatformError::PermissionDenied(what) => {
                self.emit(AppEvent::Unavailable(what));
            }
            PlatformError::Backend { .. } => {
                self.emit(AppEvent::Unavailable("clipboard read failed"));
            }
        }
    }

    fn process(&mut self, p: Pending) {
        if self.last_processed_seq == Some(p.seq) {
            return;
        }
        if let Some(reason) = self.gate(p.seq) {
            self.drop_gated(p.seq, reason);
            return;
        }
        let meta: ClipboardMeta = match self.with_retries(|a| a.snapshot_meta()) {
            Ok(m) => m,
            Err(e) => {
                self.last_processed_seq = Some(p.seq);
                self.capture_failed(&e);
                return;
            }
        };
        // The snapshot's sequence number is the truth (rule 2).
        let seq = meta.seq;
        if self.last_processed_seq == Some(seq) {
            return;
        }
        if let Some(reason) = self.gate(seq) {
            self.drop_gated(seq, reason);
            return;
        }
        let carried = match self.carried.take() {
            Some((s, src)) if s == seq => src,
            _ => None,
        };
        let source = carried.or(p.source).or(meta.source);
        let available: Vec<AvailableFormat> = meta
            .formats
            .iter()
            .filter_map(|f| {
                f.mime.map(|mime| AvailableFormat {
                    mime,
                    platform_format: f.name.clone(),
                    size_hint: f.size_hint,
                })
            })
            .collect();
        let now = self.clock.now();
        let decision = decide_pre_read(&PreReadInput {
            seq,
            last_self_write_seq: self.last_self_write_seq,
            mode: &self.policy.mode,
            now,
            capture_enabled: self.policy.capture_enabled,
            markers: &meta.markers,
            source: source.as_ref(),
            excluded: &self.policy.excluded_apps,
            available: &available,
            enabled_kinds: &self.policy.capture_kinds,
            caps: size_caps(&self.retention),
        });
        let plan = match decision {
            Decision::Read(plan) => plan,
            Decision::Skip(SkipReason::NothingReadable) => {
                // Delayed rendering: the same seq may get real formats later. Not marked
                // processed; the first-signal source is kept for that later burst.
                self.carried = Some((seq, source));
                self.skip(SkipReason::NothingReadable);
                return;
            }
            Decision::Skip(reason) => {
                self.last_processed_seq = Some(seq);
                self.skip(reason);
                return;
            }
        };
        // Policy said Read: content may be read now (GC-03).
        let read = self.with_retries(|a| a.read(&plan));
        self.last_processed_seq = Some(seq);
        let mut reps = match read {
            Ok(r) => r,
            Err(e) => {
                self.capture_failed(&e);
                return;
            }
        };
        // The markers were checked for `seq`. If the clipboard changed since the snapshot,
        // the bytes may belong to a change nobody checked: discard them. That change brings
        // its own signal.
        if self.access.sequence().ok() != Some(seq) {
            wipe_reps(&mut reps);
            return;
        }
        if reps.is_empty() {
            self.skip(SkipReason::NothingReadable);
            return;
        }
        self.store(reps, source, now);
    }

    fn store(
        &mut self,
        mut reps: Vec<RawRepresentation>,
        source: Option<ClipboardSource>,
        now: Timestamp,
    ) {
        let mut classified = classify(&reps);
        if !self.policy.capture_kinds.contains(&classified.kind) {
            self.discard(
                &mut reps,
                &mut classified.preview,
                &mut classified.plain_text,
            );
            self.skip(SkipReason::KindDisabled);
            return;
        }
        let found: ScanResult = classified
            .plain_text
            .as_deref()
            .map(|t| scan(t, &self.policy.sensitive_categories_enabled))
            .unwrap_or_default();
        let (sensitivity, expires_at) = if found.categories.is_empty() {
            (Sensitivity::default(), None)
        } else {
            match self.policy.sensitive_handling {
                SensitiveHandling::Drop => {
                    self.discard(
                        &mut reps,
                        &mut classified.preview,
                        &mut classified.plain_text,
                    );
                    self.skip(SkipReason::Sensitive);
                    return;
                }
                SensitiveHandling::StoreMasked => {
                    let hint = found.mask_hint.clone().unwrap_or_else(|| MaskHint {
                        visible_prefix: String::new(),
                        category: found
                            .categories
                            .first()
                            .copied()
                            .unwrap_or(darkclip_core::SensitiveCategory::Password),
                    });
                    let mut old = std::mem::replace(
                        &mut classified.preview,
                        masked_preview(&hint, MASK_LANG),
                    );
                    wipe_string(&mut old);
                    let ttl_ms =
                        i64::from(self.retention.sensitive_ttl_seconds).saturating_mul(1000);
                    (
                        Sensitivity {
                            level: SensitivityLevel::Masked,
                            categories: found.categories.clone(),
                        },
                        Some(Timestamp(now.0.saturating_add(ttl_ms))),
                    )
                }
                SensitiveHandling::Store => (
                    Sensitivity {
                        level: SensitivityLevel::None,
                        categories: found.categories.clone(),
                    },
                    None,
                ),
            }
        };
        let hash = canonical_hash(classified.kind, &reps, None);
        let outcome = self.storage.upsert_clip(NewClip {
            classified,
            hash,
            reps,
            source,
            sensitivity,
            expires_at,
            thumbnail_png: None,
        });
        match outcome {
            Ok(UpsertOutcome::Inserted(id)) => {
                self.emit(AppEvent::ClipAdded(id));
                self.retention_if_over_limit();
            }
            Ok(UpsertOutcome::Bumped(id)) => self.emit(AppEvent::ClipBumped(id)),
            Err(_) => self.emit(AppEvent::Unavailable("storage write failed")),
        }
    }

    fn discard(
        &self,
        reps: &mut [RawRepresentation],
        preview: &mut String,
        plain: &mut Option<String>,
    ) {
        wipe_reps(reps);
        wipe_string(preview);
        if let Some(t) = plain.as_mut() {
            wipe_string(t);
        }
    }

    fn retention_if_over_limit(&mut self) {
        let Ok(st) = self.storage.stats() else {
            return;
        };
        let r = &self.retention;
        let over_items = r.max_items > 0 && st.clips > u64::from(r.max_items);
        let limit = u64::from(r.max_storage_mb).saturating_mul(1024 * 1024);
        let over_size = r.max_storage_mb > 0 && st.db_bytes.saturating_add(st.blob_bytes) > limit;
        if over_items || over_size {
            self.run_retention();
        }
    }

    fn run_retention(&mut self) {
        if self.storage.apply_retention(&self.retention).is_err() {
            self.emit(AppEvent::Unavailable("retention failed"));
        }
        let now = Instant::now();
        self.next_retention = now.checked_add(RETENTION_EVERY).unwrap_or(now);
    }

    fn capture_blocked(&self) -> bool {
        // Any sequence number that is not our last write: only settings decide.
        let seq = self.last_self_write_seq.map_or(0, |s| s.wrapping_add(1));
        self.gate(seq).is_some()
    }

    fn on_command(&mut self, cmd: ActorCommand) {
        match cmd {
            ActorCommand::CopyBack { id, plain, reply } => {
                let r = self.copy_back(&id, plain);
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

    fn copy_back(&mut self, id: &ClipId, plain: bool) -> Result<(), ApiError> {
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
        let seq = written.map_err(|e| map_platform(&e))?;
        // Rule 5: exactly this change is ours and must not be captured again.
        self.last_self_write_seq = Some(seq);
        self.storage.mark_used(id).map_err(|e| map_storage(&e))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use darkclip_core::markers::{is_mandatory, PrivacyMarker};
    use darkclip_core::policy::SkipReason;
    use darkclip_core::sensitive::{masked_preview, scan};
    use darkclip_core::{
        canonical_hash, classify, ClipType, ClipboardSource, Lang, Mime, Platform,
        RawRepresentation, SensitiveCategory, SensitiveHandling, Sensitivity, SensitivityLevel,
        SourceConfidence,
    };
    use darkclip_platform::fake::{FakeCopy, FakeHandle, FakePlatform};
    use darkclip_platform::PlatformFormat;
    use darkclip_storage::{NewClip, OpenMode, SearchQuery};
    use std::sync::atomic::{AtomicI64, Ordering};
    use std::sync::mpsc::Receiver;
    use std::time::Instant;

    const T0: i64 = 1_795_320_000_000;
    const WAIT: Duration = Duration::from_secs(5);

    struct TestClock(AtomicI64);

    impl TestClock {
        fn set(&self, ms: i64) {
            self.0.store(ms, Ordering::SeqCst);
        }
    }

    impl Clock for TestClock {
        fn now(&self) -> Timestamp {
            Timestamp(self.0.load(Ordering::SeqCst))
        }
    }

    fn test_cfg() -> CaptureConfig {
        CaptureConfig {
            debounce_ms: 5,
            open_retries: 10,
            retry_delay_ms: 1,
        }
    }

    fn caps() -> SizeCaps {
        SizeCaps {
            text_max_bytes: 1024 * 1024,
            image_max_bytes: 20 * 1024 * 1024,
        }
    }

    /// Random-looking key body, assembled at runtime (GC-09).
    fn body(n: usize) -> String {
        let pool = b"aB3dE5gH7jK9mN1pQ2rS4tU6vW8xY0z";
        (0..n)
            .map(|i| char::from(pool[i % pool.len()]))
            .collect::<String>()
    }

    fn api_key_text() -> String {
        format!("key {}{}", concat!("s", "k-"), body(48))
    }

    fn pf(name: &str, mime: Option<Mime>, len: usize) -> PlatformFormat {
        PlatformFormat {
            name: name.to_string(),
            mime,
            size_hint: Some(len as u64),
        }
    }

    fn text_copy(s: &str) -> FakeCopy {
        FakeCopy {
            formats: vec![(
                pf("CF_UNICODETEXT", Some(Mime::TextPlain), s.len()),
                s.as_bytes().to_vec(),
            )],
            markers: Vec::new(),
            source: None,
        }
    }

    fn src(app_id: &str) -> ClipboardSource {
        ClipboardSource {
            app_id: app_id.to_string(),
            app_name: None,
            platform: Platform::Windows,
            confidence: SourceConfidence::Owner,
        }
    }

    struct H {
        dir: tempfile::TempDir,
        fake: FakeHandle,
        cap: Option<CaptureHandle>,
        events: Receiver<AppEvent>,
        clock: Arc<TestClock>,
    }

    fn start(policy: PrivacyPolicy) -> H {
        start_with(policy, RetentionPolicy::default(), test_cfg(), |_| {})
    }

    fn start_with(
        policy: PrivacyPolicy,
        retention: RetentionPolicy,
        cfg: CaptureConfig,
        seed: impl FnOnce(&mut Storage),
    ) -> H {
        let dir = tempfile::tempdir().unwrap();
        let clock = Arc::new(TestClock(AtomicI64::new(T0)));
        let dyn_clock: Arc<dyn Clock> = clock.clone();
        let mut storage =
            Storage::open(dir.path(), OpenMode::ReadWrite, Arc::clone(&dyn_clock)).unwrap();
        seed(&mut storage);
        let (platform, fake) = FakePlatform::new();
        let (tx, events) = mpsc::channel();
        let cap = CaptureHandle::spawn(
            platform.bundle(),
            storage,
            dyn_clock,
            cfg,
            policy,
            retention,
            tx,
        )
        .unwrap();
        H {
            dir,
            fake,
            cap: Some(cap),
            events,
            clock,
        }
    }

    impl H {
        fn cap(&self) -> &CaptureHandle {
            self.cap.as_ref().unwrap()
        }

        fn ro(&self) -> Storage {
            let clock: Arc<dyn Clock> = self.clock.clone();
            Storage::open(self.dir.path(), OpenMode::ReadOnly, clock).unwrap()
        }

        fn clip_count(&self) -> u64 {
            self.ro().stats().unwrap().clips
        }

        /// Waits for the first event matching `pred`; returns it and the events seen before it.
        fn wait_for(
            &self,
            what: &str,
            pred: impl Fn(&AppEvent) -> bool,
        ) -> (AppEvent, Vec<AppEvent>) {
            let deadline = Instant::now() + WAIT;
            let mut seen = Vec::new();
            loop {
                let left = deadline.saturating_duration_since(Instant::now());
                match self.events.recv_timeout(left) {
                    Ok(e) if pred(&e) => return (e, seen),
                    Ok(e) => seen.push(e),
                    Err(_) => panic!("timed out waiting for {what}; saw {seen:?}"),
                }
            }
        }

        fn wait_added(&self) -> (ClipId, Vec<AppEvent>) {
            match self.wait_for("ClipAdded", |e| matches!(e, AppEvent::ClipAdded(_))) {
                (AppEvent::ClipAdded(id), seen) => (id, seen),
                _ => unreachable!(),
            }
        }

        fn wait_skip(&self, reason: SkipReason) -> Vec<AppEvent> {
            self.wait_for("Skipped", |e| *e == AppEvent::Skipped(reason))
                .1
        }

        fn set_mode(&self, mode: CaptureMode) {
            self.cap().send(ActorCommand::SetMode(mode)).unwrap();
            self.wait_for("ModeChanged", |e| *e == AppEvent::ModeChanged(mode));
        }

        fn copy_back(&self, id: ClipId, plain: bool) -> Result<(), ApiError> {
            let (tx, rx) = mpsc::channel();
            self.cap()
                .send(ActorCommand::CopyBack {
                    id,
                    plain,
                    reply: tx,
                })
                .unwrap();
            rx.recv_timeout(WAIT).unwrap()
        }

        /// A new copy and its `ClipAdded`: proves every earlier change was fully processed.
        fn sentinel(&self) -> Vec<AppEvent> {
            self.fake.copy(text_copy("sentinel clip"));
            self.wait_added().1
        }
    }

    impl Drop for H {
        fn drop(&mut self) {
            if let Some(cap) = self.cap.take() {
                let _ = cap.shutdown(Duration::from_secs(2));
            }
        }
    }

    fn no_store_events(events: &[AppEvent]) -> bool {
        !events
            .iter()
            .any(|e| matches!(e, AppEvent::ClipAdded(_) | AppEvent::ClipBumped(_)))
    }

    #[test]
    fn stores_plain_text() {
        let h = start(PrivacyPolicy::default());
        h.fake.copy(text_copy("hello world"));
        let (id, _) = h.wait_added();
        let ro = h.ro();
        let clip = ro.get_clip(&id).unwrap().unwrap();
        assert_eq!(clip.kind, ClipType::Text);
        assert_eq!(clip.preview, "hello world");
        assert_eq!(clip.copy_count, 1);
        assert_eq!(clip.created_at, Timestamp(T0));
        assert_eq!(ro.read_representation(&id, 0).unwrap(), b"hello world");
        assert_eq!(h.fake.read_calls(), 1);
    }

    #[test]
    fn burst_same_seq_stores_once() {
        let h = start(PrivacyPolicy::default());
        h.fake.copy(text_copy("burst text"));
        h.fake.signal_again(19);
        let (id, before) = h.wait_added();
        assert!(no_store_events(&before), "{before:?}");
        let between = h.sentinel();
        assert!(no_store_events(&between), "{between:?}");
        assert_eq!(h.clip_count(), 2);
        assert_eq!(h.ro().get_clip(&id).unwrap().unwrap().copy_count, 1);
        assert_eq!(h.fake.read_calls(), 2);
    }

    #[test]
    fn ole_three_phase_keeps_first_source() {
        let h = start(PrivacyPolicy::default());
        // Phase 1: delayed rendering, placeholders only, owner known.
        h.fake.copy(FakeCopy {
            formats: vec![
                (pf("DataObject", None, 8), Vec::new()),
                (pf("Ole Private Data", None, 8), Vec::new()),
            ],
            markers: Vec::new(),
            source: Some(src("winword.exe")),
        });
        // Sync point: the actor has seen phase 1 (nothing readable yet). Without it the
        // test thread could clear the owner before the actor dequeues the first signal.
        let seen = h.wait_skip(SkipReason::NothingReadable);
        assert!(no_store_events(&seen), "{seen:?}");
        assert_eq!(h.fake.read_calls(), 0);
        // Phase 2: real formats under the same seq.
        h.fake.render(vec![
            (pf("DataObject", None, 8), Vec::new()),
            (
                pf("CF_UNICODETEXT", Some(Mime::TextPlain), 8),
                b"ole text".to_vec(),
            ),
        ]);
        // Phase 3: flush, owner gone.
        h.fake.clear_source();
        h.fake.signal_again(1);
        let (id, before) = h.wait_added();
        assert!(no_store_events(&before), "{before:?}");
        let clip = h.ro().get_clip(&id).unwrap().unwrap();
        assert_eq!(
            clip.source.as_ref().map(|s| s.app_id.as_str()),
            Some("winword.exe")
        );
        let between = h.sentinel();
        assert!(no_store_events(&between), "{between:?}");
        assert_eq!(h.clip_count(), 2);
    }

    #[test]
    fn excluded_never_read() {
        let h = start(PrivacyPolicy::default());
        let markers = [
            PrivacyMarker::OsConcealed {
                name: "ExcludeClipboardContentFromMonitorProcessing",
            },
            PrivacyMarker::OsConcealed {
                name: "CanIncludeInClipboardHistory",
            },
            PrivacyMarker::OsConcealed {
                name: "org.nspasteboard.ConcealedType",
            },
            PrivacyMarker::DarkshotExcluded,
            PrivacyMarker::PasswordManagerHint,
        ];
        for m in markers {
            assert!(is_mandatory(&m));
            let mut c = text_copy("hunter2 is my password");
            c.markers = vec![PrivacyMarker::NoCloud, m];
            h.fake.copy(c);
            let (ev, seen) = h.wait_for("Skipped", |e| matches!(e, AppEvent::Skipped(_)));
            assert!(
                matches!(
                    ev,
                    AppEvent::Skipped(SkipReason::OsConcealed | SkipReason::DarkshotExcluded)
                ),
                "{m:?} -> {ev:?}"
            );
            assert!(no_store_events(&seen), "{seen:?}");
            assert_eq!(h.fake.read_calls(), 0, "{m:?}");
        }
        assert_eq!(h.clip_count(), 0);
        let c = h.cap().skip_counters();
        assert_eq!(
            c.get(SkipReason::OsConcealed) + c.get(SkipReason::DarkshotExcluded),
            5
        );
    }

    #[test]
    fn paused_never_reads_formats() {
        let h = start(PrivacyPolicy::default());
        h.set_mode(CaptureMode::Paused {
            until: Timestamp(T0 + 60_000),
        });
        h.fake.copy(text_copy("copied while paused"));
        h.wait_skip(SkipReason::Paused);
        assert_eq!(h.fake.snapshot_meta_calls(), 0);
        assert_eq!(h.fake.read_calls(), 0);
        h.set_mode(CaptureMode::Private);
        h.fake.copy(text_copy("copied in private mode"));
        h.wait_skip(SkipReason::Private);
        assert_eq!(h.fake.snapshot_meta_calls(), 0);
        assert_eq!(h.fake.read_calls(), 0);
        assert_eq!(h.clip_count(), 0);
    }

    #[test]
    fn resume_does_not_backfill() {
        let h = start(PrivacyPolicy::default());
        // Explicit resume.
        h.set_mode(CaptureMode::Paused {
            until: Timestamp(T0 + 60_000),
        });
        h.fake.copy(text_copy("copied while paused"));
        h.wait_skip(SkipReason::Paused);
        h.set_mode(CaptureMode::Normal);
        // Spurious notifications for the paused change must not capture it.
        h.fake.signal_again(3);
        // Resume by deadline.
        h.set_mode(CaptureMode::Paused {
            until: Timestamp(T0 + 1_000),
        });
        h.fake.copy(text_copy("copied before the deadline"));
        h.wait_skip(SkipReason::Paused);
        h.clock.set(T0 + 2_000);
        h.fake.signal_again(2);
        let between = h.sentinel();
        assert!(no_store_events(&between), "{between:?}");
        assert_eq!(h.clip_count(), 1);
        assert_eq!(h.fake.snapshot_meta_calls(), 1);
        assert_eq!(h.fake.read_calls(), 1);
        let ro = h.ro();
        let (rows, _) = ro
            .search(&SearchQuery {
                text: None,
                kinds: Vec::new(),
                pinned_only: false,
                since: None,
                include_masked: true,
                limit: 10,
                cursor: None,
            })
            .unwrap();
        assert_eq!(rows.len(), 1);
        let clip = ro.get_clip(&rows[0].id).unwrap().unwrap();
        assert_eq!(clip.preview, "sentinel clip");
    }

    #[test]
    fn self_write_not_recaptured() {
        let h = start(PrivacyPolicy::default());
        h.fake.copy(text_copy("copy me back"));
        let (id, _) = h.wait_added();
        h.copy_back(id, false).unwrap();
        let seen = h.wait_skip(SkipReason::SelfWrite);
        assert!(no_store_events(&seen), "{seen:?}");
        let cur = h.fake.current();
        assert_eq!(cur.len(), 1);
        assert_eq!(cur[0].bytes, b"copy me back");
        assert!(!cur[0].truncated);
        let between = h.sentinel();
        assert!(no_store_events(&between), "{between:?}");
        let clip = h.ro().get_clip(&id).unwrap().unwrap();
        assert_eq!(clip.use_count, 1);
        assert_eq!(clip.copy_count, 1);
        assert_eq!(h.clip_count(), 2);
        assert_eq!(h.fake.read_calls(), 2);
    }

    #[test]
    fn copy_back_plain_writes_only_text() {
        let h = start(PrivacyPolicy::default());
        h.fake.copy(FakeCopy {
            formats: vec![
                (
                    pf("CF_UNICODETEXT", Some(Mime::TextPlain), 4),
                    b"bold".to_vec(),
                ),
                (
                    pf("HTML Format", Some(Mime::TextHtml), 11),
                    b"<b>bold</b>".to_vec(),
                ),
            ],
            markers: Vec::new(),
            source: None,
        });
        let (id, _) = h.wait_added();
        assert_eq!(
            h.ro().get_clip(&id).unwrap().unwrap().kind,
            ClipType::RichText
        );
        h.copy_back(id, true).unwrap();
        h.wait_skip(SkipReason::SelfWrite);
        let cur = h.fake.current();
        assert_eq!(cur.len(), 1);
        assert_eq!(cur[0].mime, Mime::TextPlain);
        assert_eq!(cur[0].bytes, b"bold");
        h.copy_back(id, false).unwrap();
        h.wait_skip(SkipReason::SelfWrite);
        assert_eq!(h.fake.current().len(), 2);
    }

    #[test]
    fn copy_back_unknown_clip_is_not_found() {
        let h = start(PrivacyPolicy::default());
        let id = ClipId::new_v7(Timestamp(T0));
        assert_eq!(h.copy_back(id, false), Err(ApiError::NotFound));
        assert!(h.fake.current().is_empty());
    }

    #[test]
    fn sensitive_drop_default() {
        let h = start(PrivacyPolicy::default());
        assert_eq!(
            PrivacyPolicy::default().sensitive_handling,
            SensitiveHandling::Drop
        );
        h.fake.copy(text_copy(&api_key_text()));
        let seen = h.wait_skip(SkipReason::Sensitive);
        assert!(no_store_events(&seen), "{seen:?}");
        let between = h.sentinel();
        assert!(no_store_events(&between), "{between:?}");
        assert_eq!(h.clip_count(), 1);
        assert_eq!(h.cap().skip_counters().get(SkipReason::Sensitive), 1);
    }

    #[test]
    fn sensitive_store_masked_sets_expiry_and_masked_preview() {
        let policy = PrivacyPolicy {
            sensitive_handling: SensitiveHandling::StoreMasked,
            ..PrivacyPolicy::default()
        };
        let enabled = policy.sensitive_categories_enabled.clone();
        let h = start(policy);
        let text = api_key_text();
        h.fake.copy(text_copy(&text));
        let (id, _) = h.wait_added();
        let ro = h.ro();
        let clip = ro.get_clip(&id).unwrap().unwrap();
        assert_eq!(clip.sensitivity.level, SensitivityLevel::Masked);
        assert!(clip
            .sensitivity
            .categories
            .contains(&SensitiveCategory::ApiKey));
        let ttl = i64::from(RetentionPolicy::default().sensitive_ttl_seconds);
        assert_eq!(clip.expires_at, Some(Timestamp(T0 + ttl * 1000)));
        let hint = scan(&text, &enabled).mask_hint.unwrap();
        assert_eq!(clip.preview, masked_preview(&hint, Lang::En));
        assert!(!clip.preview.contains(&body(48)));
        // The FTS index holds only the masked preview.
        let (rows, _) = ro
            .search(&SearchQuery {
                text: Some(body(20)),
                kinds: Vec::new(),
                pinned_only: false,
                since: None,
                include_masked: true,
                limit: 10,
                cursor: None,
            })
            .unwrap();
        assert!(rows.is_empty());
    }

    #[test]
    fn busy_retries_then_drops() {
        let h = start(PrivacyPolicy::default());
        h.fake.set_busy(11);
        h.fake.copy(text_copy("busy clipboard"));
        let seen = h.wait_skip(SkipReason::Busy);
        assert!(no_store_events(&seen), "{seen:?}");
        assert_eq!(h.cap().skip_counters().get(SkipReason::Busy), 1);
        assert_eq!(h.fake.snapshot_meta_calls(), 11);
        assert_eq!(h.fake.read_calls(), 0);
        // Still alive.
        h.sentinel();
        assert_eq!(h.clip_count(), 1);
    }

    #[test]
    fn shutdown_within_deadline() {
        let mut h = start(PrivacyPolicy::default());
        h.fake.copy(text_copy("before shutdown"));
        h.wait_added();
        let cap = h.cap.take().unwrap();
        let t = Instant::now();
        cap.shutdown(Duration::from_secs(2)).unwrap();
        assert!(t.elapsed() < Duration::from_secs(2));
        // The monitor was stopped: a later copy is not delivered anywhere.
        h.fake.copy(text_copy("after shutdown"));
        assert_eq!(h.clip_count(), 1);
    }

    #[test]
    fn kind_disabled_after_classification_dropped() {
        let policy = PrivacyPolicy {
            capture_kinds: vec![ClipType::Url],
            ..PrivacyPolicy::default()
        };
        let h = start(policy);
        h.fake.copy(text_copy("just some ordinary prose"));
        let seen = h.wait_skip(SkipReason::KindDisabled);
        assert!(no_store_events(&seen), "{seen:?}");
        // Text/plain can become a URL, so the policy allowed the read; the drop is post-read.
        assert_eq!(h.fake.read_calls(), 1);
        assert_eq!(h.clip_count(), 0);
        h.fake.copy(text_copy("https://example.com/page"));
        let (id, _) = h.wait_added();
        assert_eq!(h.ro().get_clip(&id).unwrap().unwrap().kind, ClipType::Url);
    }

    #[test]
    fn paused_passes_no_formats_to_policy() {
        // The gate decides from settings alone (empty markers and formats).
        let now = Timestamp(T0);
        let paused = PrivacyPolicy {
            mode: CaptureMode::Paused {
                until: Timestamp(T0 + 1),
            },
            ..PrivacyPolicy::default()
        };
        assert_eq!(
            gate_without_formats(&paused, now, 5, None, caps()),
            Some(Decision::Skip(SkipReason::Paused))
        );
        let private = PrivacyPolicy {
            mode: CaptureMode::Private,
            ..PrivacyPolicy::default()
        };
        assert_eq!(
            gate_without_formats(&private, now, 5, None, caps()),
            Some(Decision::Skip(SkipReason::Private))
        );
        let normal = PrivacyPolicy::default();
        assert_eq!(gate_without_formats(&normal, now, 5, None, caps()), None);
        assert_eq!(
            gate_without_formats(&normal, now, 5, Some(5), caps()),
            Some(Decision::Skip(SkipReason::SelfWrite))
        );

        // In the actor: a change while private, even with markers, never reaches snapshot_meta.
        let h = start(private);
        let mut c = text_copy("private text");
        c.markers = vec![PrivacyMarker::PasswordManagerHint];
        h.fake.copy(c);
        h.wait_skip(SkipReason::Private);
        h.set_mode(CaptureMode::Paused {
            until: Timestamp(T0 + 60_000),
        });
        h.fake.copy(text_copy("paused text"));
        h.wait_skip(SkipReason::Paused);
        assert_eq!(h.fake.snapshot_meta_calls(), 0);
        assert_eq!(h.fake.read_calls(), 0);
    }

    #[test]
    fn retention_after_insert_over_limit() {
        let retention = RetentionPolicy {
            max_items: 2,
            ..RetentionPolicy::default()
        };
        let h = start_with(PrivacyPolicy::default(), retention, test_cfg(), |_| {});
        for s in ["one", "two", "three"] {
            h.fake.copy(text_copy(s));
            h.wait_added();
        }
        assert_eq!(h.clip_count(), 2);
    }

    #[test]
    fn retention_runs_on_start() {
        let seed = |s: &mut Storage| {
            let reps = vec![RawRepresentation {
                mime: Mime::TextPlain,
                platform_format: "CF_UNICODETEXT".into(),
                bytes: b"expired masked".to_vec(),
                width: None,
                height: None,
                truncated: false,
            }];
            let classified = classify(&reps);
            let hash = canonical_hash(classified.kind, &reps, None);
            s.upsert_clip(NewClip {
                classified,
                hash,
                reps,
                source: None,
                sensitivity: Sensitivity {
                    level: SensitivityLevel::Masked,
                    categories: vec![SensitiveCategory::ApiKey],
                },
                expires_at: Some(Timestamp(T0 - 1)),
                thumbnail_png: None,
            })
            .unwrap();
        };
        let h = start_with(
            PrivacyPolicy::default(),
            RetentionPolicy::default(),
            test_cfg(),
            seed,
        );
        h.sentinel();
        assert_eq!(h.clip_count(), 1);
    }

    #[test]
    fn events_and_errors_carry_no_content() {
        let id = ClipId::new_v7(Timestamp(T0));
        let dbg = format!(
            "{:?} {:?} {:?}",
            AppEvent::ClipAdded(id),
            AppEvent::Skipped(SkipReason::Sensitive),
            ApiError::InvalidArgument("clip has no plain text")
        );
        assert!(dbg.contains("ClipAdded") && dbg.contains("Sensitive"));
    }
}
