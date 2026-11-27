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
//!
//! Layout: this file holds the public API (`CaptureHandle`, `ActorCommand`) and spawns the
//! actor; `actor` (thread, run loop, gates, commands), `pending` (debounce, burst, supersede
//! chain, heir, carried, excluded_recent), `pipeline` (snapshot → `decide_pre_read` → read →
//! classify → sensitive → decode/hash → store), `copy_back`, `retention` and `wipe`. Every
//! file under `capture/` is held to GC-03 (the only clipboard read/write path) and GC-06 (no
//! logging).

mod actor;
mod claim;
mod copy_back;
mod maintenance;
mod organize;
mod pending;
mod pipeline;
mod retention;
#[cfg(test)]
mod tests;
mod transfer;
mod wipe;

use crate::config::CaptureConfig;
use crate::events::{ApiError, AppEvent, SkipCounters};
use actor::Actor;
pub use claim::{ClaimError, CLAIM_WINDOW_MS};
use darkclip_core::{
    CaptureMode, ClipId, Clock, CollectionId, Lang, PrivacyPolicy, RetentionPolicy,
};
use darkclip_platform::{ChangeSignal, PlatformBundle};
use darkclip_storage::Storage;
#[cfg(test)]
use darkclip_storage::StorageError;
use pending::MAX_BURST_WAIT;
#[cfg(test)]
use std::sync::atomic::AtomicU32;
use std::sync::mpsc::{self, Sender};
use std::sync::{Arc, Mutex, PoisonError};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};
pub use transfer::{ImportReport, TransferOp, TransferReply, APP_VERSION};

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
    /// "Copy as...": writes this text to the clipboard as our own write (never captured,
    /// never stored; Task 404). Replies with the seq of that write (Task 603).
    CopyText {
        text: darkclip_core::Redacted<String>,
        deadline: Instant,
        reply: Sender<Result<u64, ApiError>>,
    },
    /// Translate in place (Task 603): arms the claim slot for `app_id` for `CLAIM_WINDOW_MS`.
    /// The next change from that app (or its F-14 ownerless flush) is replied as text and
    /// never stored. Every privacy skip still wins; a second request while one is armed
    /// replies `Busy`; no change in time replies `Expired`.
    ExpectForeignCopy {
        app_id: String,
        reply: Sender<Result<darkclip_core::Redacted<String>, ClaimError>>,
    },
    /// The stored clip the clipboard holds now, if any: `Some` only when the OS seq equals the
    /// last processed seq and that change was stored (inserted or deduplicated).
    RestoreCandidate {
        reply: Sender<Option<ClipId>>,
    },
    /// "Save as new clip": the edited text goes through capture's classify, kind check,
    /// sensitive handling and size limit (Task 404). Not gated by the capture mode: the user
    /// asked for it.
    SaveText {
        text: darkclip_core::Redacted<String>,
        deadline: Instant,
        reply: Sender<Result<SaveOutcome, ApiError>>,
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
    /// A data-management action of Settings > History (Task 411).
    Maintenance {
        op: MaintOp,
        deadline: Instant,
        reply: Sender<Result<MaintReply, ApiError>>,
    },
    /// A collection, favorite, tag or note change; replies with what it produced.
    Organize {
        op: OrganizeOp,
        deadline: Instant,
        reply: Sender<Result<OrganizeReply, ApiError>>,
    },
    /// A backup, restore, export or import (Task 405). Long-running: callers wait longer
    /// than for other commands.
    Transfer {
        op: TransferOp,
        deadline: Instant,
        reply: Sender<Result<TransferReply, ApiError>>,
    },
    UpdatePolicy(PrivacyPolicy, RetentionPolicy),
    /// Processes every command queued before it, stops the monitor and ends the actor.
    Shutdown {
        reply: Sender<()>,
    },
}

/// What saving an edited text produced (Task 404).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SaveOutcome {
    Saved(ClipId),
    /// The same content is already in history: nothing was touched (Task 404 review).
    Exists(ClipId),
    /// Capture's own rules refused it (a secret under "drop", a disabled kind); nothing stored.
    Dropped(darkclip_core::policy::SkipReason),
}

/// A data-management action (Task 411). `Compact` carries the free disk space the caller
/// measured: this crate has no OS access.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MaintOp {
    DeleteOlderThan(u32),
    SweepOrphans,
    Compact(u64),
}

/// What a `MaintOp` produced.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MaintReply {
    Deleted(u64),
    Orphans(darkclip_storage::OrphanReport),
    Compacted(darkclip_storage::CompactReport),
}

/// An organisation mutation (Task 402). Names and notes are user text, so the manual `Debug`
/// below prints the variant name only (the derived `Debug` of `ActorCommand` uses it).
pub enum OrganizeOp {
    CreateCollection(String),
    RenameCollection(CollectionId, String),
    DeleteCollection(CollectionId),
    AddToCollection(CollectionId, ClipId),
    RemoveFromCollection(CollectionId, ClipId),
    Reorder(CollectionId, ClipId, u32),
    SetFavorite(ClipId, bool),
    SetNote(ClipId, Option<String>),
    SetTags(ClipId, Vec<String>),
}

/// Prints the operation only, never a name, tag or note (GC-06).
impl std::fmt::Debug for OrganizeOp {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::CreateCollection(_) => "CreateCollection",
            Self::RenameCollection(..) => "RenameCollection",
            Self::DeleteCollection(_) => "DeleteCollection",
            Self::AddToCollection(..) => "AddToCollection",
            Self::RemoveFromCollection(..) => "RemoveFromCollection",
            Self::Reorder(..) => "Reorder",
            Self::SetFavorite(..) => "SetFavorite",
            Self::SetNote(..) => "SetNote",
            Self::SetTags(..) => "SetTags",
        })
    }
}

/// What an `OrganizeOp` produced.
pub enum OrganizeReply {
    Done,
    Collection(darkclip_core::Collection),
    Tags(Vec<darkclip_core::TagRef>),
}

/// Retention instrumentation: an attempt counter, and (tests only) forced failure.
#[derive(Default)]
struct RetentionHooks {
    #[cfg(test)]
    attempts: AtomicU32,
    #[cfg(test)]
    fail: std::sync::atomic::AtomicBool,
    /// Tests: the retention interval in milliseconds (0 = the real 10 minutes).
    #[cfg(test)]
    tick_ms: std::sync::atomic::AtomicU64,
    /// Tests: errors the next `upsert_clip` calls return, in order, before touching storage.
    #[cfg(test)]
    inject: Mutex<std::collections::VecDeque<StorageError>>,
    /// Tests: `next_expiry` fails after a successful retention.
    #[cfg(test)]
    expiry_fail: std::sync::atomic::AtomicBool,
    /// Tests: the actor stalls this long when it dequeues the signal for this seq (as if
    /// preempted), before it looks at the clock.
    #[cfg(test)]
    stall_on_signal: Mutex<Option<(u64, Duration)>>,
    /// Tests: every transfer command sleeps this long first (milliseconds), so a test can act
    /// while one runs (Task 405).
    #[cfg(test)]
    transfer_stall_ms: std::sync::atomic::AtomicU64,
    /// Tests: the claim window in milliseconds (0 = `CLAIM_WINDOW_MS`).
    #[cfg(test)]
    claim_window_ms: std::sync::atomic::AtomicU64,
    /// Tests: the next signal for each of these seqs is lost (removed when it arrives).
    #[cfg(test)]
    drop_signal: Mutex<Vec<u64>>,
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
            excluded_recent: None,
            claim: None,
            last_stored: None,
            next_retention: Instant::now(),
            fts_merge_due: None,
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

    /// Tests: makes every following transfer command sleep `ms` first.
    #[cfg(test)]
    pub(crate) fn set_transfer_stall(&self, ms: u64) {
        self.hooks
            .transfer_stall_ms
            .store(ms, std::sync::atomic::Ordering::SeqCst);
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
