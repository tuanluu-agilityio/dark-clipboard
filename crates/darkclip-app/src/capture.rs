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
//! `classify` → kind check → sensitive scan → decode and hash → store. This module never logs (GC-06):
//! events carry ids, skip reasons and static text only.

use crate::config::CaptureConfig;
use crate::events::{
    map_storage, ApiError, AppEvent, SkipCounters, UNAVAILABLE_READ, UNAVAILABLE_RETENTION,
    UNAVAILABLE_STORAGE_WRITE,
};
use crate::image;
use darkclip_core::policy::{
    decide_pre_read, AvailableFormat, Decision, PreReadInput, SizeCaps, SkipReason,
};
use darkclip_core::sensitive::{masked_preview, scan, MaskHint, ScanResult, MAX_SCAN_BYTES};
use darkclip_core::{
    canonical_hash, classify, CaptureMode, ClipId, ClipType, ClipboardSource, Clock, Lang, Mime,
    PrivacyPolicy, RawRepresentation, RetentionPolicy, SensitiveCategory, SensitiveHandling,
    Sensitivity, SensitivityLevel, Timestamp,
};
use darkclip_platform::{
    ChangeSignal, ClipboardAccess, ClipboardMeta, ClipboardMonitor, PlatformBundle, PlatformError,
    SourceResolver,
};
use darkclip_storage::{NewClip, Storage, StorageError, UpsertOutcome};
#[cfg(test)]
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex, PoisonError};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

/// Retention runs on start, after an insert that exceeds a limit, and at this interval.
const RETENTION_EVERY: Duration = Duration::from_secs(10 * 60);

/// Longest wait from the first signal of a burst, so a noisy writer cannot starve capture.
const MAX_BURST_WAIT: Duration = Duration::from_millis(500);

#[derive(Debug)]
///
/// Commands with a `reply` carry a `deadline`: the actor skips one that is already late, with
/// no side effect, because its caller has given up. A caller that times out while the actor
/// is executing the command cannot tell whether it ran: the outcome is unknown.
pub enum ActorCommand {
    /// Writes a stored clip back to the clipboard (`plain`: text/plain only).
    CopyBack {
        id: ClipId,
        plain: bool,
        deadline: Instant,
        reply: Sender<Result<(), ApiError>>,
    },
    SetMode(CaptureMode),
    /// Language for masked previews stored from now on.
    SetLang(Lang),
    /// Pin or unpin. Masked clips are rejected (pinning would not extend their expiry).
    SetPinned {
        id: ClipId,
        pinned: bool,
        deadline: Instant,
        reply: Sender<Result<(), ApiError>>,
    },
    DeleteClip {
        id: ClipId,
        deadline: Instant,
        reply: Sender<Result<(), ApiError>>,
    },
    /// Deletes the history; replies with the number of clips removed.
    ClearHistory {
        keep_pinned: bool,
        deadline: Instant,
        reply: Sender<Result<u64, ApiError>>,
    },
    UpdatePolicy(PrivacyPolicy, RetentionPolicy),
    /// Processes every command queued before it, stops the monitor and ends the actor.
    Shutdown {
        reply: Sender<()>,
    },
}

/// Retention instrumentation: an attempt counter, and (tests only) forced failure.
#[derive(Default)]
struct RetentionHooks {
    #[cfg(test)]
    attempts: AtomicU32,
    #[cfg(test)]
    fail: std::sync::atomic::AtomicBool,
    /// Tests: errors the next `upsert_clip` calls return, in order, before touching storage.
    #[cfg(test)]
    inject: Mutex<std::collections::VecDeque<StorageError>>,
    /// Tests: `next_expiry` fails after a successful retention.
    #[cfg(test)]
    expiry_fail: std::sync::atomic::AtomicBool,
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
    #[cfg(test)]
    hooks: Arc<RetentionHooks>,
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
        Self::spawn_inner(
            bundle,
            storage,
            clock,
            cfg,
            policy,
            retention,
            events,
            MAX_BURST_WAIT,
        )
    }

    /// `spawn` with the burst cap as a parameter (tests shorten it).
    #[allow(clippy::too_many_arguments)]
    fn spawn_inner(
        bundle: PlatformBundle,
        storage: Storage,
        clock: Arc<dyn Clock>,
        cfg: CaptureConfig,
        policy: PrivacyPolicy,
        retention: RetentionPolicy,
        events: Sender<AppEvent>,
        max_burst_wait: Duration,
    ) -> Result<Self, ApiError> {
        let (tx, rx) = mpsc::channel::<Msg>();
        let (ready_tx, ready_rx) = mpsc::channel::<Result<(), ApiError>>();
        let counters = Arc::new(Mutex::new(SkipCounters::default()));
        let hooks = Arc::new(RetentionHooks::default());
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
            max_burst_wait,
            policy,
            retention,
            events,
            counters: Arc::clone(&counters),
            hooks: Arc::clone(&hooks),
            last_processed_seq: None,
            last_self_write_seq: None,
            pending: None,
            carried: None,
            next_retention: Instant::now(),
            next_expiry: None,
            retention_retry_at: None,
            retention_backoff: None,
        };
        let fwd_tx = tx.clone();
        #[cfg(not(test))]
        drop(hooks);
        let join = std::thread::Builder::new()
            .name("darkclip-capture".into())
            .spawn(move || actor.start(fwd_tx, rx, ready_tx))
            .map_err(|_| ApiError::Internal)?;
        match ready_rx.recv() {
            Ok(Ok(())) => Ok(Self {
                tx: Some(tx),
                join: Some(join),
                counters,
                #[cfg(test)]
                hooks,
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

/// `decide_pre_read` with the primary source, then the excluded-app check against every
/// other source seen for the same change: if any of them is excluded, the change is skipped.
/// Steps before the app check do not depend on the source, so precedence is unchanged.
fn decide_all_sources(input: &PreReadInput<'_>, others: &[&ClipboardSource]) -> Decision {
    let d = decide_pre_read(input);
    for other in others {
        let alt = decide_pre_read(&PreReadInput {
            source: Some(*other),
            ..*input
        });
        if alt == Decision::Skip(SkipReason::ExcludedApp) {
            return alt;
        }
    }
    d
}

/// Sensitive scan of the clip's text: the plain text, or, when that is empty, the HTML/RTF
/// bodies decoded as lossy UTF-8 (at most `MAX_SCAN_BYTES` each, like the plain text).
fn scan_clip(
    plain: Option<&str>,
    reps: &[RawRepresentation],
    enabled: &[SensitiveCategory],
) -> ScanResult {
    if let Some(t) = plain.filter(|t| !t.trim().is_empty()) {
        return scan(t, enabled);
    }
    let mut out = ScanResult::default();
    for r in reps
        .iter()
        .filter(|r| matches!(r.mime, Mime::TextHtml | Mime::TextRtf))
    {
        let n = r.bytes.len().min(MAX_SCAN_BYTES);
        let mut text = String::from_utf8_lossy(r.bytes.get(..n).unwrap_or_default()).into_owned();
        let found = scan(&text, enabled);
        wipe_string(&mut text);
        for c in found.categories {
            if !out.categories.contains(&c) {
                out.categories.push(c);
            }
        }
        if out.mask_hint.is_none() {
            out.mask_hint = found.mask_hint;
        }
    }
    out
}

/// Hash and optional thumbnail of a clip about to be stored.
struct Hashed {
    hash: [u8; 32],
    thumbnail: Option<Vec<u8>>,
    /// Dimensions that were missing on the decoded rep and have been filled in.
    filled_dims: Option<(u32, u32)>,
}

/// Canonical hash; for an image with a decodable PNG, over its pixels (so the same picture in
/// two encodings dedupes), plus a thumbnail (unless `want_thumbnail` is false) and missing
/// dimensions filled in on the decoded rep. A decode failure keeps the fallback hash over the
/// encoded bytes. The RGBA buffer is wiped and dropped here.
fn decode_and_hash(reps: &mut [RawRepresentation], kind: ClipType, want_thumbnail: bool) -> Hashed {
    if kind == ClipType::Image {
        let idx = reps
            .iter()
            .position(|r| r.mime == Mime::ImagePng && !r.truncated);
        let decoded = idx.and_then(|i| {
            image::decode_png(&reps[i].bytes, image::MAX_PIXELS, image::MAX_RGBA_BYTES).ok()
        });
        if let (Some(i), Some(mut img)) = (idx, decoded) {
            let hash = canonical_hash(kind, reps, Some((&img.rgba, img.width, img.height)));
            let thumb = if want_thumbnail {
                image::thumbnail(&img, image::THUMB_MAX_SIDE)
            } else {
                Vec::new()
            };
            wipe_bytes(&mut img.rgba);
            let mut filled_dims = None;
            if reps[i].width.is_none() || reps[i].height.is_none() {
                reps[i].width = Some(img.width);
                reps[i].height = Some(img.height);
                filled_dims = Some((img.width, img.height));
            }
            return Hashed {
                hash,
                thumbnail: (!thumb.is_empty()).then_some(thumb),
                filled_dims,
            };
        }
    }
    Hashed {
        hash: canonical_hash(kind, reps, None),
        thumbnail: None,
        filled_dims: None,
    }
}

/// Wait after the first failed retention; doubles per further failure up to the cap.
const RETENTION_BACKOFF_FIRST: Duration = Duration::from_secs(30);
const RETENTION_BACKOFF_MAX: Duration = Duration::from_secs(10 * 60);

fn next_backoff(prev: Option<Duration>) -> Duration {
    match prev {
        None => RETENTION_BACKOFF_FIRST,
        Some(d) => d.saturating_mul(2).min(RETENTION_BACKOFF_MAX),
    }
}

fn size_caps(r: &RetentionPolicy) -> SizeCaps {
    SizeCaps {
        text_max_bytes: u64::from(r.text_max_kb).saturating_mul(1024),
        image_max_bytes: u64::from(r.image_max_mb).saturating_mul(1024 * 1024),
    }
}

/// The caller of a reply-bearing command has stopped waiting.
fn late(deadline: Instant) -> bool {
    Instant::now() >= deadline
}

fn map_platform(e: &PlatformError) -> ApiError {
    match e {
        PlatformError::Busy => ApiError::ClipboardBusy,
        PlatformError::Unsupported(_) => ApiError::UnsupportedPlatform,
        PlatformError::PermissionDenied(what) => ApiError::PermissionDenied(what),
        PlatformError::TooLarge { .. } | PlatformError::Backend { .. } => ApiError::Internal,
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
    /// Every distinct source the signals for `seq` carried, in order; the first is the
    /// primary (the owner may be gone by the last signal).
    sources: Vec<ClipboardSource>,
    /// First signal of the burst, for the `max_burst_wait` cap.
    first_at: Instant,
    due: Instant,
    /// Signal time (`ChangeSignal::at`) of the burst's first signal.
    burst_signal_at: Timestamp,
    /// Signal time of this change's first signal.
    signal_at: Timestamp,
    /// Signal time of this change's latest signal.
    last_signal_at: Timestamp,
    /// F-14 rule 1: every source of the changes this burst superseded before they were
    /// inspected. Checked against the excluded apps when this change has no source of its own.
    superseded: Vec<ClipboardSource>,
    /// F-14 rule 2: the latest superseded change that had a source; the only candidate for
    /// the stored source of an ownerless OLE flush.
    heir: Option<Superseded>,
}

/// A change replaced by a newer seq before it was inspected.
struct Superseded {
    seq: u64,
    /// Its primary source.
    source: ClipboardSource,
    last_signal_at: Timestamp,
    /// A snapshot of this seq completed earlier (delayed rendering, `carried`).
    inspected: bool,
}

/// Largest seq distance between a superseded change and the ownerless flush that may inherit
/// its source (F-14 rule 2).
const MAX_HEIR_SEQ_GAP: u64 = 8;

/// Windows OLE placeholders; their presence in the final snapshot is the flush signature.
const OLE_SIGNATURE_FORMATS: [&str; 2] = ["DataObject", "Ole Private Data"];

/// The snapshot still offers an OLE placeholder (reported with no MIME).
fn has_ole_signature(meta: &ClipboardMeta) -> bool {
    meta.formats.iter().any(|f| {
        f.mime.is_none()
            && OLE_SIGNATURE_FORMATS
                .iter()
                .any(|n| f.name.eq_ignore_ascii_case(n))
    })
}

/// `later - earlier` in ms, if `later` is not before `earlier` and at most `max_ms` after it.
fn within_ms(earlier: Timestamp, later: Timestamp, max_ms: u64) -> bool {
    later
        .0
        .checked_sub(earlier.0)
        .and_then(|d| u64::try_from(d).ok())
        .is_some_and(|d| d <= max_ms)
}

struct Actor {
    monitor: Box<dyn ClipboardMonitor>,
    access: Box<dyn ClipboardAccess>,
    source: Box<dyn SourceResolver>,
    storage: Storage,
    clock: Arc<dyn Clock>,
    cfg: CaptureConfig,
    max_burst_wait: Duration,
    policy: PrivacyPolicy,
    retention: RetentionPolicy,
    events: Sender<AppEvent>,
    counters: Arc<Mutex<SkipCounters>>,
    /// Test instrumentation; unused in release builds.
    #[cfg_attr(not(test), allow(dead_code))]
    hooks: Arc<RetentionHooks>,
    last_processed_seq: Option<u64>,
    last_self_write_seq: Option<u64>,
    pending: Option<Pending>,
    /// Source of a change that had nothing readable yet (OLE delayed rendering), kept for a
    /// later burst with the same sequence number.
    carried: Option<(u64, Option<ClipboardSource>)>,
    next_retention: Instant,
    /// Earliest `expires_at` in storage (clock time); retention runs when it falls due so a
    /// masked clip does not outlive its TTL.
    next_expiry: Option<Timestamp>,
    /// After a failed retention: no expiry-driven retry before this instant.
    retention_retry_at: Option<Instant>,
    /// Current back-off step; `None` after a success.
    retention_backoff: Option<Duration>,
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

    fn in_backoff(&self) -> bool {
        self.retention_retry_at.is_some_and(|t| Instant::now() < t)
    }

    fn expiry_due(&self) -> bool {
        !self.in_backoff() && self.next_expiry.is_some_and(|t| t <= self.clock.now())
    }

    /// Real-time estimate of the next expiry (the clock may be a test clock: the loop also
    /// re-checks `expiry_due` after every message), never before the back-off ends.
    fn expiry_wake(&self) -> Instant {
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
        let now = Instant::now();
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
        // F-14: within the burst cap, the superseded change joins the chain (rule 1: its
        // sources are checked for exclusion; rule 2: it may name an ownerless flush's source).
        let in_cap = first_at
            .checked_add(self.max_burst_wait)
            .is_some_and(|cap| now <= cap);
        let (superseded, heir, burst_signal_at) = match prev {
            Some(p) if in_cap => self.supersede(p),
            _ => (Vec::new(), None, s.at),
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
            || self.carried.as_ref().is_some_and(|(s, _)| *s == p.seq);
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

    /// F-14 rule 2: the stored source of an ownerless change, taken from the change it
    /// superseded, only for the OLE flush shape: never inspected, same seq as the snapshot,
    /// at most `MAX_HEIR_SEQ_GAP` seqs and `debounce_ms` after it, inside the burst cap, no
    /// self-write in between, and the snapshot still offers an OLE placeholder. The caller
    /// asks only when the change has no source of its own.
    fn heir_source(&self, p: &Pending, meta: &ClipboardMeta) -> Option<ClipboardSource> {
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
                self.emit(AppEvent::Unavailable(UNAVAILABLE_READ));
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
        // Every source seen for this change; the first one seen is the primary.
        let mut seen: Vec<ClipboardSource> = Vec::new();
        for s in carried.iter().chain(&p.sources).chain(meta.source.as_ref()) {
            if !seen.contains(s) {
                seen.push(s.clone());
            }
        }
        // F-14, an ownerless change (e.g. an OLE flush): rule 2 may name the stored source;
        // rule 1 checks every source the burst superseded, never as the primary (only adds
        // skips).
        let ownerless = seen.is_empty();
        let mut seen = seen.into_iter();
        let source = if ownerless {
            self.heir_source(&p, &meta)
        } else {
            seen.next()
        };
        let mut others: Vec<ClipboardSource> = seen.collect();
        if ownerless {
            for s in &p.superseded {
                if source.as_ref() != Some(s) && !others.contains(s) {
                    others.push(s.clone());
                }
            }
        }
        let others: Vec<&ClipboardSource> = others.iter().collect();
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
        let decision = decide_all_sources(
            &PreReadInput {
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
            },
            &others,
        );
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
            self.counters
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .count_changed_during_read();
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
        let found = scan_clip(
            classified.plain_text.as_deref(),
            &reps,
            &self.policy.sensitive_categories_enabled,
        );
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
                            .unwrap_or(SensitiveCategory::Password),
                    });
                    let mut old = std::mem::replace(
                        &mut classified.preview,
                        masked_preview(&hint, self.cfg.lang),
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
        // Only now, after the policy decision and sensitive handling, are pixels decoded.
        let masked = sensitivity.level == SensitivityLevel::Masked;
        let hashed = decode_and_hash(&mut reps, classified.kind, !masked);
        if let (Some((w, h)), false) = (hashed.filled_dims, masked) {
            classified.preview = darkclip_core::image_preview(w, h);
        }
        let hash = hashed.hash;
        let thumbnail_png = hashed.thumbnail;
        let outcome = self.insert_with_retry(NewClip {
            classified,
            hash,
            reps,
            source,
            sensitivity,
            expires_at,
            thumbnail_png,
        });
        if outcome.is_ok() {
            if let Some(t) = expires_at {
                self.next_expiry = Some(self.next_expiry.map_or(t, |n| n.min(t)));
            }
        }
        match outcome {
            Ok(UpsertOutcome::Inserted(id)) => {
                // Limits first, so a listener never sees the store over its limits.
                self.retention_if_over_limit();
                self.emit(AppEvent::ClipAdded(id));
            }
            Ok(UpsertOutcome::Bumped(id)) => self.emit(AppEvent::ClipBumped(id)),
            Err(_) => self.emit(AppEvent::Unavailable(UNAVAILABLE_STORAGE_WRITE)),
        }
    }

    /// F-02: on `StorageFull` run retention once and retry the insert once with the same
    /// value. If the retry fails with any error the failure is counted and returned (event
    /// from the caller); the actor stays alive. The clip data is wiped afterwards.
    fn insert_with_retry(&mut self, mut new: NewClip) -> Result<UpsertOutcome, StorageError> {
        let mut result = self.try_upsert(&new);
        if result == Err(StorageError::StorageFull) {
            self.run_retention();
            result = self.try_upsert(&new);
            if result.is_err() {
                self.counters
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .count_storage_full();
            }
        }
        wipe_reps(&mut new.reps);
        wipe_string(&mut new.classified.preview);
        if let Some(t) = new.classified.plain_text.as_mut() {
            wipe_string(t);
        }
        if let Some(t) = new.thumbnail_png.as_mut() {
            wipe_bytes(t);
        }
        result
    }

    fn try_upsert(&mut self, new: &NewClip) -> Result<UpsertOutcome, StorageError> {
        #[cfg(test)]
        {
            let injected = self
                .hooks
                .inject
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .pop_front();
            if let Some(e) = injected {
                return Err(e);
            }
        }
        self.storage.upsert_clip(new)
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
        if (over_items || over_size) && !self.in_backoff() {
            self.run_retention();
        }
    }

    fn run_retention(&mut self) {
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
            lang: Lang::En,
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
        start_full(policy, retention, cfg, MAX_BURST_WAIT, seed)
    }

    fn start_full(
        policy: PrivacyPolicy,
        retention: RetentionPolicy,
        cfg: CaptureConfig,
        max_burst_wait: Duration,
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
        let cap = CaptureHandle::spawn_inner(
            platform.bundle(),
            storage,
            dyn_clock,
            cfg,
            policy,
            retention,
            tx,
            max_burst_wait,
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

        /// Previews of every stored clip (masked ones included).
        fn stored_previews(&self) -> Vec<String> {
            let (rows, _) = self
                .ro()
                .search(&SearchQuery {
                    text: None,
                    kinds: Vec::new(),
                    pinned_only: false,
                    since: None,
                    include_masked: true,
                    limit: 200,
                    cursor: None,
                })
                .unwrap();
            rows.into_iter().map(|r| r.preview).collect()
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
                    deadline: Instant::now() + WAIT,
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
    fn storage_full_runs_retention_then_retries_once() {
        let h = start(PrivacyPolicy::default());
        h.sentinel();
        let hooks = &h.cap().hooks;
        let before = hooks.attempts.load(Ordering::SeqCst);
        hooks
            .inject
            .lock()
            .unwrap()
            .push_back(StorageError::StorageFull);
        h.fake.copy(text_copy("second clip"));
        let (_, seen) = h.wait_added();
        assert!(
            !seen.iter().any(|e| matches!(e, AppEvent::Unavailable(_))),
            "{seen:?}"
        );
        assert_eq!(hooks.attempts.load(Ordering::SeqCst), before + 1);
        assert_eq!(h.clip_count(), 2);
        assert_eq!(h.cap().skip_counters().storage_full(), 0);
    }

    #[test]
    fn storage_full_twice_reports_unavailable() {
        let h = start(PrivacyPolicy::default());
        h.sentinel();
        let hooks = &h.cap().hooks;
        let before = hooks.attempts.load(Ordering::SeqCst);
        hooks
            .inject
            .lock()
            .unwrap()
            .extend([StorageError::StorageFull, StorageError::StorageFull]);
        h.fake.copy(text_copy("never stored"));
        let (_, seen) = h.wait_for("Unavailable", |e| {
            *e == AppEvent::Unavailable(UNAVAILABLE_STORAGE_WRITE)
        });
        assert!(no_store_events(&seen), "{seen:?}");
        // Exactly one retention run, one retry.
        assert_eq!(hooks.attempts.load(Ordering::SeqCst), before + 1);
        assert_eq!(h.cap().skip_counters().storage_full(), 1);
        assert_eq!(h.clip_count(), 1);
        // The actor is alive and stores again.
        h.fake.copy(text_copy("after the failure"));
        h.wait_added();
        assert_eq!(h.clip_count(), 2);
    }

    #[test]
    fn storage_full_then_other_retry_error_is_counted() {
        let h = start(PrivacyPolicy::default());
        h.sentinel();
        let hooks = &h.cap().hooks;
        hooks.inject.lock().unwrap().extend([
            StorageError::StorageFull,
            StorageError::Sqlite("Other".into()),
        ]);
        h.fake.copy(text_copy("never stored either"));
        let (_, seen) = h.wait_for("Unavailable", |e| {
            *e == AppEvent::Unavailable(UNAVAILABLE_STORAGE_WRITE)
        });
        assert!(no_store_events(&seen), "{seen:?}");
        assert_eq!(h.cap().skip_counters().storage_full(), 1);
        assert_eq!(h.clip_count(), 1);
    }

    #[test]
    fn non_full_insert_error_is_not_retried() {
        let h = start(PrivacyPolicy::default());
        h.sentinel();
        let hooks = &h.cap().hooks;
        let before = hooks.attempts.load(Ordering::SeqCst);
        hooks
            .inject
            .lock()
            .unwrap()
            .push_back(StorageError::Sqlite("Other".into()));
        h.fake.copy(text_copy("single failure"));
        h.wait_for("Unavailable", |e| {
            *e == AppEvent::Unavailable(UNAVAILABLE_STORAGE_WRITE)
        });
        assert_eq!(hooks.attempts.load(Ordering::SeqCst), before);
        assert_eq!(h.cap().skip_counters().storage_full(), 0);
    }

    #[test]
    fn shutdown_within_deadline() {
        let mut h = start(PrivacyPolicy::default());
        h.fake.copy(text_copy("before shutdown"));
        h.wait_added();
        let cap = h.cap.take().unwrap();
        // Queued before Shutdown: must be drained, not lost.
        cap.send(ActorCommand::SetMode(CaptureMode::Private))
            .unwrap();
        let t = Instant::now();
        cap.shutdown(Duration::from_secs(2)).unwrap();
        assert!(t.elapsed() < Duration::from_secs(2));
        // `shutdown` consumes the handle, so no later `send` can compile; the actor is gone:
        // the drained event arrived and then the event channel closed.
        assert_eq!(
            h.events.recv_timeout(WAIT),
            Ok(AppEvent::ModeChanged(CaptureMode::Private))
        );
        assert_eq!(
            h.events.recv_timeout(WAIT),
            Err(mpsc::RecvTimeoutError::Disconnected)
        );
        // The monitor was stopped: a later copy reaches nobody.
        let (meta, reads) = (h.fake.snapshot_meta_calls(), h.fake.read_calls());
        h.fake.copy(text_copy("after shutdown"));
        h.fake.signal_again(3);
        assert_eq!(h.fake.snapshot_meta_calls(), meta);
        assert_eq!(h.fake.read_calls(), reads);
        assert_eq!(h.clip_count(), 1);
    }

    fn slow_cfg() -> CaptureConfig {
        // Wide enough that two back-to-back copies land in one debounce window.
        CaptureConfig {
            debounce_ms: 100,
            ..test_cfg()
        }
    }

    fn excluded_copy(s: &str) -> FakeCopy {
        let mut c = text_copy(s);
        // In the default exclusion list; case differs on purpose (Windows match is insensitive).
        c.source = Some(src("KeePassXC.exe"));
        c
    }

    /// Runs a scenario whose steps must land in one debounce window. Under heavy load the
    /// window can close between two steps (detected by the attempt, which returns false);
    /// the attempt is then repeated with a fresh actor. Every attempt that lands in one
    /// window runs all its assertions.
    fn in_one_window(mut attempt: impl FnMut() -> bool) {
        for _ in 0..5 {
            if attempt() {
                return;
            }
        }
        panic!("the steps never landed in one debounce window");
    }

    #[test]
    fn excluded_app_after_other_copy_in_window_is_skipped() {
        in_one_window(|| {
            let h = start_with(
                PrivacyPolicy::default(),
                RetentionPolicy::default(),
                slow_cfg(),
                |_| {},
            );
            let mut c = text_copy("notepad text");
            c.source = Some(src("notepad.exe"));
            h.fake.copy(c);
            // The signal carries notepad.exe (resolved at signal time, F-01), so no sync with
            // the actor's dequeue is needed before the next copy.
            h.fake.copy(excluded_copy("vault entry"));
            let seen = h.wait_skip(SkipReason::ExcludedApp);
            // Every attempt, also one whose window closed early: the excluded copy is never stored.
            assert!(!h.stored_previews().iter().any(|p| p == "vault entry"));
            if h.fake.snapshot_meta_calls() > 1 {
                return false; // Window closed between the copies: notepad's was inspected alone.
            }
            assert!(no_store_events(&seen), "{seen:?}");
            assert_eq!(h.fake.read_calls(), 0);
            assert_eq!(h.clip_count(), 0);
            true
        });
    }

    #[test]
    fn other_copy_after_excluded_app_in_window_is_not_mistagged() {
        in_one_window(|| {
            let h = start_with(
                PrivacyPolicy::default(),
                RetentionPolicy::default(),
                slow_cfg(),
                |_| {},
            );
            // The signal carries the excluded owner; no sync with the dequeue is needed.
            h.fake.copy(excluded_copy("vault entry"));
            let mut c = text_copy("notepad text");
            c.source = Some(src("notepad.exe"));
            h.fake.copy(c);
            let (id, before) = h.wait_added();
            // Never mis-tagged, whether or not the window held.
            let clip = h.ro().get_clip(&id).unwrap().unwrap();
            assert_eq!(clip.preview, "notepad text");
            assert_eq!(
                clip.source.as_ref().map(|s| s.app_id.as_str()),
                Some("notepad.exe")
            );
            if h.fake.snapshot_meta_calls() > 1 {
                return false; // Window closed: the excluded change was inspected alone.
            }
            // One window: the excluded change was superseded, never inspected on its own.
            assert!(before.is_empty(), "{before:?}");
            true
        });
    }

    #[test]
    fn excluded_app_matches_any_source_of_the_change() {
        let policy = PrivacyPolicy::default();
        let (notepad, vault) = (src("notepad.exe"), src("KeePassXC.exe"));
        let available = [AvailableFormat {
            mime: Mime::TextPlain,
            platform_format: "CF_UNICODETEXT".into(),
            size_hint: Some(4),
        }];
        let decide = |source: Option<&ClipboardSource>,
                      markers: &[PrivacyMarker],
                      others: &[&ClipboardSource]| {
            decide_all_sources(
                &PreReadInput {
                    seq: 9,
                    last_self_write_seq: None,
                    mode: &policy.mode,
                    now: Timestamp(T0),
                    capture_enabled: true,
                    markers,
                    source,
                    excluded: &policy.excluded_apps,
                    available: &available,
                    enabled_kinds: &policy.capture_kinds,
                    caps: caps(),
                },
                others,
            )
        };
        assert_eq!(
            decide(Some(&notepad), &[], &[&vault]),
            Decision::Skip(SkipReason::ExcludedApp)
        );
        assert_eq!(
            decide(None, &[], &[&vault]),
            Decision::Skip(SkipReason::ExcludedApp)
        );
        // Precedence unchanged: a mandatory marker still reports as concealed.
        assert_eq!(
            decide(
                Some(&notepad),
                &[PrivacyMarker::PasswordManagerHint],
                &[&vault]
            ),
            Decision::Skip(SkipReason::OsConcealed)
        );
        assert!(matches!(
            decide(Some(&notepad), &[], &[]),
            Decision::Read(_)
        ));
    }

    #[test]
    fn excluded_source_seen_at_any_point_of_the_change_skips() {
        // The first-signal source is excluded, the snapshot's owner is not (or is gone).
        let h = start(PrivacyPolicy::default());
        // The signal carries the excluded owner (F-01); the owner may be gone at dequeue.
        h.fake.copy(excluded_copy("vault entry"));
        h.fake.clear_source();
        let seen = h.wait_skip(SkipReason::ExcludedApp);
        assert!(no_store_events(&seen), "{seen:?}");
        assert_eq!(h.fake.read_calls(), 0);
    }

    #[test]
    fn signal_source_is_primary_over_dequeue_resolve() {
        // The signal carries A (resolved on the monitor thread); the owner is B by the
        // time the actor dequeues and snapshots.
        let h = start(PrivacyPolicy::default());
        let mut c = text_copy("from app a");
        c.source = Some(src("a.exe"));
        h.fake.copy_then_set_source(c, Some(src("b.exe")));
        let (id, _) = h.wait_added();
        let clip = h.ro().get_clip(&id).unwrap().unwrap();
        assert_eq!(
            clip.source.as_ref().map(|s| s.app_id.as_str()),
            Some("a.exe")
        );
        // The signal had a source: no resolve at dequeue.
        assert_eq!(h.fake.resolve_calls(), 0);
        // B is still checked against excluded apps.
        let mut c = text_copy("vault entry");
        c.source = Some(src("a.exe"));
        h.fake.copy_then_set_source(c, Some(src("KeePassXC.exe")));
        let seen = h.wait_skip(SkipReason::ExcludedApp);
        assert!(no_store_events(&seen), "{seen:?}");
        assert_eq!(h.fake.read_calls(), 1);
        assert_eq!(h.fake.resolve_calls(), 0);
        assert_eq!(h.clip_count(), 1);
    }

    #[test]
    fn excluded_source_in_signal_skips() {
        // Only the signal knows the excluded owner; it is gone at dequeue and snapshot.
        let h = start(PrivacyPolicy::default());
        h.fake
            .copy_then_set_source(excluded_copy("vault entry"), None);
        let seen = h.wait_skip(SkipReason::ExcludedApp);
        assert!(no_store_events(&seen), "{seen:?}");
        assert_eq!(h.fake.read_calls(), 0);
        assert_eq!(h.fake.resolve_calls(), 0);
        assert_eq!(h.clip_count(), 0);
    }

    #[test]
    fn signal_without_source_falls_back_to_dequeue_resolve() {
        // Backends that cannot resolve at signal time: the actor resolves at dequeue.
        let h = start(PrivacyPolicy::default());
        h.fake
            .copy_then_set_source(text_copy("late owner"), Some(src("excel.exe")));
        let (id, _) = h.wait_added();
        assert!(h.fake.resolve_calls() >= 1);
        let clip = h.ro().get_clip(&id).unwrap().unwrap();
        assert_eq!(
            clip.source.as_ref().map(|s| s.app_id.as_str()),
            Some("excel.exe")
        );
    }

    #[test]
    fn signal_storm_is_capped() {
        let cfg = CaptureConfig {
            debounce_ms: 10,
            ..test_cfg()
        };
        let h = start_full(
            PrivacyPolicy::default(),
            RetentionPolicy::default(),
            cfg,
            Duration::from_millis(40),
            |_| {},
        );
        h.fake.copy(text_copy("noisy writer"));
        let stop = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let ended = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let storm = {
            let (fake, stop, ended) = (h.fake.clone(), Arc::clone(&stop), Arc::clone(&ended));
            std::thread::spawn(move || {
                // Same seq every 0.5 ms (busy-wait: OS sleeps may be coarser than the debounce).
                let start = Instant::now();
                while start.elapsed() < Duration::from_millis(300) {
                    if stop.load(Ordering::SeqCst) {
                        return;
                    }
                    fake.signal_again(1);
                    let t = Instant::now();
                    while t.elapsed() < Duration::from_micros(500) {
                        std::hint::spin_loop();
                    }
                }
                ended.store(true, Ordering::SeqCst);
            })
        };
        h.wait_added();
        let storm_was_running = !ended.load(Ordering::SeqCst);
        stop.store(true, Ordering::SeqCst);
        storm.join().unwrap();
        assert!(storm_was_running, "capture waited for the storm to end");
        assert_eq!(h.clip_count(), 1);
    }

    fn masked_policy() -> PrivacyPolicy {
        PrivacyPolicy {
            sensitive_handling: SensitiveHandling::StoreMasked,
            ..PrivacyPolicy::default()
        }
    }

    #[test]
    fn masked_clip_removed_at_expiry_not_ten_minutes_later() {
        let h = start(masked_policy());
        h.fake.copy(text_copy(&api_key_text()));
        let (id, _) = h.wait_added();
        let ttl = i64::from(RetentionPolicy::default().sensitive_ttl_seconds);
        h.clock.set(T0 + ttl * 1000);
        h.sentinel();
        assert!(h.ro().get_clip(&id).unwrap().is_none());
        assert_eq!(h.clip_count(), 1);
    }

    fn seed_masked(expires_at: i64) -> impl FnOnce(&mut Storage) {
        move |s: &mut Storage| {
            let reps = vec![RawRepresentation {
                mime: Mime::TextPlain,
                platform_format: "CF_UNICODETEXT".into(),
                bytes: b"masked seed".to_vec(),
                width: None,
                height: None,
                truncated: false,
            }];
            let classified = classify(&reps);
            let hash = canonical_hash(classified.kind, &reps, None);
            s.upsert_clip(&NewClip {
                classified,
                hash,
                reps,
                source: None,
                sensitivity: Sensitivity {
                    level: SensitivityLevel::Masked,
                    categories: vec![SensitiveCategory::ApiKey],
                },
                expires_at: Some(Timestamp(expires_at)),
                thumbnail_png: None,
            })
            .unwrap();
        }
    }

    #[test]
    fn masked_preview_uses_settings_language() {
        let policy = PrivacyPolicy {
            sensitive_handling: SensitiveHandling::StoreMasked,
            ..PrivacyPolicy::default()
        };
        let enabled = policy.sensitive_categories_enabled.clone();
        let cfg = CaptureConfig {
            lang: Lang::Vi,
            ..test_cfg()
        };
        let h = start_with(policy, RetentionPolicy::default(), cfg, |_| {});
        let text = api_key_text();
        h.fake.copy(text_copy(&text));
        let (id, _) = h.wait_added();
        let hint = scan(&text, &enabled).mask_hint.unwrap();
        let clip = h.ro().get_clip(&id).unwrap().unwrap();
        assert_eq!(clip.preview, masked_preview(&hint, Lang::Vi));
        assert_ne!(clip.preview, masked_preview(&hint, Lang::En));
        // A language change applies to later captures.
        h.cap().send(ActorCommand::SetLang(Lang::En)).unwrap();
        h.fake
            .copy(text_copy(&format!("other {}", &api_key_text()[4..])));
        let (id2, _) = h.wait_added();
        let clip2 = h.ro().get_clip(&id2).unwrap().unwrap();
        let hint2 = scan(&format!("other {}", &api_key_text()[4..]), &enabled)
            .mask_hint
            .unwrap();
        assert_eq!(clip2.preview, masked_preview(&hint2, Lang::En));
    }

    #[test]
    fn late_copy_back_does_not_write() {
        let h = start(PrivacyPolicy::default());
        h.fake.copy(text_copy("clip A"));
        let (a, _) = h.wait_added();
        h.fake.copy(text_copy("clip B"));
        h.wait_added();
        let before: Vec<_> = h.fake.current().into_iter().map(|r| r.bytes).collect();
        assert_eq!(h.fake.write_calls(), 0);
        let deadline = Instant::now();
        std::thread::sleep(Duration::from_millis(5));
        let (tx, rx) = mpsc::channel();
        h.cap()
            .send(ActorCommand::CopyBack {
                id: a,
                plain: false,
                deadline,
                reply: tx,
            })
            .unwrap();
        assert_eq!(rx.recv_timeout(WAIT).unwrap(), Err(ApiError::Internal));
        assert_eq!(h.fake.write_calls(), 0);
        let now: Vec<_> = h.fake.current().into_iter().map(|r| r.bytes).collect();
        assert_eq!(now, before);
        // On time, the same command writes.
        h.copy_back(a, false).unwrap();
        assert_eq!(h.fake.write_calls(), 1);
    }

    #[test]
    fn late_commands_have_no_side_effect() {
        let h = start(PrivacyPolicy::default());
        h.fake.copy(text_copy("keep me"));
        let (id, _) = h.wait_added();
        let deadline = Instant::now();
        std::thread::sleep(Duration::from_millis(5));
        let (tx, rx) = mpsc::channel();
        h.cap()
            .send(ActorCommand::DeleteClip {
                id,
                deadline,
                reply: tx,
            })
            .unwrap();
        assert_eq!(rx.recv_timeout(WAIT).unwrap(), Err(ApiError::Internal));
        let (tx, rx) = mpsc::channel();
        h.cap()
            .send(ActorCommand::SetPinned {
                id,
                pinned: true,
                deadline,
                reply: tx,
            })
            .unwrap();
        assert_eq!(rx.recv_timeout(WAIT).unwrap(), Err(ApiError::Internal));
        let (tx, rx) = mpsc::channel();
        h.cap()
            .send(ActorCommand::ClearHistory {
                keep_pinned: false,
                deadline,
                reply: tx,
            })
            .unwrap();
        assert_eq!(rx.recv_timeout(WAIT).unwrap(), Err(ApiError::Internal));
        assert_eq!(h.clip_count(), 1);
        assert!(!h.ro().get_clip(&id).unwrap().unwrap().pinned);
    }

    #[test]
    fn retention_recovery_emits_retention_ok() {
        let h = start_with(
            PrivacyPolicy::default(),
            RetentionPolicy::default(),
            test_cfg(),
            |_| {},
        );
        let hooks = &h.cap().hooks;
        hooks.fail.store(true, Ordering::SeqCst);
        let send_policy = || {
            h.cap()
                .send(ActorCommand::UpdatePolicy(
                    PrivacyPolicy::default(),
                    RetentionPolicy::default(),
                ))
                .unwrap();
        };
        send_policy();
        h.wait_for("retention failed", |e| {
            *e == AppEvent::Unavailable(UNAVAILABLE_RETENTION)
        });
        hooks.fail.store(false, Ordering::SeqCst);
        send_policy();
        h.wait_for("RetentionOk", |e| *e == AppEvent::RetentionOk);
    }

    #[test]
    fn expiry_lookup_failure_falls_back_to_timer() {
        let h = start_with(
            PrivacyPolicy::default(),
            RetentionPolicy::default(),
            test_cfg(),
            seed_masked(T0 + 1_000),
        );
        h.sentinel();
        let hooks = &h.cap().hooks;
        hooks.expiry_fail.store(true, Ordering::SeqCst);
        // A retention run that succeeds but cannot read the next expiry: the schedule is dropped.
        h.cap()
            .send(ActorCommand::UpdatePolicy(
                PrivacyPolicy::default(),
                RetentionPolicy::default(),
            ))
            .unwrap();
        assert_eq!(
            h.copy_back(ClipId::new_v7(Timestamp(T0)), false),
            Err(ApiError::NotFound)
        );
        let before = hooks.attempts.load(Ordering::SeqCst);
        h.clock.set(T0 + 2_000);
        h.fake.signal_again(1);
        std::thread::sleep(Duration::from_millis(200));
        // No expiry-driven retention (the 10-minute timer is far away), so the clip stays.
        assert_eq!(hooks.attempts.load(Ordering::SeqCst), before);
        assert_eq!(h.clip_count(), 2);
    }

    #[test]
    fn failed_retention_backs_off() {
        let h = start_with(
            PrivacyPolicy::default(),
            RetentionPolicy::default(),
            test_cfg(),
            seed_masked(T0 + 1_000),
        );
        h.sentinel();
        let hooks = &h.cap().hooks;
        hooks.fail.store(true, Ordering::SeqCst);
        let before = hooks.attempts.load(Ordering::SeqCst);
        // Past expiry, and every retention attempt fails.
        h.clock.set(T0 + 2_000);
        h.fake.signal_again(1);
        std::thread::sleep(Duration::from_millis(200));
        let attempts = hooks.attempts.load(Ordering::SeqCst) - before;
        let failures = h
            .events
            .try_iter()
            .filter(|e| *e == AppEvent::Unavailable("retention failed"))
            .count();
        assert!((1..=2).contains(&attempts), "attempts {attempts}");
        assert!((1..=2).contains(&failures), "Unavailable events {failures}");
        // Nothing was deleted while failing.
        assert_eq!(h.clip_count(), 2);
    }

    #[test]
    fn retention_backoff_doubles_to_ten_minutes() {
        assert_eq!(next_backoff(None), Duration::from_secs(30));
        assert_eq!(
            next_backoff(Some(Duration::from_secs(30))),
            Duration::from_secs(60)
        );
        assert_eq!(
            next_backoff(Some(Duration::from_secs(480))),
            Duration::from_secs(600)
        );
        assert_eq!(
            next_backoff(Some(Duration::from_secs(600))),
            Duration::from_secs(600)
        );
    }

    #[test]
    fn masked_clip_from_earlier_session_removed_at_expiry() {
        let seed = |s: &mut Storage| {
            let reps = vec![RawRepresentation {
                mime: Mime::TextPlain,
                platform_format: "CF_UNICODETEXT".into(),
                bytes: b"masked from last run".to_vec(),
                width: None,
                height: None,
                truncated: false,
            }];
            let classified = classify(&reps);
            let hash = canonical_hash(classified.kind, &reps, None);
            s.upsert_clip(&NewClip {
                classified,
                hash,
                reps,
                source: None,
                sensitivity: Sensitivity {
                    level: SensitivityLevel::Masked,
                    categories: vec![SensitiveCategory::ApiKey],
                },
                expires_at: Some(Timestamp(T0 + 5_000)),
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
        // Not expired yet: survives the start-up retention.
        h.sentinel();
        assert_eq!(h.clip_count(), 2);
        h.clock.set(T0 + 5_000);
        h.fake.copy(text_copy("second sentinel"));
        h.wait_added();
        assert_eq!(h.clip_count(), 2);
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
        assert!(rows.iter().all(|r| r.sensitivity == SensitivityLevel::None));
    }

    #[test]
    fn html_only_api_key_dropped() {
        let h = start(PrivacyPolicy::default());
        let html = format!("<p>{}</p>", api_key_text());
        h.fake.copy(FakeCopy {
            formats: vec![(
                pf("HTML Format", Some(Mime::TextHtml), html.len()),
                html.into_bytes(),
            )],
            markers: Vec::new(),
            source: None,
        });
        let seen = h.wait_skip(SkipReason::Sensitive);
        assert!(no_store_events(&seen), "{seen:?}");
        assert_eq!(h.fake.read_calls(), 1);
        assert_eq!(h.clip_count(), 0);
    }

    #[test]
    fn seq_change_during_read_drops_bytes() {
        let h = start(PrivacyPolicy::default());
        h.fake.bump_seq_on_next_read();
        h.fake.copy(text_copy("raced text"));
        let deadline = Instant::now() + WAIT;
        while h.cap().skip_counters().changed_during_read() == 0 {
            assert!(Instant::now() < deadline, "no changed-during-read count");
            std::thread::sleep(Duration::from_millis(1));
        }
        assert_eq!(h.fake.read_calls(), 1);
        let between = h.sentinel();
        assert!(no_store_events(&between), "{between:?}");
        assert_eq!(h.clip_count(), 1);
        let ro = h.ro();
        let (rows, _) = ro
            .search(&SearchQuery {
                text: Some("raced".into()),
                kinds: Vec::new(),
                pinned_only: false,
                since: None,
                include_masked: true,
                limit: 10,
                cursor: None,
            })
            .unwrap();
        assert!(rows.is_empty());
        assert_eq!(h.cap().skip_counters().changed_during_read(), 1);
    }

    #[test]
    fn ole_three_phases_in_one_window() {
        in_one_window(|| {
            let h = start_with(
                PrivacyPolicy::default(),
                RetentionPolicy::default(),
                slow_cfg(),
                |_| {},
            );
            h.fake.copy(FakeCopy {
                formats: vec![
                    (pf("DataObject", None, 8), Vec::new()),
                    (pf("Ole Private Data", None, 8), Vec::new()),
                ],
                markers: Vec::new(),
                source: Some(src("winword.exe")),
            });
            // The first signal carries winword.exe (F-01): phases 2 and 3 may land before
            // or after the actor dequeues it.
            h.fake.render(vec![
                (pf("DataObject", None, 8), Vec::new()),
                (
                    pf("CF_UNICODETEXT", Some(Mime::TextPlain), 8),
                    b"ole text".to_vec(),
                ),
            ]);
            h.fake.clear_source();
            h.fake.signal_again(1);
            let (id, before) = h.wait_added();
            if h.fake.snapshot_meta_calls() > 1 {
                return false; // Window closed: phase 1 was inspected alone.
            }
            // One window: phase 1 was never inspected on its own.
            assert!(before.is_empty(), "{before:?}");
            let clip = h.ro().get_clip(&id).unwrap().unwrap();
            assert_eq!(clip.preview, "ole text");
            assert_eq!(
                clip.source.as_ref().map(|s| s.app_id.as_str()),
                Some("winword.exe")
            );
            let between = h.sentinel();
            assert!(no_store_events(&between), "{between:?}");
            assert_eq!(h.clip_count(), 2);
            true
        });
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
            s.upsert_clip(&NewClip {
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

    fn png_copy(name: &str, bytes: Vec<u8>) -> FakeCopy {
        FakeCopy {
            formats: vec![(pf(name, Some(Mime::ImagePng), bytes.len()), bytes)],
            markers: Vec::new(),
            source: None,
        }
    }

    /// The same 3x2 picture, encoded two ways (different filter and compression).
    fn two_encodings() -> (Vec<u8>, Vec<u8>) {
        let px: Vec<u8> = (0..24u8).map(|i| i.wrapping_mul(9)).collect();
        let enc = |filter: png::Filter, comp: png::Compression| {
            let mut out = Vec::new();
            let mut e = png::Encoder::new(&mut out, 3, 2);
            e.set_color(png::ColorType::Rgba);
            e.set_depth(png::BitDepth::Eight);
            e.set_filter(filter);
            e.set_compression(comp);
            let mut w = e.write_header().unwrap();
            w.write_image_data(&px).unwrap();
            w.finish().unwrap();
            out
        };
        (
            enc(png::Filter::NoFilter, png::Compression::Fast),
            enc(png::Filter::Sub, png::Compression::High),
        )
    }

    #[test]
    fn same_pixels_different_encoding_dedupes() {
        let (a, b) = two_encodings();
        assert_ne!(a, b);
        let h = start(PrivacyPolicy::default());
        h.fake.copy(png_copy("PNG", a));
        let (first, _) = h.wait_added();
        h.fake.copy(png_copy("PNG", b));
        h.wait_for("ClipBumped", |e| *e == AppEvent::ClipBumped(first));
        assert_eq!(h.clip_count(), 1);
    }

    #[test]
    fn corrupt_png_still_stored_with_fallback_hash() {
        let h = start(PrivacyPolicy::default());
        let bytes = b"\x89PNG\r\n\x1a\nnot really a png".to_vec();
        h.fake.copy(png_copy("PNG", bytes.clone()));
        let (id, _) = h.wait_added();
        let ro = h.ro();
        let clip = ro.get_clip(&id).unwrap().unwrap();
        assert_eq!(clip.kind, ClipType::Image);
        assert!(clip.metadata.image_thumb_sha256.is_none());
        let want = canonical_hash(
            ClipType::Image,
            &[RawRepresentation {
                mime: Mime::ImagePng,
                platform_format: "PNG".into(),
                bytes,
                width: None,
                height: None,
                truncated: false,
            }],
            None,
        );
        assert_eq!(clip.content_hash, want);
    }

    #[test]
    fn masked_image_clip_has_no_thumbnail() {
        let (a, _) = two_encodings();
        let html = format!("<p>{}</p>", api_key_text()).into_bytes();
        let h = start(masked_policy());
        h.fake.copy(FakeCopy {
            formats: vec![
                (pf("PNG", Some(Mime::ImagePng), a.len()), a),
                (pf("HTML Format", Some(Mime::TextHtml), html.len()), html),
            ],
            markers: Vec::new(),
            source: None,
        });
        let (id, _) = h.wait_added();
        let clip = h.ro().get_clip(&id).unwrap().unwrap();
        assert_eq!(clip.kind, ClipType::Image);
        assert_eq!(clip.sensitivity.level, SensitivityLevel::Masked);
        assert!(clip.metadata.image_thumb_sha256.is_none());
    }

    #[test]
    fn image_clip_has_thumbnail_and_dimensions() {
        let px: Vec<u8> = vec![120; 600 * 40 * 4];
        let mut bytes = Vec::new();
        let mut e = png::Encoder::new(&mut bytes, 600, 40);
        e.set_color(png::ColorType::Rgba);
        e.set_depth(png::BitDepth::Eight);
        let mut w = e.write_header().unwrap();
        w.write_image_data(&px).unwrap();
        w.finish().unwrap();
        let h = start(PrivacyPolicy::default());
        h.fake.copy(png_copy("PNG", bytes));
        let (id, _) = h.wait_added();
        let ro = h.ro();
        let clip = ro.get_clip(&id).unwrap().unwrap();
        assert!(clip.metadata.image_thumb_sha256.is_some());
        let rep = &clip.representations[0];
        assert_eq!((rep.width, rep.height), (Some(600), Some(40)));
        assert_eq!(clip.preview, "Image 600\u{00d7}40");
    }

    // F-14 (Task 109): OLE writers bump the seq at each phase; the final flush has no owner.

    /// Debounce wide enough that signal gaps in these tests stay inside it; the 500 ms burst
    /// cap still bounds every wait.
    fn ole_cfg() -> CaptureConfig {
        CaptureConfig {
            debounce_ms: 1_000,
            ..test_cfg()
        }
    }

    /// A copy with an owner, offering the OLE placeholder next to its text.
    fn ole_owned(app: &str, text: &str) -> FakeCopy {
        FakeCopy {
            formats: vec![
                (pf("DataObject", None, 8), Vec::new()),
                (
                    pf("CF_UNICODETEXT", Some(Mime::TextPlain), text.len()),
                    text.as_bytes().to_vec(),
                ),
            ],
            markers: Vec::new(),
            source: Some(src(app)),
        }
    }

    /// The ownerless flush phase: the OLE placeholders and the rendered text, no owner.
    fn ole_flush(text: &str) -> FakeCopy {
        FakeCopy {
            formats: vec![
                (pf("DataObject", None, 8), Vec::new()),
                (pf("Ole Private Data", None, 8), Vec::new()),
                (
                    pf("CF_UNICODETEXT", Some(Mime::TextPlain), text.len()),
                    text.as_bytes().to_vec(),
                ),
            ],
            markers: Vec::new(),
            source: None,
        }
    }

    fn app_of(h: &H, id: &ClipId) -> Option<String> {
        h.ro()
            .get_clip(id)
            .unwrap()
            .unwrap()
            .source
            .map(|s| s.app_id)
    }

    #[test]
    fn ole_flush_follow_up_keeps_source() {
        let h = start_with(
            PrivacyPolicy::default(),
            RetentionPolicy::default(),
            ole_cfg(),
            |_| {},
        );
        h.fake.copy_burst(vec![
            ole_owned("notepad.exe", "flushed text"),
            ole_flush("flushed text"),
        ]);
        let (id, before) = h.wait_added();
        assert!(no_store_events(&before), "{before:?}");
        assert_eq!(app_of(&h, &id).as_deref(), Some("notepad.exe"));
        assert_eq!(h.fake.snapshot_meta_calls(), 1);
        let between = h.sentinel();
        assert!(no_store_events(&between), "{between:?}");
        assert_eq!(h.clip_count(), 2);
    }

    #[test]
    fn excluded_app_ole_flush_without_markers_is_skipped() {
        let h = start_with(
            PrivacyPolicy::default(),
            RetentionPolicy::default(),
            ole_cfg(),
            |_| {},
        );
        // KeePassXC.exe is on the default excluded list; no markers on either phase.
        h.fake.copy_burst(vec![
            ole_owned("KeePassXC.exe", "vault entry"),
            ole_flush("vault entry"),
        ]);
        let seen = h.wait_skip(SkipReason::ExcludedApp);
        assert!(no_store_events(&seen), "{seen:?}");
        assert_eq!(h.fake.read_calls(), 0);
        let between = h.sentinel();
        assert!(no_store_events(&between), "{between:?}");
        assert_eq!(h.clip_count(), 1);
        assert!(!h.stored_previews().iter().any(|p| p == "vault entry"));
    }

    #[test]
    fn ownerless_write_without_ole_signature_does_not_inherit() {
        // Not excluded: stored, but the source is not inherited.
        let h = start_with(
            PrivacyPolicy::default(),
            RetentionPolicy::default(),
            ole_cfg(),
            |_| {},
        );
        h.fake.copy_burst(vec![
            ole_owned("notepad.exe", "first writer"),
            text_copy("other writer"),
        ]);
        let (id, before) = h.wait_added();
        assert!(no_store_events(&before), "{before:?}");
        let clip = h.ro().get_clip(&id).unwrap().unwrap();
        assert_eq!(clip.preview, "other writer");
        assert!(clip.source.is_none());
        // Excluded: the superseded source is still checked, so the change is skipped unread.
        let h = start_with(
            PrivacyPolicy::default(),
            RetentionPolicy::default(),
            ole_cfg(),
            |_| {},
        );
        h.fake.copy_burst(vec![
            ole_owned("KeePassXC.exe", "vault entry"),
            text_copy("other writer"),
        ]);
        let seen = h.wait_skip(SkipReason::ExcludedApp);
        assert!(no_store_events(&seen), "{seen:?}");
        assert_eq!(h.fake.read_calls(), 0);
    }

    #[test]
    fn processed_change_is_never_carried() {
        // A short burst cap processes the first change long before the debounce would.
        let h = start_full(
            PrivacyPolicy::default(),
            RetentionPolicy::default(),
            ole_cfg(),
            Duration::from_millis(20),
            |_| {},
        );
        h.fake.copy(ole_owned("notepad.exe", "first text"));
        let (first, _) = h.wait_added();
        assert_eq!(app_of(&h, &first).as_deref(), Some("notepad.exe"));
        // Within the debounce of the first signal, OLE signature, seq + 1: still not carried.
        h.fake.copy(ole_flush("second text"));
        let (second, before) = h.wait_added();
        assert!(no_store_events(&before), "{before:?}");
        assert_eq!(app_of(&h, &second), None);
        assert_eq!(h.clip_count(), 2);
    }

    #[test]
    fn self_write_seq_breaks_the_chain() {
        let h = start_with(
            PrivacyPolicy::default(),
            RetentionPolicy::default(),
            ole_cfg(),
            |_| {},
        );
        h.fake.copy(text_copy("stored earlier"));
        let (earlier, _) = h.wait_added();
        h.fake.copy(ole_owned("notepad.exe", "before copy back"));
        h.copy_back(earlier, false).unwrap();
        h.fake.copy(ole_flush("after copy back"));
        let (id, before) = h.wait_added();
        assert!(
            before.contains(&AppEvent::Skipped(SkipReason::SelfWrite)),
            "{before:?}"
        );
        let clip = h.ro().get_clip(&id).unwrap().unwrap();
        assert_eq!(clip.preview, "after copy back");
        assert!(clip.source.is_none());
    }

    #[test]
    fn burst_cap_bounds_the_chain() {
        // Signal times are explicit; both signals are queued together, so only their times
        // differ. Debounce 1 s, burst cap 500 ms.
        let run = |gap_ms: i64| {
            let h = start_with(
                PrivacyPolicy::default(),
                RetentionPolicy::default(),
                ole_cfg(),
                |_| {},
            );
            h.fake.copy_burst_at(vec![
                (Timestamp(T0), ole_owned("notepad.exe", "flushed text")),
                (Timestamp(T0 + gap_ms), ole_flush("flushed text")),
            ]);
            let (id, before) = h.wait_added();
            assert!(no_store_events(&before), "{before:?}");
            assert_eq!(h.fake.snapshot_meta_calls(), 1);
            app_of(&h, &id)
        };
        // Inside the cap: inherited.
        assert_eq!(run(400).as_deref(), Some("notepad.exe"));
        // Beyond the cap (but inside the debounce): not inherited.
        assert_eq!(run(600), None);
    }
}
