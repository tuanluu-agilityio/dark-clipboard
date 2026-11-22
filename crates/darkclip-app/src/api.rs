//! The application API (`ClipApi`, ai-integration §1) and the `App` that implements it.
//!
//! Write path (system-overview §2 rule 6: the capture actor owns the only writable
//! `Storage`): `App` never writes. Reads (search, get, stats) use a second, read-only
//! `Storage` connection (WAL allows it next to the actor's writer). Every mutation (pin,
//! delete, clear history, copy back) is an `ActorCommand` executed on the actor thread, so
//! writes stay serialized with capture and retention.
//!
//! Errors carry no clip content (GC-06); `Caller::Mcp` never sees masked clips.

use crate::capture::{ActorCommand, CaptureHandle};
use crate::config::CaptureConfig;
use crate::envelope::Page;
use crate::events::{
    map_storage, ApiError, AppEvent, UNAVAILABLE_READ, UNAVAILABLE_RETENTION,
    UNAVAILABLE_STORAGE_WRITE,
};
use crate::permission::{Operation, PermissionGuard};
use crate::settings::{LoadOutcome, Settings};
use darkclip_core::{
    CaptureMode, Clip, ClipId, ClipType, Clock, CollectionId, Mime, SensitivityLevel, TagId,
    Timestamp,
};
use darkclip_platform::{Capabilities, PlatformBundle};
use darkclip_storage::{ClipSummary, OpenMode, SearchQuery, Storage};
use serde::Serialize;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Sender};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

/// Who is calling. The App is in-process in Phase 1; the P2 CLI and MCP processes
/// will reach it over IPC.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Caller {
    Ui,
    Cli,
    Mcp { client_id: String },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CopyMode {
    Original,
    PlainText,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct GetOptions {
    pub include_content: bool,
}

/// Search result item (api-envelope example: `id, kind, preview, pinned, last_copied_at`).
#[derive(Clone, PartialEq, Eq, Serialize)]
pub struct ClipSummaryView {
    pub id: ClipId,
    pub kind: ClipType,
    pub preview: String,
    pub pinned: bool,
    pub last_copied_at: Timestamp,
    /// Only ever true for `Caller::Ui` (other callers never get masked clips).
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub masked: bool,
    /// Source app name (or id) for the popup's source chips. UI only: never serialized, so
    /// the envelope (CLI/MCP) is unchanged.
    #[serde(skip)]
    pub source_app: Option<String>,
}

/// Never prints the preview (GC-06).
impl std::fmt::Debug for ClipSummaryView {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ClipSummaryView")
            .field("id", &self.id)
            .field("kind", &self.kind)
            .field("preview", &"<redacted>")
            .field("pinned", &self.pinned)
            .field("masked", &self.masked)
            .finish_non_exhaustive()
    }
}

impl From<ClipSummary> for ClipSummaryView {
    fn from(c: ClipSummary) -> Self {
        Self {
            id: c.id,
            kind: c.kind,
            preview: c.preview,
            pinned: c.pinned,
            last_copied_at: c.last_copied_at,
            masked: c.sensitivity == SensitivityLevel::Masked,
            source_app: c.source_app,
        }
    }
}

#[derive(Clone, PartialEq, Eq, Serialize)]
pub struct ClipContent {
    pub text: String,
    /// The text was cut at 100 KiB (on a character boundary).
    pub truncated: bool,
}

/// Never prints the text (GC-06).
impl std::fmt::Debug for ClipContent {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ClipContent")
            .field("text", &"<redacted>")
            .field("truncated", &self.truncated)
            .finish()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ClipView {
    #[serde(flatten)]
    pub clip: Clip,
    /// Plain text of the clip, only when asked for; never for masked clips.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<ClipContent>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CollectionView {
    pub id: CollectionId,
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TagView {
    pub id: TagId,
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ExportResult {
    pub path: String,
    pub clip_count: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct KindCount {
    pub kind: ClipType,
    pub count: u64,
}

/// Counts only (ai-integration §2: works for any caller).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct StorageStatsView {
    pub clips: u64,
    pub pinned: u64,
    pub by_kind: Vec<KindCount>,
    pub db_bytes: u64,
    pub blob_bytes: u64,
    /// `normal`, `paused`, `private` or `unavailable`.
    pub capture: &'static str,
    /// Skipped changes by reason name (in memory counters since start).
    pub skipped: BTreeMap<String, u64>,
}

/// What the UI shows for capture health.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CaptureStatus {
    Active,
    Paused,
    Private,
    Unavailable(&'static str),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AppStatus {
    pub capture: CaptureStatus,
    /// Settings could not be read or are from a newer version: defaults are in use and
    /// nothing is written to the settings file (so the original is never overwritten).
    pub settings_read_only: bool,
}

/// ai-integration §1. `PermissionGuard` runs first in every method.
pub trait ClipApi {
    fn search_clips(&self, c: &Caller, q: SearchQuery) -> Result<Page<ClipSummaryView>, ApiError>;
    fn get_clip(&self, c: &Caller, id: ClipId, opts: GetOptions) -> Result<ClipView, ApiError>;
    fn list_collections(&self, c: &Caller) -> Result<Vec<CollectionView>, ApiError>;
    fn create_collection(&self, c: &Caller, name: &str) -> Result<CollectionView, ApiError>;
    fn add_clip_to_collection(
        &self,
        c: &Caller,
        clip: ClipId,
        col: CollectionId,
    ) -> Result<(), ApiError>;
    fn update_clip_note(
        &self,
        c: &Caller,
        clip: ClipId,
        note: Option<String>,
    ) -> Result<(), ApiError>;
    fn update_clip_tags(
        &self,
        c: &Caller,
        clip: ClipId,
        tags: Vec<String>,
    ) -> Result<Vec<TagView>, ApiError>;
    fn pin_clip(&self, c: &Caller, clip: ClipId, pinned: bool) -> Result<(), ApiError>;
    fn copy_clip_to_clipboard(
        &self,
        c: &Caller,
        clip: ClipId,
        mode: CopyMode,
    ) -> Result<(), ApiError>;
    fn delete_clip(&self, c: &Caller, clip: ClipId) -> Result<(), ApiError>;
    fn get_storage_stats(&self, c: &Caller) -> Result<StorageStatsView, ApiError>;
    fn export_collection(&self, c: &Caller, col: CollectionId) -> Result<ExportResult, ApiError>;
    fn set_capture_mode(&self, c: &Caller, mode: CaptureMode) -> Result<(), ApiError>;
}

/// P1/P2 methods. `ApiError` has no `Unsupported` variant and the envelope code set is
/// fixed by the schema, so these use `InvalidArgument`.
const NOT_YET: ApiError = ApiError::InvalidArgument("not available until P1");

const MAX_LIMIT: u16 = 200;
const MAX_LIMIT_MCP: u16 = 50;
const CONTENT_MAX_BYTES: usize = 100 * 1024;
const ACTOR_REPLY_TIMEOUT: Duration = Duration::from_secs(10);

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Capture health as the pump sees it, one slot per cause with its own recovery:
/// - `permanent` (monitor stopped, platform unsupported or denied): until restart;
/// - `retention`: cleared by a successful retention run (`RetentionOk`);
/// - `transient` (storage write or clipboard read failed): cleared by the next successful
///   store.
///
/// A mode change clears nothing.
#[derive(Debug, Default)]
struct StatusState {
    permanent: Option<&'static str>,
    retention: Option<&'static str>,
    transient: Option<&'static str>,
}

impl StatusState {
    fn apply(&mut self, e: &AppEvent) {
        match e {
            AppEvent::Unavailable(what) => match *what {
                UNAVAILABLE_RETENTION => self.retention = Some(what),
                UNAVAILABLE_STORAGE_WRITE | UNAVAILABLE_READ => self.transient = Some(what),
                _ => self.permanent = Some(what),
            },
            AppEvent::RetentionOk => self.retention = None,
            AppEvent::ClipAdded(_) | AppEvent::ClipBumped(_) => self.transient = None,
            AppEvent::ModeChanged(_) | AppEvent::Skipped(_) => {}
        }
    }

    fn current(&self) -> Option<&'static str> {
        self.permanent.or(self.retention).or(self.transient)
    }
}

pub struct App {
    /// Read-only connection; the actor's connection is the only writer.
    reader: Mutex<Storage>,
    cap: CaptureHandle,
    /// Held while saving, so saves are serialized and the file matches memory.
    settings: Mutex<Settings>,
    settings_path: PathBuf,
    /// False when the settings file must not be written (see `AppStatus::settings_read_only`).
    persist: bool,
    status: Arc<Mutex<StatusState>>,
    clock: Arc<dyn Clock>,
    /// What the platform adapter reported at start.
    caps: Capabilities,
    pump: Option<JoinHandle<()>>,
}

impl App {
    /// Loads settings, opens storage, starts the capture actor. `events` receives every
    /// `AppEvent` after the App has updated its status from it.
    pub fn start(
        bundle: PlatformBundle,
        data_dir: &Path,
        settings_path: &Path,
        clock: Arc<dyn Clock>,
        cfg: CaptureConfig,
        events: Sender<AppEvent>,
    ) -> Result<(App, LoadOutcome), ApiError> {
        let (settings, outcome) = Settings::load(settings_path);
        let persist = !matches!(outcome, LoadOutcome::DefaultedReadOnly(_));
        let writer = Storage::open(data_dir, OpenMode::ReadWrite, Arc::clone(&clock))
            .map_err(|e| map_storage(&e))?;
        let reader = Storage::open(data_dir, OpenMode::ReadOnly, Arc::clone(&clock))
            .map_err(|e| map_storage(&e))?;
        let (tx, rx) = mpsc::channel::<AppEvent>();
        let status = Arc::new(Mutex::new(StatusState::default()));
        let pump_status = Arc::clone(&status);
        let pump = std::thread::Builder::new()
            .name("darkclip-events".into())
            .spawn(move || {
                for e in rx {
                    lock(&pump_status).apply(&e);
                    let _ = events.send(e);
                }
            })
            .map_err(|_| ApiError::Internal)?;
        let cfg = CaptureConfig {
            lang: settings.ui.language,
            ..cfg
        };
        let caps = bundle.caps;
        let cap = CaptureHandle::spawn(
            bundle,
            writer,
            Arc::clone(&clock),
            cfg,
            settings.privacy.clone(),
            settings.retention.clone(),
            tx,
        )?;
        let app = App {
            reader: Mutex::new(reader),
            cap,
            settings: Mutex::new(settings),
            settings_path: settings_path.to_path_buf(),
            persist,
            status,
            clock,
            caps,
            pump: Some(pump),
        };
        Ok((app, outcome))
    }

    /// Stops the actor (2 s deadline) and the event pump.
    pub fn shutdown(mut self) -> Result<(), ApiError> {
        let pump = self.pump.take();
        // Move the handle out so dropping `self` afterwards does not send a second stop.
        let App { cap, .. } = self;
        let r = cap.shutdown(Duration::from_secs(2));
        // Join the pump only after a clean stop; otherwise the actor may still hold its
        // sender and the join would block, so the pump is detached.
        if r.is_ok() {
            if let Some(p) = pump {
                let _ = p.join();
            }
        }
        r
    }

    /// The platform capabilities reported when the App started.
    pub fn capabilities(&self) -> Capabilities {
        self.caps
    }

    pub fn settings(&self) -> Settings {
        lock(&self.settings).clone()
    }

    pub fn lang(&self) -> darkclip_core::Lang {
        lock(&self.settings).ui.language
    }

    pub fn status(&self) -> AppStatus {
        let capture = if let Some(what) = lock(&self.status).current() {
            CaptureStatus::Unavailable(what)
        } else {
            let s = lock(&self.settings);
            if !s.privacy.capture_enabled {
                CaptureStatus::Paused
            } else {
                match s.privacy.mode {
                    CaptureMode::Normal => CaptureStatus::Active,
                    CaptureMode::Paused { until } if until > self.clock.now() => {
                        CaptureStatus::Paused
                    }
                    CaptureMode::Paused { .. } => CaptureStatus::Active,
                    CaptureMode::Private => CaptureStatus::Private,
                }
            }
        };
        AppStatus {
            capture,
            settings_read_only: !self.persist,
        }
    }

    /// Validates, saves, then applies to the actor (restoring the old file if that fails).
    /// `respect_os_markers` cannot be turned off. The capture mode (`privacy.mode`) is runtime
    /// state that only `set_capture_mode` changes: the one in `new` is ignored.
    pub fn update_settings(&self, new: Settings) -> Result<(), ApiError> {
        if !new.privacy.respect_os_markers {
            return Err(ApiError::InvalidArgument(
                "OS privacy markers cannot be disabled",
            ));
        }
        let mut cur = lock(&self.settings);
        let mut new = new;
        new.privacy.mode = cur.privacy.mode;
        if self.persist {
            new.save(&self.settings_path)?;
        }
        let applied = self
            .cap
            .send(ActorCommand::SetLang(new.ui.language))
            .and_then(|()| {
                self.cap.send(ActorCommand::UpdatePolicy(
                    new.privacy.clone(),
                    new.retention.clone(),
                ))
            });
        if let Err(e) = applied {
            if self.persist {
                let _ = cur.save(&self.settings_path);
            }
            return Err(e);
        }
        *cur = new;
        Ok(())
    }

    /// Deletes the history (optionally keeping pinned clips); returns how many were removed.
    /// Runs on the actor thread.
    pub fn clear_history(&self, c: &Caller, keep_pinned: bool) -> Result<u64, ApiError> {
        PermissionGuard::check(c, Operation::ClearHistory)?;
        self.ask(|reply, deadline| ActorCommand::ClearHistory {
            keep_pinned,
            deadline,
            reply,
        })
    }

    fn reader(&self) -> MutexGuard<'_, Storage> {
        lock(&self.reader)
    }

    /// Sends a command to the actor and waits for its reply. The command carries the
    /// deadline, so the actor drops it unexecuted if it is already late. A `NotFound`-free
    /// `Internal` after a timeout during execution means the outcome is unknown.
    fn ask<T>(
        &self,
        make: impl FnOnce(Sender<Result<T, ApiError>>, Instant) -> ActorCommand,
    ) -> Result<T, ApiError> {
        let (tx, rx) = mpsc::channel();
        let deadline = Instant::now() + ACTOR_REPLY_TIMEOUT;
        self.cap.send(make(tx, deadline))?;
        rx.recv_timeout(ACTOR_REPLY_TIMEOUT)
            .map_err(|_| ApiError::Internal)?
    }

    /// The clip, or `NotFound` when it is missing or hidden from this caller (masked clips
    /// are visible to the UI only).
    fn visible_clip(&self, c: &Caller, id: &ClipId) -> Result<Clip, ApiError> {
        let clip = self
            .reader()
            .get_clip(id)
            .map_err(|e| map_storage(&e))?
            .ok_or(ApiError::NotFound)?;
        let masked = clip.sensitivity.level == SensitivityLevel::Masked;
        if masked && *c != Caller::Ui {
            return Err(ApiError::NotFound);
        }
        if clip.expires_at.is_some_and(|t| t <= self.clock.now()) {
            return Err(ApiError::NotFound);
        }
        Ok(clip)
    }

    fn mode_name(&self) -> &'static str {
        match self.status().capture {
            CaptureStatus::Active => "normal",
            CaptureStatus::Paused => "paused",
            CaptureStatus::Private => "private",
            CaptureStatus::Unavailable(_) => "unavailable",
        }
    }
}

impl ClipApi for App {
    fn search_clips(&self, c: &Caller, q: SearchQuery) -> Result<Page<ClipSummaryView>, ApiError> {
        PermissionGuard::check(c, Operation::SearchClips)?;
        let cap = if matches!(c, Caller::Mcp { .. }) {
            MAX_LIMIT_MCP
        } else {
            MAX_LIMIT
        };
        if q.limit == 0 {
            return Err(ApiError::InvalidArgument("limit must be at least 1"));
        }
        let limit = q.limit.min(cap);
        let q = SearchQuery {
            limit,
            // Masked clips are filtered in the SQL query, for every caller but the UI.
            include_masked: *c == Caller::Ui,
            ..q
        };
        let (rows, next) = self.reader().search(&q).map_err(|e| map_storage(&e))?;
        Ok(Page {
            items: rows.into_iter().map(ClipSummaryView::from).collect(),
            limit,
            next_cursor: next.map(|n| n.encode()),
        })
    }

    fn get_clip(&self, c: &Caller, id: ClipId, opts: GetOptions) -> Result<ClipView, ApiError> {
        PermissionGuard::check(c, Operation::GetClip)?;
        let clip = self.visible_clip(c, &id)?;
        let mut content = None;
        if opts.include_content && clip.sensitivity.level != SensitivityLevel::Masked {
            let ord = clip
                .representations
                .iter()
                .position(|r| r.mime == Mime::TextPlain);
            if let Some(ord) = ord {
                let ord = u32::try_from(ord).map_err(|_| ApiError::Internal)?;
                let bytes = self
                    .reader()
                    .read_representation(&id, ord)
                    .map_err(|e| map_storage(&e))?;
                let full = String::from_utf8_lossy(&bytes);
                let mut end = full.len().min(CONTENT_MAX_BYTES);
                while !full.is_char_boundary(end) {
                    end -= 1;
                }
                content = Some(ClipContent {
                    text: full[..end].to_string(),
                    truncated: end < full.len(),
                });
            }
        }
        Ok(ClipView { clip, content })
    }

    fn list_collections(&self, c: &Caller) -> Result<Vec<CollectionView>, ApiError> {
        PermissionGuard::check(c, Operation::ListCollections)?;
        Err(NOT_YET)
    }

    fn create_collection(&self, c: &Caller, _name: &str) -> Result<CollectionView, ApiError> {
        PermissionGuard::check(c, Operation::CreateCollection)?;
        Err(NOT_YET)
    }

    fn add_clip_to_collection(
        &self,
        c: &Caller,
        _clip: ClipId,
        _col: CollectionId,
    ) -> Result<(), ApiError> {
        PermissionGuard::check(c, Operation::AddClipToCollection)?;
        Err(NOT_YET)
    }

    fn update_clip_note(
        &self,
        c: &Caller,
        _clip: ClipId,
        _note: Option<String>,
    ) -> Result<(), ApiError> {
        PermissionGuard::check(c, Operation::UpdateClipNote)?;
        Err(NOT_YET)
    }

    fn update_clip_tags(
        &self,
        c: &Caller,
        _clip: ClipId,
        _tags: Vec<String>,
    ) -> Result<Vec<TagView>, ApiError> {
        PermissionGuard::check(c, Operation::UpdateClipTags)?;
        Err(NOT_YET)
    }

    fn pin_clip(&self, c: &Caller, clip: ClipId, pinned: bool) -> Result<(), ApiError> {
        PermissionGuard::check(c, Operation::PinClip)?;
        // Masked clips do not exist for non-UI callers.
        if *c != Caller::Ui {
            self.visible_clip(c, &clip)?;
        }
        // The actor decides (it rejects masked clips) so the check and the write cannot race.
        self.ask(|reply, deadline| ActorCommand::SetPinned {
            id: clip,
            pinned,
            deadline,
            reply,
        })
    }

    fn copy_clip_to_clipboard(
        &self,
        c: &Caller,
        clip: ClipId,
        mode: CopyMode,
    ) -> Result<(), ApiError> {
        PermissionGuard::check(c, Operation::CopyClipToClipboard)?;
        // Hides masked clips from non-UI callers; the write itself is the actor's.
        self.visible_clip(c, &clip)?;
        self.ask(|reply, deadline| ActorCommand::CopyBack {
            id: clip,
            plain: mode == CopyMode::PlainText,
            deadline,
            reply,
        })
    }

    fn delete_clip(&self, c: &Caller, clip: ClipId) -> Result<(), ApiError> {
        PermissionGuard::check(c, Operation::DeleteClip)?;
        if *c != Caller::Ui {
            self.visible_clip(c, &clip)?;
        }
        self.ask(|reply, deadline| ActorCommand::DeleteClip {
            id: clip,
            deadline,
            reply,
        })
    }

    fn get_storage_stats(&self, c: &Caller) -> Result<StorageStatsView, ApiError> {
        PermissionGuard::check(c, Operation::GetStorageStats)?;
        let st = self.reader().stats().map_err(|e| map_storage(&e))?;
        let skipped = self
            .cap
            .skip_counters()
            .iter()
            .map(|(r, n)| (r.name().to_string(), n))
            .collect();
        Ok(StorageStatsView {
            clips: st.clips,
            pinned: st.pinned,
            by_kind: st
                .by_kind
                .into_iter()
                .map(|(kind, count)| KindCount { kind, count })
                .collect(),
            db_bytes: st.db_bytes,
            blob_bytes: st.blob_bytes,
            capture: self.mode_name(),
            skipped,
        })
    }

    fn export_collection(&self, c: &Caller, _col: CollectionId) -> Result<ExportResult, ApiError> {
        PermissionGuard::check(c, Operation::ExportCollection)?;
        Err(NOT_YET)
    }

    /// Applies the mode to the actor first, then saves it. If the save fails the mode stays
    /// applied at runtime and the call returns `Err`. That is intentional: the user's privacy
    /// choice (pause, private) wins over persistence.
    fn set_capture_mode(&self, c: &Caller, mode: CaptureMode) -> Result<(), ApiError> {
        PermissionGuard::check(c, Operation::SetCaptureMode)?;
        let mut s = lock(&self.settings);
        self.cap.send(ActorCommand::SetMode(mode))?;
        s.privacy.mode = mode;
        if self.persist {
            s.save(&self.settings_path)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::envelope::to_envelope;
    use darkclip_core::{
        canonical_hash, classify, ClipboardSource, Lang, Platform, PrivacyPolicy,
        RawRepresentation, SensitiveCategory, SensitiveHandling, Sensitivity, SourceConfidence,
    };
    use darkclip_platform::fake::{FakeCopy, FakeHandle, FakePlatform};
    use darkclip_platform::PlatformFormat;
    use darkclip_storage::NewClip;
    use std::path::Path;
    use std::sync::atomic::{AtomicI64, Ordering};
    use std::sync::mpsc::Receiver;
    use std::time::Instant;

    const T0: i64 = 1_795_320_000_000;
    const WAIT: Duration = Duration::from_secs(5);

    struct TestClock(AtomicI64);
    impl Clock for TestClock {
        fn now(&self) -> Timestamp {
            Timestamp(self.0.load(Ordering::SeqCst))
        }
    }

    fn raw(text: &str) -> RawRepresentation {
        RawRepresentation {
            mime: Mime::TextPlain,
            platform_format: "CF_UNICODETEXT".into(),
            bytes: text.as_bytes().to_vec(),
            width: None,
            height: None,
            truncated: false,
        }
    }

    fn new_clip(text: &str, masked: bool) -> NewClip {
        let reps = vec![raw(text)];
        let mut classified = classify(&reps);
        let hash = canonical_hash(classified.kind, &reps, None);
        let mut sensitivity = Sensitivity::default();
        let mut expires_at = None;
        if masked {
            classified.preview = "[masked api key]".into();
            sensitivity = Sensitivity {
                level: SensitivityLevel::Masked,
                categories: vec![SensitiveCategory::ApiKey],
            };
            expires_at = Some(Timestamp(T0 + 3_600_000));
        }
        NewClip {
            classified,
            hash,
            reps,
            source: Some(ClipboardSource {
                app_id: "org.example.editor".into(),
                app_name: None,
                platform: Platform::Windows,
                confidence: SourceConfidence::Owner,
            }),
            sensitivity,
            expires_at,
            thumbnail_png: None,
        }
    }

    fn text_copy(s: &str) -> FakeCopy {
        FakeCopy {
            formats: vec![(
                PlatformFormat {
                    name: "CF_UNICODETEXT".into(),
                    mime: Some(Mime::TextPlain),
                    size_hint: Some(s.len() as u64),
                },
                s.as_bytes().to_vec(),
            )],
            markers: Vec::new(),
            source: None,
        }
    }

    struct H {
        dir: tempfile::TempDir,
        app: Option<App>,
        fake: FakeHandle,
        events: Receiver<AppEvent>,
        ids: Vec<ClipId>,
    }

    impl H {
        fn app(&self) -> &App {
            self.app.as_ref().unwrap()
        }
        fn wait_added(&self) -> ClipId {
            let deadline = Instant::now() + WAIT;
            loop {
                let left = deadline.saturating_duration_since(Instant::now());
                match self.events.recv_timeout(left) {
                    Ok(AppEvent::ClipAdded(id)) => return id,
                    Ok(_) => {}
                    Err(_) => panic!("timed out waiting for ClipAdded"),
                }
            }
        }
    }

    impl Drop for H {
        fn drop(&mut self) {
            if let Some(a) = self.app.take() {
                let _ = a.shutdown();
            }
        }
    }

    /// Seeds `(text, masked)` clips, then starts the App on the same directory.
    fn start(seed: &[(&str, bool)], settings: Option<Settings>) -> H {
        start_with_file(seed, |p| {
            if let Some(s) = settings {
                s.save(p).unwrap();
            }
        })
    }

    /// Like `start`, with the settings file prepared by `prepare` (given its path).
    fn start_with_file(seed: &[(&str, bool)], prepare: impl FnOnce(&Path)) -> H {
        let dir = tempfile::tempdir().unwrap();
        let clock: Arc<dyn Clock> = Arc::new(TestClock(AtomicI64::new(T0)));
        let mut ids = Vec::new();
        {
            let mut s = Storage::open(dir.path(), OpenMode::ReadWrite, Arc::clone(&clock)).unwrap();
            for (text, masked) in seed {
                match s.upsert_clip(&new_clip(text, *masked)).unwrap() {
                    darkclip_storage::UpsertOutcome::Inserted(id)
                    | darkclip_storage::UpsertOutcome::Bumped(id) => ids.push(id),
                }
            }
        }
        let settings_path = dir.path().join("settings.json");
        prepare(&settings_path);
        let (platform, fake) = FakePlatform::new();
        let (tx, events) = mpsc::channel();
        let cfg = CaptureConfig {
            debounce_ms: 5,
            open_retries: 10,
            retry_delay_ms: 1,
            lang: Lang::En,
        };
        let (app, _) = App::start(
            platform.bundle(),
            dir.path(),
            &settings_path,
            clock,
            cfg,
            tx,
        )
        .unwrap();
        H {
            dir,
            app: Some(app),
            fake,
            events,
            ids,
        }
    }

    fn query(text: Option<&str>) -> SearchQuery {
        SearchQuery {
            text: text.map(str::to_string),
            kinds: Vec::new(),
            pinned_only: false,
            since: None,
            include_masked: false,
            limit: 50,
            cursor: None,
        }
    }

    fn mcp() -> Caller {
        Caller::Mcp {
            client_id: "claude-code".into(),
        }
    }

    #[test]
    fn capabilities_exposed() {
        let h = start(&[], None);
        let caps = h.app().capabilities();
        assert_eq!(caps.backend, "fake");
        assert_eq!(caps.write, darkclip_platform::Support::Native);
    }

    #[test]
    fn mcp_caller_denied_by_default() {
        let h = start(&[("plain clip", false)], None);
        let a = h.app();
        let c = mcp();
        let id = h.ids[0];
        let col = CollectionId::new_v7(Timestamp(T0));
        let denied = |e: ApiError| matches!(e, ApiError::PermissionDenied(_));
        assert!(denied(a.search_clips(&c, query(None)).unwrap_err()));
        assert!(denied(
            a.get_clip(&c, id, GetOptions::default()).unwrap_err()
        ));
        assert!(denied(a.list_collections(&c).unwrap_err()));
        assert!(denied(a.create_collection(&c, "x").unwrap_err()));
        assert!(denied(a.add_clip_to_collection(&c, id, col).unwrap_err()));
        assert!(denied(a.update_clip_note(&c, id, None).unwrap_err()));
        assert!(denied(a.update_clip_tags(&c, id, vec![]).unwrap_err()));
        assert!(denied(a.pin_clip(&c, id, true).unwrap_err()));
        assert!(denied(
            a.copy_clip_to_clipboard(&c, id, CopyMode::Original)
                .unwrap_err()
        ));
        assert!(denied(a.delete_clip(&c, id).unwrap_err()));
        assert!(denied(a.export_collection(&c, col).unwrap_err()));
        assert!(denied(
            a.set_capture_mode(&c, CaptureMode::Private).unwrap_err()
        ));
        assert!(denied(a.clear_history(&c, false).unwrap_err()));
        // Counts only: allowed.
        let st = a.get_storage_stats(&c).unwrap();
        assert_eq!(st.clips, 1);
        // Nothing changed.
        assert_eq!(a.status().capture, CaptureStatus::Active);
        assert!(a.get_clip(&Caller::Ui, id, GetOptions::default()).is_ok());
    }

    #[test]
    fn search_hides_masked_for_non_ui_callers() {
        let h = start(&[("visible note", false), ("hidden secret", true)], None);
        let a = h.app();
        let ui = a.search_clips(&Caller::Ui, query(None)).unwrap();
        assert_eq!(ui.items.len(), 2);
        let m = ui.items.iter().find(|s| s.masked).unwrap();
        assert_eq!(m.preview, "[masked api key]");
        let cli = a.search_clips(&Caller::Cli, query(None)).unwrap();
        assert_eq!(cli.items.len(), 1);
        assert_eq!(cli.items[0].preview, "visible note");
        assert!(!cli.items.iter().any(|s| s.masked));
        // The caller cannot opt in.
        let mut q = query(None);
        q.include_masked = true;
        assert_eq!(a.search_clips(&Caller::Cli, q).unwrap().items.len(), 1);
        // Nor reach it by id.
        let masked_id = m.id;
        assert_eq!(
            a.get_clip(&Caller::Cli, masked_id, GetOptions::default())
                .unwrap_err(),
            ApiError::NotFound
        );
        assert_eq!(
            a.copy_clip_to_clipboard(&Caller::Cli, masked_id, CopyMode::Original)
                .unwrap_err(),
            ApiError::NotFound
        );
        assert!(h.fake.current().is_empty());
    }

    #[test]
    fn ui_summaries_carry_the_source_app_but_never_serialize_it() {
        let h = start(&[("from an editor", false)], None);
        let page = h.app().search_clips(&Caller::Ui, query(None)).unwrap();
        assert_eq!(
            page.items[0].source_app.as_deref(),
            Some("org.example.editor")
        );
        // The envelope shape is unchanged (UI-only field).
        let env = to_envelope(&Ok(page), Lang::En);
        assert!(env["data"][0].get("source_app").is_none());
    }

    #[test]
    fn search_limits_and_cursor() {
        let h = start(&[("alpha one", false), ("alpha two", false)], None);
        let a = h.app();
        let mut q = query(Some("alpha"));
        q.limit = 500;
        let page = a.search_clips(&Caller::Ui, q).unwrap();
        assert_eq!(page.limit, 200);
        assert_eq!(page.items.len(), 2);
        // Text searches never carry a cursor.
        assert_eq!(page.next_cursor, None);
        let env = to_envelope(&Ok(page), Lang::En);
        assert!(env["page"].get("next_cursor").is_none());
        let mut zero = query(None);
        zero.limit = 0;
        assert_eq!(
            a.search_clips(&Caller::Ui, zero).unwrap_err(),
            ApiError::InvalidArgument("limit must be at least 1")
        );
    }

    #[test]
    fn copy_clip_routes_through_actor() {
        let h = start(&[("copy me back", false)], None);
        let id = h.ids[0];
        assert_eq!(h.fake.read_calls(), 0);
        h.app()
            .copy_clip_to_clipboard(&Caller::Ui, id, CopyMode::Original)
            .unwrap();
        let cur = h.fake.current();
        assert_eq!(cur.len(), 1);
        assert_eq!(cur[0].mime, Mime::TextPlain);
        assert_eq!(cur[0].bytes, b"copy me back");
        // The write did not read the clipboard, and was not captured again.
        assert_eq!(h.fake.read_calls(), 0);
        assert!(h
            .events
            .recv_timeout(Duration::from_millis(100))
            .map_or(true, |e| !matches!(e, AppEvent::ClipAdded(_))));
        assert_eq!(h.app().get_storage_stats(&Caller::Ui).unwrap().clips, 1);
        // Unknown clip.
        let missing = ClipId::new_v7(Timestamp(T0));
        assert_eq!(
            h.app()
                .copy_clip_to_clipboard(&Caller::Ui, missing, CopyMode::PlainText)
                .unwrap_err(),
            ApiError::NotFound
        );
    }

    #[test]
    fn pin_masked_clip_rejected() {
        let h = start(&[("normal", false), ("secret", true)], None);
        let a = h.app();
        let masked = a
            .search_clips(&Caller::Ui, query(None))
            .unwrap()
            .items
            .into_iter()
            .find(|s| s.masked)
            .unwrap()
            .id;
        assert_eq!(
            a.pin_clip(&Caller::Ui, masked, true).unwrap_err(),
            ApiError::InvalidArgument("masked clips cannot be pinned")
        );
        // Unpinning a masked clip is harmless; pinning a normal one works.
        assert!(a.pin_clip(&Caller::Ui, masked, false).is_ok());
        a.pin_clip(&Caller::Ui, h.ids[0], true).unwrap();
        let mut q = query(None);
        q.pinned_only = true;
        let pinned = a.search_clips(&Caller::Ui, q).unwrap();
        assert_eq!(pinned.items.len(), 1);
        assert_eq!(pinned.items[0].id, h.ids[0]);
        a.pin_clip(&Caller::Ui, h.ids[0], false).unwrap();
        assert_eq!(a.get_storage_stats(&Caller::Ui).unwrap().pinned, 0);
        assert_eq!(
            a.pin_clip(&Caller::Ui, ClipId::new_v7(Timestamp(T0)), true)
                .unwrap_err(),
            ApiError::NotFound
        );
    }

    #[test]
    fn delete_and_clear_run_on_the_actor() {
        let h = start(&[("one", false), ("two", false), ("three", false)], None);
        let a = h.app();
        a.pin_clip(&Caller::Ui, h.ids[2], true).unwrap();
        a.delete_clip(&Caller::Ui, h.ids[0]).unwrap();
        assert_eq!(
            a.delete_clip(&Caller::Ui, h.ids[0]).unwrap_err(),
            ApiError::NotFound
        );
        assert_eq!(a.get_storage_stats(&Caller::Ui).unwrap().clips, 2);
        assert_eq!(a.clear_history(&Caller::Ui, true).unwrap(), 1);
        assert_eq!(a.get_storage_stats(&Caller::Ui).unwrap().clips, 1);
        assert_eq!(a.clear_history(&Caller::Ui, false).unwrap(), 1);
        assert_eq!(a.get_storage_stats(&Caller::Ui).unwrap().clips, 0);
    }

    #[test]
    fn get_clip_content_only_when_asked() {
        let h = start(&[("some body text", false), ("secret", true)], None);
        let a = h.app();
        let without = a
            .get_clip(&Caller::Cli, h.ids[0], GetOptions::default())
            .unwrap();
        assert!(without.content.is_none());
        let with = a
            .get_clip(
                &Caller::Cli,
                h.ids[0],
                GetOptions {
                    include_content: true,
                },
            )
            .unwrap();
        assert_eq!(with.content.unwrap().text, "some body text");
        // Masked content is never returned, not even to the UI.
        let masked = a
            .get_clip(
                &Caller::Ui,
                h.ids[1],
                GetOptions {
                    include_content: true,
                },
            )
            .unwrap();
        assert!(masked.content.is_none());
        assert_eq!(masked.clip.preview, "[masked api key]");
    }

    #[test]
    fn unsupported_methods_report_not_available() {
        let h = start(&[("x", false)], None);
        let a = h.app();
        let col = CollectionId::new_v7(Timestamp(T0));
        let nope = ApiError::InvalidArgument("not available until P1");
        assert_eq!(a.list_collections(&Caller::Ui).unwrap_err(), nope);
        assert_eq!(a.create_collection(&Caller::Cli, "n").unwrap_err(), nope);
        assert_eq!(
            a.add_clip_to_collection(&Caller::Ui, h.ids[0], col)
                .unwrap_err(),
            nope
        );
        assert_eq!(
            a.update_clip_note(&Caller::Ui, h.ids[0], None).unwrap_err(),
            nope
        );
        assert_eq!(
            a.update_clip_tags(&Caller::Ui, h.ids[0], vec![])
                .unwrap_err(),
            nope
        );
        assert_eq!(a.export_collection(&Caller::Ui, col).unwrap_err(), nope);
    }

    #[test]
    fn status_cause_recovery() {
        let added = AppEvent::ClipAdded(ClipId::new_v7(Timestamp(T0)));
        let mut st = StatusState::default();
        // Storage write failure: cleared by the next successful store only.
        st.apply(&AppEvent::Unavailable("storage write failed"));
        assert_eq!(st.current(), Some("storage write failed"));
        st.apply(&AppEvent::ModeChanged(CaptureMode::Private));
        st.apply(&AppEvent::RetentionOk);
        assert_eq!(st.current(), Some("storage write failed"));
        st.apply(&added);
        assert_eq!(st.current(), None);
        // Retention failure: cleared by a successful retention, not by stores or mode changes.
        st.apply(&AppEvent::Unavailable("retention failed"));
        st.apply(&added);
        st.apply(&AppEvent::ModeChanged(CaptureMode::Normal));
        assert_eq!(st.current(), Some("retention failed"));
        st.apply(&AppEvent::RetentionOk);
        assert_eq!(st.current(), None);
        // Monitor stopped / platform denial: until restart, nothing clears them.
        for what in ["clipboard monitor stopped", "clipboard access denied"] {
            let mut st = StatusState::default();
            st.apply(&AppEvent::Unavailable(what));
            st.apply(&added);
            st.apply(&AppEvent::RetentionOk);
            st.apply(&AppEvent::ModeChanged(CaptureMode::Private));
            assert_eq!(st.current(), Some(what));
        }
        // The permanent cause wins over the recoverable ones.
        let mut st = StatusState::default();
        st.apply(&AppEvent::Unavailable("retention failed"));
        st.apply(&AppEvent::Unavailable("clipboard monitor stopped"));
        assert_eq!(st.current(), Some("clipboard monitor stopped"));
    }

    #[test]
    fn api_views_debug_redacts_content() {
        let secret = "TOPSECRET-VALUE";
        let view = ClipSummaryView {
            id: ClipId::new_v7(Timestamp(T0)),
            kind: ClipType::Text,
            preview: secret.into(),
            pinned: false,
            last_copied_at: Timestamp(T0),
            masked: false,
            source_app: Some(secret.into()),
        };
        let content = ClipContent {
            text: secret.into(),
            truncated: false,
        };
        let page = Page {
            items: vec![view.clone()],
            limit: 5,
            next_cursor: None,
        };
        let summary = ClipSummary {
            id: view.id,
            kind: ClipType::Text,
            preview: secret.into(),
            pinned: false,
            last_copied_at: Timestamp(T0),
            sensitivity: SensitivityLevel::None,
            thumb: None,
            source_app: None,
        };
        let h = start(&[("clip", false)], None);
        let full = h
            .app()
            .get_clip(
                &Caller::Ui,
                h.ids[0],
                GetOptions {
                    include_content: true,
                },
            )
            .unwrap();
        for d in [
            format!("{view:?}"),
            format!("{view:#?}"),
            format!("{content:?}"),
            format!("{content:#?}"),
            format!("{page:?}"),
            format!("{page:#?}"),
            format!("{summary:?}"),
            format!("{summary:#?}"),
            format!("{full:?}"),
            format!("{full:#?}"),
        ] {
            assert!(!d.contains(secret), "{d}");
            assert!(!d.contains("clip\""), "{d}");
        }
        let ClipView {
            content: Some(c), ..
        } = full
        else {
            panic!("content expected");
        };
        assert!(!format!("{c:?}").contains("clip"));
    }

    #[test]
    fn update_settings_keeps_runtime_mode() {
        let h = start(&[], None);
        let a = h.app();
        let snapshot = a.settings();
        assert_eq!(snapshot.privacy.mode, CaptureMode::Normal);
        a.set_capture_mode(&Caller::Ui, CaptureMode::Private)
            .unwrap();
        let mut changed = snapshot;
        changed.ui.language = Lang::Vi;
        a.update_settings(changed).unwrap();
        // In memory, saved, and (below) in the actor.
        assert_eq!(a.settings().privacy.mode, CaptureMode::Private);
        assert_eq!(a.settings().ui.language, Lang::Vi);
        assert_eq!(a.status().capture, CaptureStatus::Private);
        let (saved, _) = Settings::load(&h.dir.path().join("settings.json"));
        assert_eq!(saved.privacy.mode, CaptureMode::Private);
        assert_eq!(saved.ui.language, Lang::Vi);
        // The actor still blocks capture.
        h.fake.copy(text_copy("must not be captured"));
        let sentinel_wait = h.events.recv_timeout(Duration::from_millis(300));
        assert!(!matches!(sentinel_wait, Ok(AppEvent::ClipAdded(_))));
        assert_eq!(a.get_storage_stats(&Caller::Ui).unwrap().clips, 0);
    }

    #[test]
    fn update_settings_save_failure_changes_nothing() {
        use darkclip_core::sensitive::{masked_preview, scan};
        let s0 = Settings {
            privacy: PrivacyPolicy {
                sensitive_handling: SensitiveHandling::StoreMasked,
                ..PrivacyPolicy::default()
            },
            ui: crate::settings::UiSettings {
                language: Lang::Vi,
                ..Default::default()
            },
            ..Settings::default()
        };
        let h = start(&[], Some(s0.clone()));
        let a = h.app();
        let path = h.dir.path().join("settings.json");
        let before = std::fs::read(&path).unwrap();
        // Make the save fail: a directory takes the place of the file.
        std::fs::remove_file(&path).unwrap();
        std::fs::create_dir(&path).unwrap();
        let mut bad = s0.clone();
        bad.ui.language = Lang::En;
        bad.retention.max_items = 7;
        assert!(a.update_settings(bad).is_err());
        // Memory and file keep the old settings.
        assert_eq!(a.settings(), s0);
        assert!(path.is_dir());
        assert!(!before.is_empty());
        // The actor keeps the old policy: masked previews are still Vietnamese.
        let key = format!(
            "key {}{}",
            concat!("s", "k-"),
            "aB3dE5gH7jK9mN1pQ2rS4tU6vW8xY0zaB3dE5gH7jK9m"
        );
        h.fake.copy(text_copy(&key));
        let id = h.wait_added();
        let hint = scan(&key, &s0.privacy.sensitive_categories_enabled)
            .mask_hint
            .unwrap();
        let preview = a
            .get_clip(&Caller::Ui, id, GetOptions::default())
            .unwrap()
            .clip
            .preview;
        assert_eq!(preview, masked_preview(&hint, Lang::Vi));
    }

    #[test]
    fn set_capture_mode_save_failure_keeps_mode_and_errs() {
        let h = start(&[], None);
        let a = h.app();
        let path = h.dir.path().join("settings.json");
        std::fs::create_dir(&path).unwrap();
        // `persist` is true (the file did not exist at start), so the save is attempted.
        assert!(a
            .set_capture_mode(&Caller::Ui, CaptureMode::Private)
            .is_err());
        assert_eq!(a.status().capture, CaptureStatus::Private);
        h.fake.copy(text_copy("not captured while private"));
        assert!(!matches!(
            h.events.recv_timeout(Duration::from_millis(300)),
            Ok(AppEvent::ClipAdded(_))
        ));
    }

    #[test]
    fn indefinite_pause_is_saved_and_loads_back() {
        let h = start(&[], None);
        let a = h.app();
        let mode = CaptureMode::Paused {
            until: darkclip_core::FAR_FUTURE,
        };
        a.set_capture_mode(&Caller::Ui, mode).unwrap();
        assert_eq!(a.status().capture, CaptureStatus::Paused);
        let (loaded, outcome) =
            crate::settings::Settings::load(&h.dir.path().join("settings.json"));
        assert_eq!(outcome, crate::settings::LoadOutcome::Loaded);
        assert_eq!(loaded.privacy.mode, mode);
    }

    #[test]
    fn newer_settings_file_is_never_overwritten() {
        let mut newer = String::new();
        let h = start_with_file(&[], |p| {
            let mut v = serde_json::to_value(Settings::default()).unwrap();
            v["settings_version"] = 99.into();
            newer = v.to_string();
            std::fs::write(p, &newer).unwrap();
        });
        let a = h.app();
        assert!(a.status().settings_read_only);
        a.set_capture_mode(&Caller::Ui, CaptureMode::Private)
            .unwrap();
        let mut s = a.settings();
        s.ui.language = Lang::Vi;
        a.update_settings(s).unwrap();
        // Applied in memory, never written.
        assert_eq!(a.status().capture, CaptureStatus::Private);
        assert_eq!(
            std::fs::read_to_string(h.dir.path().join("settings.json")).unwrap(),
            newer
        );
    }

    #[test]
    fn unreadable_settings_are_never_overwritten() {
        let h = start_with_file(&[], |p| std::fs::create_dir(p).unwrap());
        let a = h.app();
        assert!(a.status().settings_read_only);
        // Saving would fail anyway; it is not even tried, so the call succeeds.
        a.set_capture_mode(&Caller::Ui, CaptureMode::Private)
            .unwrap();
        assert!(h.dir.path().join("settings.json").is_dir());
        // A normal start is writable.
        let ok = start(&[], None);
        assert!(!ok.app().status().settings_read_only);
    }

    #[test]
    fn masked_clips_do_not_exist_for_non_ui_mutations() {
        let h = start(&[("normal", false), ("secret", true)], None);
        let a = h.app();
        let masked = h.ids[1];
        assert_eq!(
            a.pin_clip(&Caller::Cli, masked, true).unwrap_err(),
            ApiError::NotFound
        );
        assert_eq!(
            a.pin_clip(&Caller::Cli, masked, false).unwrap_err(),
            ApiError::NotFound
        );
        assert_eq!(
            a.delete_clip(&Caller::Cli, masked).unwrap_err(),
            ApiError::NotFound
        );
        assert_eq!(a.get_storage_stats(&Caller::Ui).unwrap().clips, 2);
        // The UI still can.
        a.delete_clip(&Caller::Ui, masked).unwrap();
    }

    #[test]
    fn get_clip_content_is_cut_on_a_char_boundary() {
        // Three-byte characters: 100 KiB is not a multiple of 3, so the cut must back off.
        let text: String = "€".repeat(40_000);
        assert!(text.len() > CONTENT_MAX_BYTES);
        let h = start(&[(&text, false), ("short", false)], None);
        let a = h.app();
        let big = a
            .get_clip(
                &Caller::Ui,
                h.ids[0],
                GetOptions {
                    include_content: true,
                },
            )
            .unwrap()
            .content
            .unwrap();
        assert!(big.truncated);
        assert!(big.text.len() <= CONTENT_MAX_BYTES);
        assert_eq!(big.text.len() % 3, 0);
        assert!(!big.text.contains('\u{FFFD}'));
        let small = a
            .get_clip(
                &Caller::Ui,
                h.ids[1],
                GetOptions {
                    include_content: true,
                },
            )
            .unwrap()
            .content
            .unwrap();
        assert!(!small.truncated);
    }

    #[test]
    fn stats_skipped_keys_are_fixed_snake_case_names() {
        use darkclip_core::policy::SkipReason;
        const NAMES: [&str; 13] = [
            "disabled",
            "paused",
            "private",
            "self_write",
            "os_concealed",
            "darkshot_excluded",
            "transient",
            "excluded_app",
            "kind_disabled",
            "too_large",
            "nothing_readable",
            "sensitive",
            "busy",
        ];
        let h = start(&[], None);
        let a = h.app();
        a.set_capture_mode(&Caller::Ui, CaptureMode::Private)
            .unwrap();
        h.fake.copy(text_copy("skipped while private"));
        let deadline = Instant::now() + WAIT;
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            match h.events.recv_timeout(left) {
                Ok(AppEvent::Skipped(SkipReason::Private)) => break,
                Ok(_) => {}
                Err(_) => panic!("timed out waiting for Skipped(Private)"),
            }
        }
        let skipped = a.get_storage_stats(&Caller::Ui).unwrap().skipped;
        let keys: Vec<&str> = skipped.keys().map(String::as_str).collect();
        assert_eq!(keys, vec!["private"]);
        assert_eq!(skipped.get("private"), Some(&1));
        assert!(keys.iter().all(|k| NAMES.contains(k)));
    }

    #[test]
    fn status_follows_events_and_mode() {
        let h = start(&[], None);
        let a = h.app();
        assert_eq!(a.status().capture, CaptureStatus::Active);
        a.set_capture_mode(&Caller::Ui, CaptureMode::Private)
            .unwrap();
        assert_eq!(a.status().capture, CaptureStatus::Private);
        assert_eq!(a.get_storage_stats(&Caller::Ui).unwrap().capture, "private");
        // The mode is persisted.
        let (saved, _) = Settings::load(&h.dir.path().join("settings.json"));
        assert_eq!(saved.privacy.mode, CaptureMode::Private);
        let until = Timestamp(T0 + 60_000);
        a.set_capture_mode(&Caller::Ui, CaptureMode::Paused { until })
            .unwrap();
        assert_eq!(a.status().capture, CaptureStatus::Paused);
        a.set_capture_mode(&Caller::Ui, CaptureMode::Normal)
            .unwrap();
        assert_eq!(a.status().capture, CaptureStatus::Active);
    }

    #[test]
    fn settings_language_reaches_the_actor() {
        let s = Settings {
            privacy: PrivacyPolicy {
                sensitive_handling: SensitiveHandling::StoreMasked,
                ..PrivacyPolicy::default()
            },
            ui: crate::settings::UiSettings {
                language: Lang::Vi,
                ..Default::default()
            },
            ..Settings::default()
        };
        let h = start(&[], Some(s.clone()));
        let key = format!(
            "key {}{}",
            concat!("s", "k-"),
            "aB3dE5gH7jK9mN1pQ2rS4tU6vW8xY0zaB3dE5gH7jK9m"
        );
        h.fake.copy(text_copy(&key));
        let id = h.wait_added();
        let vi = h
            .app()
            .get_clip(&Caller::Ui, id, GetOptions::default())
            .unwrap()
            .clip
            .preview;
        // Switch to English through update_settings: later captures follow.
        let mut en = s;
        en.ui.language = Lang::En;
        h.app().update_settings(en).unwrap();
        assert_eq!(h.app().lang(), Lang::En);
        h.fake.copy(text_copy(&format!("{key} again")));
        let id2 = h.wait_added();
        let en_preview = h
            .app()
            .get_clip(&Caller::Ui, id2, GetOptions::default())
            .unwrap()
            .clip
            .preview;
        assert_ne!(vi, en_preview);
        // OS markers cannot be switched off through the API either.
        let mut bad = h.app().settings();
        bad.privacy.respect_os_markers = false;
        assert!(matches!(
            h.app().update_settings(bad),
            Err(ApiError::InvalidArgument(_))
        ));
    }
}
