//! The application API (`ClipApi`, ai-integration §1) and the `App` that implements it.
//!
//! Write path (system-overview §2 rule 6: the capture actor owns the only writable
//! `Storage`): `App` never writes. Reads (search, get, stats) use a second, read-only
//! `Storage` connection (WAL allows it next to the actor's writer). Every mutation (pin,
//! delete, clear history, copy back) is an `ActorCommand` executed on the actor thread, so
//! writes stay serialized with capture and retention.
//!
//! Errors carry no clip content (GC-06); `Caller::Mcp` never sees masked clips.

use crate::capture::{ActorCommand, CaptureHandle, MaintOp, MaintReply, OrganizeOp, OrganizeReply};
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

mod transfer;
mod transform_ui;
pub use crate::capture::SaveOutcome;
use serde::Serialize;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Sender};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};
pub use transfer::{ExportSelection, ImportReport};
pub use transform_ui::TransformPreview;

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
    /// Member of Favorites. Serialized only when true.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub favorite: bool,
    /// Tag names, sorted. Serialized only when there are any.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<String>,
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
            favorite: false,
            tags: Vec::new(),
        }
    }
}

#[derive(Clone, PartialEq, Eq, Serialize)]
pub struct ClipContent {
    pub text: String,
    /// The text was cut at the cap (100 KiB for `get_clip`, 32 KiB for the popup).
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

/// 8-bit RGBA pixels for the popup, decoded off the UI thread. UI only (never serialized).
#[derive(Clone, PartialEq, Eq)]
pub struct PixelsView {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

/// Never prints pixels (GC-06).
impl std::fmt::Debug for PixelsView {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PixelsView")
            .field("width", &self.width)
            .field("height", &self.height)
            .finish_non_exhaustive()
    }
}

/// An image clip's detail: the original size and the stored 256 px thumbnail, decoded.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImageDetailView {
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub thumbnail: Option<PixelsView>,
}

/// What the popup's detail pane needs beyond the preview (Task 206). UI only: not part of
/// `ClipApi`, so the CLI and MCP surfaces are unchanged. For a masked clip every field but
/// `masked` is empty and nothing was read from storage.
#[derive(Clone, PartialEq, Eq, Default)]
pub struct ClipDetailView {
    pub masked: bool,
    pub image: Option<ImageDetailView>,
    /// `text/uri-list` lines of a file clip (at most `MAX_FILE_URIS`). Paths: shown in the UI,
    /// never logged.
    pub file_uris: Vec<String>,
    /// How many lines the clip has in all.
    pub file_total: usize,
}

/// Never prints file paths (GC-06).
impl std::fmt::Debug for ClipDetailView {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ClipDetailView")
            .field("masked", &self.masked)
            .field("image", &self.image)
            .field("file_total", &self.file_total)
            .finish_non_exhaustive()
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
    /// The system Favorites collection: it cannot be renamed or deleted.
    pub system: bool,
    pub clip_count: u64,
}

impl From<darkclip_core::Collection> for CollectionView {
    fn from(c: darkclip_core::Collection) -> Self {
        Self {
            id: c.id,
            name: c.name,
            system: c.kind == darkclip_core::CollectionKind::System,
            clip_count: c.clip_count,
        }
    }
}

/// A tag with the number of clips that carry it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TagCountView {
    pub id: TagId,
    pub name: String,
    pub clip_count: u64,
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
    /// Masked or sensitive clips left out (they are never exported; Task 405).
    pub excluded_sensitive: u64,
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
    pub favorites: u64,
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
    fn rename_collection(&self, c: &Caller, col: CollectionId, name: &str) -> Result<(), ApiError>;
    /// User collections only; the clips stay in the history.
    fn delete_collection(&self, c: &Caller, col: CollectionId) -> Result<(), ApiError>;
    fn remove_clip_from_collection(
        &self,
        c: &Caller,
        col: CollectionId,
        clip: ClipId,
    ) -> Result<(), ApiError>;
    /// Moves a member of a manually sorted collection to `index` (clamped to the end).
    fn reorder_clip_in_collection(
        &self,
        c: &Caller,
        col: CollectionId,
        clip: ClipId,
        index: u32,
    ) -> Result<(), ApiError>;
    fn set_favorite(&self, c: &Caller, clip: ClipId, favorite: bool) -> Result<(), ApiError>;
    fn list_tags(&self, c: &Caller) -> Result<Vec<TagCountView>, ApiError>;
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

/// The in-process UI methods are for `Caller::Ui` alone.
fn ui_only(c: &Caller) -> Result<(), ApiError> {
    if *c == Caller::Ui {
        Ok(())
    } else {
        Err(ApiError::PermissionDenied("UI only"))
    }
}

const MAX_LIMIT: u16 = 200;
const MAX_LIMIT_MCP: u16 = 50;
const CONTENT_MAX_BYTES: usize = 100 * 1024;
/// File names the detail pane may carry (the rest is counted).
pub const MAX_FILE_URIS: usize = 100;
/// Text the popup's detail pane may load (a long wrapped `Text` is slow to lay out).
pub const UI_TEXT_MAX_BYTES: usize = 32 * 1024;
/// A stored thumbnail is at most 256 px; this bounds its decoded size.
const THUMB_RGBA_MAX: u64 = 2 * 1024 * 1024;
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
            AppEvent::ModeChanged(_) | AppEvent::Skipped(_) | AppEvent::OrganizationChanged => {}
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
    /// The data folder (`export_collection` writes under `<data>/exports/`).
    data_dir: PathBuf,
    /// False when the settings file must not be written (see `AppStatus::settings_read_only`).
    persist: bool,
    status: Arc<Mutex<StatusState>>,
    clock: Arc<dyn Clock>,
    /// What the platform adapter reported at start.
    caps: Capabilities,
    pump: Option<JoinHandle<()>>,
}

impl App {
    /// Detail for the popup's pane (Task 206): image size and decoded thumbnail, or the file
    /// list. UI only: any other caller gets `PermissionDenied`. Runs on the caller's thread
    /// (the popup's worker). Masked clips answer `masked: true` without any read of content;
    /// expired or missing: `NotFound`. One reader guard covers the check and the reads, and the
    /// clip is checked again after the read (a clip that turned masked, expired or was replaced
    /// meanwhile answers `NotFound`).
    pub fn ui_clip_detail(&self, c: &Caller, id: ClipId) -> Result<ClipDetailView, ApiError> {
        ui_only(c)?;
        let st = self.reader();
        let clip = self.visible_clip_in(&st, c, &id)?;
        if clip.sensitivity.level == SensitivityLevel::Masked {
            return Ok(ClipDetailView {
                masked: true,
                ..ClipDetailView::default()
            });
        }
        let mut out = ClipDetailView::default();
        match clip.kind {
            ClipType::Image => {
                let rep = clip
                    .representations
                    .iter()
                    .find(|r| r.mime == Mime::ImagePng);
                let png = st.read_thumbnail(&id).map_err(|e| map_storage(&e))?;
                self.recheck(&st, c, &clip)?;
                let thumbnail = png.and_then(|png| {
                    crate::image::decode_png(&png, crate::image::MAX_PIXELS, THUMB_RGBA_MAX)
                        .ok()
                        .map(|d| PixelsView {
                            width: d.width,
                            height: d.height,
                            rgba: d.rgba,
                        })
                });
                out.image = Some(ImageDetailView {
                    width: rep.and_then(|r| r.width),
                    height: rep.and_then(|r| r.height),
                    thumbnail,
                });
            }
            ClipType::Files => {
                let ord = clip
                    .representations
                    .iter()
                    .position(|r| r.mime == Mime::UriList);
                if let Some(ord) = ord {
                    let ord = u32::try_from(ord).map_err(|_| ApiError::Internal)?;
                    let bytes = st
                        .read_representation(&id, ord)
                        .map_err(|e| map_storage(&e))?;
                    self.recheck(&st, c, &clip)?;
                    let text = String::from_utf8_lossy(&bytes);
                    let lines: Vec<&str> = text
                        .lines()
                        .map(str::trim)
                        .filter(|l| !l.is_empty() && !l.starts_with('#'))
                        .collect();
                    out.file_total = lines.len();
                    out.file_uris = lines
                        .into_iter()
                        .take(MAX_FILE_URIS)
                        .map(str::to_string)
                        .collect();
                }
            }
            _ => {}
        }
        Ok(out)
    }

    /// The full image of an image clip, decoded and downscaled to at most `max_side` px
    /// (never upscaled). UI only. Heavy: the popup calls it on its worker thread, on demand.
    /// Masked clips and non-images answer `NotFound` before anything is read.
    pub fn ui_clip_image(
        &self,
        c: &Caller,
        id: ClipId,
        max_side: u32,
    ) -> Result<PixelsView, ApiError> {
        ui_only(c)?;
        let bytes = {
            let st = self.reader();
            let clip = self.visible_clip_in(&st, c, &id)?;
            if clip.sensitivity.level == SensitivityLevel::Masked || clip.kind != ClipType::Image {
                return Err(ApiError::NotFound);
            }
            let ord = clip
                .representations
                .iter()
                .position(|r| r.mime == Mime::ImagePng)
                .ok_or(ApiError::NotFound)?;
            let ord = u32::try_from(ord).map_err(|_| ApiError::Internal)?;
            let bytes = st
                .read_representation(&id, ord)
                .map_err(|e| map_storage(&e))?;
            self.recheck(&st, c, &clip)?;
            bytes
        };
        // Decoding runs without the reader guard.
        let img = crate::image::decode_png(
            &bytes,
            crate::image::MAX_PIXELS,
            crate::image::MAX_RGBA_BYTES,
        )
        .map_err(|_| ApiError::InvalidArgument("image cannot be shown"))?;
        let small = crate::image::downscale(&img, max_side)
            .ok_or(ApiError::InvalidArgument("image cannot be shown"))?;
        Ok(PixelsView {
            width: small.width,
            height: small.height,
            rgba: small.rgba,
        })
    }

    /// The plain text of a clip for the popup's pane, cut at `UI_TEXT_MAX_BYTES` on a character
    /// boundary. UI only. Masked, expired or missing clips (and clips without text) answer
    /// `NotFound`; nothing is read for a masked clip.
    pub fn ui_clip_text(&self, c: &Caller, id: ClipId) -> Result<ClipContent, ApiError> {
        ui_only(c)?;
        let st = self.reader();
        let clip = self.visible_clip_in(&st, c, &id)?;
        self.read_text(&st, c, &clip, UI_TEXT_MAX_BYTES)?
            .ok_or(ApiError::NotFound)
    }

    /// The clip's plain text cut at `cap` bytes, or `None` for a masked clip or one without a
    /// text representation. Re-checks the clip after the read.
    fn read_text(
        &self,
        st: &Storage,
        c: &Caller,
        clip: &Clip,
        cap: usize,
    ) -> Result<Option<ClipContent>, ApiError> {
        if clip.sensitivity.level == SensitivityLevel::Masked {
            return Ok(None);
        }
        let Some(ord) = clip
            .representations
            .iter()
            .position(|r| r.mime == Mime::TextPlain)
        else {
            return Ok(None);
        };
        let ord = u32::try_from(ord).map_err(|_| ApiError::Internal)?;
        let bytes = st
            .read_representation(&clip.id, ord)
            .map_err(|e| map_storage(&e))?;
        self.recheck(st, c, clip)?;
        let full = String::from_utf8_lossy(&bytes);
        let mut end = full.len().min(cap);
        while !full.is_char_boundary(end) {
            end -= 1;
        }
        Ok(Some(ClipContent {
            text: full[..end].to_string(),
            truncated: end < full.len(),
        }))
    }

    /// After a content read: the clip must still exist, be visible to `c`, unexpired, not
    /// masked and unchanged (same content hash, so its representations were not replaced).
    /// Otherwise the bytes just read are dropped by the caller via `NotFound`.
    fn recheck(&self, st: &Storage, c: &Caller, before: &Clip) -> Result<(), ApiError> {
        let now = self.visible_clip_in(st, c, &before.id)?;
        if now.sensitivity.level == SensitivityLevel::Masked
            || now.content_hash != before.content_hash
            // A same-hash bump may reorder representations: the ordinal read must still mean
            // the same MIME.
            || now.representations != before.representations
        {
            return Err(ApiError::NotFound);
        }
        Ok(())
    }

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
            data_dir: data_dir.to_path_buf(),
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

    /// How many clips `clear_history(keep_pinned)` would delete: the count the confirmation
    /// shows. With `keep_pinned`, pinned clips and favorites stay.
    pub fn count_clearable(&self, c: &Caller, keep_pinned: bool) -> Result<u64, ApiError> {
        PermissionGuard::check(c, Operation::ClearHistory)?;
        self.reader()
            .count_clearable(keep_pinned)
            .map_err(|e| map_storage(&e))
    }

    /// How many clips `delete_older_than(days)` would delete.
    pub fn count_older_than(&self, c: &Caller, days: u32) -> Result<u64, ApiError> {
        PermissionGuard::check(c, Operation::ClearHistory)?;
        self.reader()
            .count_older_than(days)
            .map_err(|e| map_storage(&e))
    }

    /// Deletes clips last copied more than `days` days ago (`days >= 1`), except pinned clips
    /// and favorites. Runs on the actor thread.
    pub fn delete_older_than(&self, c: &Caller, days: u32) -> Result<u64, ApiError> {
        match self.maintain(c, MaintOp::DeleteOlderThan(days))? {
            MaintReply::Deleted(n) => Ok(n),
            _ => Err(ApiError::Internal),
        }
    }

    /// Orphaned blob files and their size, without removing anything.
    pub fn scan_orphans(&self, c: &Caller) -> Result<darkclip_storage::OrphanReport, ApiError> {
        PermissionGuard::check(c, Operation::ClearHistory)?;
        self.reader().scan_orphans().map_err(|e| map_storage(&e))
    }

    /// Removes orphaned blob files now (the startup sweep on demand); reports what it freed.
    pub fn sweep_orphan_blobs(
        &self,
        c: &Caller,
    ) -> Result<darkclip_storage::OrphanReport, ApiError> {
        match self.maintain(c, MaintOp::SweepOrphans)? {
            MaintReply::Orphans(r) => Ok(r),
            _ => Err(ApiError::Internal),
        }
    }

    /// `VACUUM`. `free_bytes` is the free disk space of the data folder as the caller measured
    /// it; `None` (unknown) and anything below the database size are refused.
    pub fn compact_database(
        &self,
        c: &Caller,
        free_bytes: Option<u64>,
    ) -> Result<darkclip_storage::CompactReport, ApiError> {
        PermissionGuard::check(c, Operation::ClearHistory)?;
        let free = free_bytes.ok_or(ApiError::InvalidArgument("free disk space unknown"))?;
        match self.maintain(c, MaintOp::Compact(free))? {
            MaintReply::Compacted(r) => Ok(r),
            _ => Err(ApiError::Internal),
        }
    }

    fn maintain(&self, c: &Caller, op: MaintOp) -> Result<MaintReply, ApiError> {
        PermissionGuard::check(c, Operation::ClearHistory)?;
        self.ask(|reply, deadline| ActorCommand::Maintenance {
            op,
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
        let st = self.reader();
        self.visible_clip_in(&st, c, id)
    }

    /// `visible_clip` on a reader the caller already holds.
    fn visible_clip_in(&self, st: &Storage, c: &Caller, id: &ClipId) -> Result<Clip, ApiError> {
        let clip = st
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

    /// The clip an organisation call targets: `NotFound` when it is missing, expired or
    /// hidden from this caller. `adding` (a membership, note or tag is being added) also
    /// refuses masked clips for the UI: masked clips cannot be organised, only cleared.
    fn organize_target(&self, c: &Caller, id: &ClipId, adding: bool) -> Result<(), ApiError> {
        let clip = self.visible_clip(c, id)?;
        if adding && clip.sensitivity.level == SensitivityLevel::Masked {
            return Err(ApiError::NotFound);
        }
        Ok(())
    }

    fn organize(&self, op: OrganizeOp) -> Result<OrganizeReply, ApiError> {
        self.ask(|reply, deadline| ActorCommand::Organize {
            op,
            deadline,
            reply,
        })
    }

    fn organize_done(&self, op: OrganizeOp) -> Result<(), ApiError> {
        self.organize(op).map(|_| ())
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
        let st = self.reader();
        let (rows, next) = st.search(&q).map_err(|e| map_storage(&e))?;
        let ids: Vec<ClipId> = rows.iter().map(|r| r.id).collect();
        let mut labels = st.clip_labels_batch(&ids).map_err(|e| map_storage(&e))?;
        let items = rows
            .into_iter()
            .map(|row| {
                let id = row.id;
                let mut v = ClipSummaryView::from(row);
                if let Some((favorite, tags)) = labels.remove(&id) {
                    v.favorite = favorite;
                    v.tags = tags;
                }
                v
            })
            .collect();
        drop(st);
        Ok(Page {
            items,
            limit,
            next_cursor: next.map(|n| n.encode()),
        })
    }

    fn get_clip(&self, c: &Caller, id: ClipId, opts: GetOptions) -> Result<ClipView, ApiError> {
        PermissionGuard::check(c, Operation::GetClip)?;
        let st = self.reader();
        let clip = self.visible_clip_in(&st, c, &id)?;
        let content = if opts.include_content {
            self.read_text(&st, c, &clip, CONTENT_MAX_BYTES)?
        } else {
            None
        };
        Ok(ClipView { clip, content })
    }

    fn list_collections(&self, c: &Caller) -> Result<Vec<CollectionView>, ApiError> {
        PermissionGuard::check(c, Operation::ListCollections)?;
        let rows = self
            .reader()
            .list_collections()
            .map_err(|e| map_storage(&e))?;
        Ok(rows.into_iter().map(CollectionView::from).collect())
    }

    fn create_collection(&self, c: &Caller, name: &str) -> Result<CollectionView, ApiError> {
        PermissionGuard::check(c, Operation::CreateCollection)?;
        match self.organize(OrganizeOp::CreateCollection(name.to_string()))? {
            OrganizeReply::Collection(col) => Ok(col.into()),
            _ => Err(ApiError::Internal),
        }
    }

    fn add_clip_to_collection(
        &self,
        c: &Caller,
        clip: ClipId,
        col: CollectionId,
    ) -> Result<(), ApiError> {
        PermissionGuard::check(c, Operation::AddClipToCollection)?;
        self.organize_target(c, &clip, true)?;
        self.organize_done(OrganizeOp::AddToCollection(col, clip))
    }

    fn update_clip_note(
        &self,
        c: &Caller,
        clip: ClipId,
        note: Option<String>,
    ) -> Result<(), ApiError> {
        PermissionGuard::check(c, Operation::UpdateClipNote)?;
        self.organize_target(c, &clip, note.as_deref().is_some_and(|n| !n.is_empty()))?;
        self.organize_done(OrganizeOp::SetNote(clip, note))
    }

    fn update_clip_tags(
        &self,
        c: &Caller,
        clip: ClipId,
        tags: Vec<String>,
    ) -> Result<Vec<TagView>, ApiError> {
        PermissionGuard::check(c, Operation::UpdateClipTags)?;
        self.organize_target(c, &clip, !tags.is_empty())?;
        match self.organize(OrganizeOp::SetTags(clip, tags))? {
            OrganizeReply::Tags(t) => Ok(t
                .into_iter()
                .map(|t| TagView {
                    id: t.id,
                    name: t.name,
                })
                .collect()),
            _ => Err(ApiError::Internal),
        }
    }

    fn rename_collection(&self, c: &Caller, col: CollectionId, name: &str) -> Result<(), ApiError> {
        PermissionGuard::check(c, Operation::RenameCollection)?;
        self.organize_done(OrganizeOp::RenameCollection(col, name.to_string()))
    }

    fn delete_collection(&self, c: &Caller, col: CollectionId) -> Result<(), ApiError> {
        PermissionGuard::check(c, Operation::DeleteCollection)?;
        self.organize_done(OrganizeOp::DeleteCollection(col))
    }

    fn remove_clip_from_collection(
        &self,
        c: &Caller,
        col: CollectionId,
        clip: ClipId,
    ) -> Result<(), ApiError> {
        PermissionGuard::check(c, Operation::RemoveClipFromCollection)?;
        self.organize_target(c, &clip, false)?;
        self.organize_done(OrganizeOp::RemoveFromCollection(col, clip))
    }

    fn reorder_clip_in_collection(
        &self,
        c: &Caller,
        col: CollectionId,
        clip: ClipId,
        index: u32,
    ) -> Result<(), ApiError> {
        PermissionGuard::check(c, Operation::ReorderClipInCollection)?;
        self.organize_target(c, &clip, false)?;
        self.organize_done(OrganizeOp::Reorder(col, clip, index))
    }

    fn set_favorite(&self, c: &Caller, clip: ClipId, favorite: bool) -> Result<(), ApiError> {
        PermissionGuard::check(c, Operation::SetFavorite)?;
        self.organize_target(c, &clip, favorite)?;
        self.organize_done(OrganizeOp::SetFavorite(clip, favorite))
    }

    fn list_tags(&self, c: &Caller) -> Result<Vec<TagCountView>, ApiError> {
        PermissionGuard::check(c, Operation::ListTags)?;
        let rows = self.reader().list_tags().map_err(|e| map_storage(&e))?;
        Ok(rows
            .into_iter()
            .map(|t| TagCountView {
                id: t.tag.id,
                name: t.tag.name,
                clip_count: t.clip_count,
            })
            .collect())
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
            favorites: st.favorites,
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

    /// UI: writes `<data>/exports/<id>.dcexport` (T-11) and reports the masked or sensitive
    /// clips left out (Task 405). The CLI gets it in P2.
    fn export_collection(&self, c: &Caller, col: CollectionId) -> Result<ExportResult, ApiError> {
        PermissionGuard::check(c, Operation::ExportCollection)?;
        if *c != Caller::Ui {
            return Err(NOT_YET);
        }
        self.export_collection_ui(col)
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
        // An existing install (capture on); a test of the first run removes the file.
        Settings::default().save(&settings_path).unwrap();
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
            collection: None,
            tag: None,
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
    fn clear_history_keeps_favorites_and_pinned_through_the_actor() {
        let h = start(
            &[("pin", false), ("fav", false), ("a", false), ("b", false)],
            None,
        );
        let a = h.app();
        let ui = &Caller::Ui;
        a.pin_clip(ui, h.ids[0], true).unwrap();
        a.set_favorite(ui, h.ids[1], true).unwrap();
        assert_eq!(a.count_clearable(ui, true).unwrap(), 2);
        assert_eq!(a.count_clearable(ui, false).unwrap(), 4);
        assert_eq!(a.clear_history(ui, true).unwrap(), 2);
        let st = a.get_storage_stats(ui).unwrap();
        assert_eq!((st.clips, st.pinned, st.favorites), (2, 1, 1));
        assert_eq!(a.clear_history(ui, false).unwrap(), 2);
        assert_eq!(a.get_storage_stats(ui).unwrap().clips, 0);
    }

    #[test]
    fn data_management_runs_on_the_actor_and_is_denied_to_other_callers() {
        let h = start(&[("one", false), ("two", false)], None);
        let a = h.app();
        let ui = &Caller::Ui;
        // Everything in the seed is fresh: nothing is older than a day.
        assert_eq!(a.count_older_than(ui, 1).unwrap(), 0);
        assert_eq!(a.delete_older_than(ui, 1).unwrap(), 0);
        assert_eq!(
            a.delete_older_than(ui, 0).unwrap_err(),
            ApiError::InvalidArgument("days must be 1 or more")
        );
        let shard = h.dir.path().join("blobs").join("ab");
        std::fs::create_dir_all(&shard).unwrap();
        std::fs::write(shard.join("c".repeat(64)), vec![0u8; 500]).unwrap();
        let scan = a.scan_orphans(ui).unwrap();
        assert_eq!((scan.files, scan.bytes), (1, 500));
        let done = a.sweep_orphan_blobs(ui).unwrap();
        assert_eq!((done.files, done.bytes), (1, 500));
        assert!(!shard.join("c".repeat(64)).exists());
        // Compaction: refused without a free-space figure or with too little of it.
        assert!(a.compact_database(ui, None).is_err());
        assert!(a.compact_database(ui, Some(0)).is_err());
        let r = a.compact_database(ui, Some(u64::MAX)).unwrap();
        assert!(r.bytes_after <= r.bytes_before);
        assert_eq!(a.get_storage_stats(ui).unwrap().clips, 2);
        let denied = |e: ApiError| matches!(e, ApiError::PermissionDenied(_));
        let m = &mcp();
        assert!(denied(a.count_clearable(m, true).unwrap_err()));
        assert!(denied(a.count_older_than(m, 1).unwrap_err()));
        assert!(denied(a.delete_older_than(m, 1).unwrap_err()));
        assert!(denied(a.scan_orphans(m).unwrap_err()));
        assert!(denied(a.sweep_orphan_blobs(m).unwrap_err()));
        assert!(denied(a.compact_database(m, Some(u64::MAX)).unwrap_err()));
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
            favorite: false,
            tags: Vec::new(),
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
    fn fresh_install_does_not_capture_until_start() {
        let h = start_with_file(&[], |p| std::fs::remove_file(p).unwrap());
        let a = h.app();
        assert!(a.settings().needs_welcome());
        assert_eq!(a.status().capture, CaptureStatus::Paused);
        h.fake.copy(text_copy("copied before Start"));
        std::thread::sleep(Duration::from_millis(300));
        assert_eq!(a.get_storage_stats(&Caller::Ui).unwrap().clips, 0);
        // Not even read: the policy said no before any clipboard content was touched (GC-03).
        assert_eq!(h.fake.read_calls(), 0);
        // Start: persisted, and capture begins.
        let mut s = a.settings();
        s.finish_onboarding(true);
        a.update_settings(s).unwrap();
        assert_eq!(a.status().capture, CaptureStatus::Active);
        let (saved, o) = Settings::load(&h.dir.path().join("settings.json"));
        assert_eq!(o, crate::settings::LoadOutcome::Loaded);
        assert!(saved.privacy.capture_enabled && !saved.needs_welcome());
        // A copy after Start is captured; the one from before is not in the history. The policy
        // update reaches the capture actor asynchronously, so a copy made in the same instant
        // can still be dropped (never backfilled); copy again until the actor has the new policy.
        let deadline = Instant::now() + WAIT;
        let rows = loop {
            h.fake.copy(text_copy("copied after Start"));
            std::thread::sleep(Duration::from_millis(100));
            let page = a.search_clips(&Caller::Ui, query(None)).unwrap();
            if !page.items.is_empty() || Instant::now() > deadline {
                break page.items;
            }
        };
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].preview, "copied after Start");
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
        std::fs::remove_file(&path).unwrap();
        std::fs::create_dir(&path).unwrap();
        // `persist` is true (the file was readable at start), so the save is attempted.
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
        let h = start_with_file(&[], |p| {
            std::fs::remove_file(p).unwrap();
            std::fs::create_dir(p).unwrap();
        });
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

    fn png_of(w: u32, h: u32) -> Vec<u8> {
        let mut bytes = Vec::new();
        let mut e = png::Encoder::new(&mut bytes, w, h);
        e.set_color(png::ColorType::Rgba);
        e.set_depth(png::BitDepth::Eight);
        let mut wr = e.write_header().unwrap();
        wr.write_image_data(&vec![90u8; (w * h * 4) as usize])
            .unwrap();
        wr.finish().unwrap();
        bytes
    }

    fn image_clip(w: u32, h: u32, masked: bool) -> NewClip {
        let png = png_of(w, h);
        let reps = vec![RawRepresentation {
            mime: Mime::ImagePng,
            platform_format: "PNG".into(),
            bytes: png.clone(),
            width: Some(w),
            height: Some(h),
            truncated: false,
        }];
        let mut n = new_clip("x", masked);
        n.classified = classify(&reps);
        if masked {
            n.classified.preview = "[masked image]".into();
        }
        n.hash = canonical_hash(n.classified.kind, &reps, None);
        n.reps = reps;
        // Even a thumbnail handed to storage must not be served for a masked clip.
        let img = crate::image::decode_png(&png, 1 << 26, 1 << 28).unwrap();
        n.thumbnail_png = Some(crate::image::thumbnail(&img, crate::image::THUMB_MAX_SIDE));
        n
    }

    fn files_clip(uris: &str) -> NewClip {
        let reps = vec![RawRepresentation {
            mime: Mime::UriList,
            platform_format: "CF_HDROP".into(),
            bytes: uris.as_bytes().to_vec(),
            width: None,
            height: None,
            truncated: false,
        }];
        let mut n = new_clip("x", false);
        n.classified = classify(&reps);
        n.hash = canonical_hash(n.classified.kind, &reps, None);
        n.reps = reps;
        n
    }

    /// Seeds `clips`, then starts the App on the same directory.
    fn start_with_clips(clips: &[NewClip]) -> H {
        start_with_clips_clock(clips).0
    }

    /// Like `start_with_clips`, also giving the test clock (to move time forward).
    fn start_with_clips_clock(clips: &[NewClip]) -> (H, Arc<TestClock>) {
        let dir = tempfile::tempdir().unwrap();
        let test_clock = Arc::new(TestClock(AtomicI64::new(T0)));
        let clock: Arc<dyn Clock> = test_clock.clone();
        let mut ids = Vec::new();
        {
            let mut s = Storage::open(dir.path(), OpenMode::ReadWrite, Arc::clone(&clock)).unwrap();
            for c in clips {
                match s.upsert_clip(c).unwrap() {
                    darkclip_storage::UpsertOutcome::Inserted(id)
                    | darkclip_storage::UpsertOutcome::Bumped(id) => ids.push(id),
                }
            }
        }
        Settings::default()
            .save(&dir.path().join("settings.json"))
            .unwrap();
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
            &dir.path().join("settings.json"),
            clock,
            cfg,
            tx,
        )
        .unwrap();
        (
            H {
                dir,
                app: Some(app),
                fake,
                events,
                ids,
            },
            test_clock,
        )
    }

    #[test]
    fn ui_detail_of_an_image_has_size_and_decoded_thumbnail() {
        let h = start_with_clips(&[image_clip(600, 40, false)]);
        let d = h.app().ui_clip_detail(&Caller::Ui, h.ids[0]).unwrap();
        assert!(!d.masked);
        let img = d.image.unwrap();
        assert_eq!((img.width, img.height), (Some(600), Some(40)));
        let t = img.thumbnail.unwrap();
        assert_eq!((t.width, t.height), (256, 17));
        assert_eq!(t.rgba.len(), 256 * 17 * 4);
        assert!(d.file_uris.is_empty());
    }

    #[test]
    fn ui_detail_of_files_lists_uris_and_counts_the_rest() {
        let many: String = (0..130)
            .map(|i| {
                format!(
                    "file:///C:/d/f{i}.txt
"
                )
            })
            .collect();
        let h = start_with_clips(&[
            files_clip(
                "# c
file:///C:/a%20b.txt
file:///C:/c.txt
",
            ),
            files_clip(&many),
        ]);
        let d = h.app().ui_clip_detail(&Caller::Ui, h.ids[0]).unwrap();
        assert_eq!(d.file_total, 2);
        assert_eq!(d.file_uris, ["file:///C:/a%20b.txt", "file:///C:/c.txt"]);
        let big = h.app().ui_clip_detail(&Caller::Ui, h.ids[1]).unwrap();
        assert_eq!(big.file_total, 130);
        assert_eq!(big.file_uris.len(), MAX_FILE_URIS);
    }

    #[test]
    fn ui_detail_of_a_masked_clip_reads_nothing() {
        let h = start_with_clips(&[image_clip(8, 8, true), new_clip("secret text", true)]);
        for id in &h.ids {
            let d = h.app().ui_clip_detail(&Caller::Ui, *id).unwrap();
            assert!(d.masked);
            assert!(d.image.is_none());
            assert!(d.file_uris.is_empty());
            // No full image either, and no thumbnail at the storage level.
            assert_eq!(
                h.app().ui_clip_image(&Caller::Ui, *id, 1024).unwrap_err(),
                ApiError::NotFound
            );
            assert_eq!(h.app().reader().read_thumbnail(id).unwrap(), None);
        }
    }

    #[test]
    fn ui_image_is_downscaled_and_only_for_images() {
        let h = start_with_clips(&[image_clip(2000, 10, false), new_clip("plain", false)]);
        let p = h.app().ui_clip_image(&Caller::Ui, h.ids[0], 1024).unwrap();
        assert_eq!((p.width, p.height), (1024, 5));
        assert_eq!(p.rgba.len(), 1024 * 5 * 4);
        assert_eq!(
            h.app()
                .ui_clip_image(&Caller::Ui, h.ids[1], 1024)
                .unwrap_err(),
            ApiError::NotFound
        );
        let t = h.app().ui_clip_detail(&Caller::Ui, h.ids[1]).unwrap();
        assert!(t.image.is_none() && t.file_uris.is_empty() && !t.masked);
        let missing = ClipId::new_v7(Timestamp(1));
        assert_eq!(
            h.app().ui_clip_detail(&Caller::Ui, missing).unwrap_err(),
            ApiError::NotFound
        );
    }

    #[test]
    fn ui_methods_are_for_the_ui_caller_only() {
        let h = start_with_clips(&[image_clip(8, 8, false), new_clip("some text", false)]);
        let a = h.app();
        for c in [Caller::Cli, mcp()] {
            assert!(matches!(
                a.ui_clip_detail(&c, h.ids[0]),
                Err(ApiError::PermissionDenied(_))
            ));
            assert!(matches!(
                a.ui_clip_image(&c, h.ids[0], 64),
                Err(ApiError::PermissionDenied(_))
            ));
            assert!(matches!(
                a.ui_clip_text(&c, h.ids[1]),
                Err(ApiError::PermissionDenied(_))
            ));
        }
    }

    #[test]
    fn ui_methods_answer_not_found_for_expired_clips_masked_and_pinned() {
        let mut expiring_image = image_clip(8, 8, false);
        expiring_image.expires_at = Some(Timestamp(T0 + 60_000));
        let mut expiring_text = new_clip("expiring text", false);
        expiring_text.expires_at = Some(Timestamp(T0 + 60_000));
        let masked_image = image_clip(9, 9, true);
        let (h, clock) = start_with_clips_clock(&[
            expiring_image,
            expiring_text,
            masked_image,
            new_clip("masked text", true),
        ]);
        let a = h.app();
        // Pinned does not outlive the privacy TTL (D-025); a masked clip cannot be pinned.
        a.pin_clip(&Caller::Ui, h.ids[0], true).unwrap();
        a.pin_clip(&Caller::Ui, h.ids[1], true).unwrap();
        assert!(a.pin_clip(&Caller::Ui, h.ids[2], true).is_err());
        // Before expiry the unmasked ones answer.
        assert!(a.ui_clip_detail(&Caller::Ui, h.ids[0]).is_ok());
        assert!(a.ui_clip_text(&Caller::Ui, h.ids[1]).is_ok());
        clock.0.store(T0 + 2 * 3_600_000, Ordering::SeqCst);
        for id in &h.ids {
            assert_eq!(
                a.ui_clip_detail(&Caller::Ui, *id).unwrap_err(),
                ApiError::NotFound
            );
            assert_eq!(
                a.ui_clip_image(&Caller::Ui, *id, 64).unwrap_err(),
                ApiError::NotFound
            );
            assert_eq!(
                a.ui_clip_text(&Caller::Ui, *id).unwrap_err(),
                ApiError::NotFound
            );
        }
    }

    #[test]
    fn ui_text_is_cut_at_32_kib_on_a_char_boundary_and_never_for_masked() {
        assert_eq!(UI_TEXT_MAX_BYTES, 32 * 1024);
        let big: String = "€".repeat(20_000);
        let h = start(&[(&big, false), ("short", false), ("secret", true)], None);
        let a = h.app();
        let t = a.ui_clip_text(&Caller::Ui, h.ids[0]).unwrap();
        assert!(t.truncated);
        assert!(t.text.len() <= UI_TEXT_MAX_BYTES && t.text.len().is_multiple_of(3));
        let s = a.ui_clip_text(&Caller::Ui, h.ids[1]).unwrap();
        assert_eq!((s.text.as_str(), s.truncated), ("short", false));
        assert_eq!(
            a.ui_clip_text(&Caller::Ui, h.ids[2]).unwrap_err(),
            ApiError::NotFound
        );
        // The shared CLI/MCP path keeps its own 100 KiB cap.
        let c = a
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
        assert!(c.text.len() > UI_TEXT_MAX_BYTES);
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

    // ---- Task 402: organisation API ----

    fn fav_id() -> CollectionId {
        CollectionId::parse(darkclip_core::FAVORITES_COLLECTION_ID).unwrap()
    }

    fn ids_of(p: &Page<ClipSummaryView>) -> Vec<ClipId> {
        p.items.iter().map(|i| i.id).collect()
    }

    fn user_collections(a: &App) -> Vec<CollectionView> {
        a.list_collections(&Caller::Ui)
            .unwrap()
            .into_iter()
            .filter(|c| !c.system)
            .collect()
    }

    /// Task 405: `export_collection` works for the UI (tests in `transfer_tests`); the CLI
    /// gets it in P2.
    #[test]
    fn export_stays_unavailable_outside_the_ui() {
        let h = start(&[("x", false)], None);
        let a = h.app();
        let col = CollectionId::new_v7(Timestamp(T0));
        let nope = ApiError::InvalidArgument("not available until P1");
        assert_eq!(a.export_collection(&Caller::Cli, col).unwrap_err(), nope);
        assert!(matches!(
            a.export_collection(&mcp(), col).unwrap_err(),
            ApiError::PermissionDenied(_)
        ));
    }

    #[test]
    fn collections_create_list_rename_delete() {
        let h = start(&[("a", false)], None);
        let a = h.app();
        let ui = &Caller::Ui;
        let all = a.list_collections(ui).unwrap();
        assert_eq!(all.len(), 1);
        assert!(all[0].system);
        assert_eq!(all[0].id, fav_id());
        let col = a.create_collection(ui, "  Work  ").unwrap();
        assert_eq!(col.name, "Work");
        assert!(!col.system);
        assert_eq!(a.list_collections(ui).unwrap().len(), 2);
        a.rename_collection(ui, col.id, "Home").unwrap();
        assert_eq!(user_collections(a)[0].name, "Home");
        a.add_clip_to_collection(ui, h.ids[0], col.id).unwrap();
        assert_eq!(user_collections(a)[0].clip_count, 1);
        a.delete_collection(ui, col.id).unwrap();
        assert!(user_collections(a).is_empty());
        // The clip is kept.
        assert!(a.get_clip(ui, h.ids[0], GetOptions::default()).is_ok());
        // Errors.
        for bad in ["", "   ", &"x".repeat(81)] {
            assert!(matches!(
                a.create_collection(ui, bad),
                Err(ApiError::InvalidArgument(_))
            ));
        }
        let other = a.create_collection(ui, "Other").unwrap();
        assert!(matches!(
            a.rename_collection(ui, other.id, " "),
            Err(ApiError::InvalidArgument(_))
        ));
        let ghost = CollectionId::new_v7(Timestamp(T0));
        assert_eq!(
            a.rename_collection(ui, ghost, "x").unwrap_err(),
            ApiError::NotFound
        );
        assert_eq!(
            a.delete_collection(ui, ghost).unwrap_err(),
            ApiError::NotFound
        );
        let fav = fav_id();
        assert!(matches!(
            a.rename_collection(ui, fav, "x"),
            Err(ApiError::InvalidArgument(_))
        ));
        assert!(matches!(
            a.delete_collection(ui, fav),
            Err(ApiError::InvalidArgument(_))
        ));
    }

    #[test]
    fn membership_add_remove_reorder_and_filter() {
        let h = start(
            &[("a", false), ("b", false), ("c", false), ("m", true)],
            None,
        );
        let a = h.app();
        let ui = &Caller::Ui;
        let col = a.create_collection(ui, "Set").unwrap();
        for i in [0, 1, 2] {
            a.add_clip_to_collection(ui, h.ids[i], col.id).unwrap();
        }
        a.add_clip_to_collection(ui, h.ids[0], col.id).unwrap(); // idempotent
        let mut q = query(None);
        q.collection = Some(col.id);
        let page = a.search_clips(ui, q.clone()).unwrap();
        assert_eq!(ids_of(&page), vec![h.ids[0], h.ids[1], h.ids[2]]);
        a.reorder_clip_in_collection(ui, col.id, h.ids[2], 0)
            .unwrap();
        let page = a.search_clips(ui, q.clone()).unwrap();
        assert_eq!(ids_of(&page), vec![h.ids[2], h.ids[0], h.ids[1]]);
        a.remove_clip_from_collection(ui, col.id, h.ids[0]).unwrap();
        let page = a.search_clips(ui, q.clone()).unwrap();
        assert_eq!(ids_of(&page), vec![h.ids[2], h.ids[1]]);
        // Unknown ids and masked clips.
        let ghost_col = CollectionId::new_v7(Timestamp(T0));
        let ghost_clip = ClipId::new_v7(Timestamp(T0));
        assert_eq!(
            a.add_clip_to_collection(ui, h.ids[0], ghost_col)
                .unwrap_err(),
            ApiError::NotFound
        );
        assert_eq!(
            a.add_clip_to_collection(ui, ghost_clip, col.id)
                .unwrap_err(),
            ApiError::NotFound
        );
        assert_eq!(
            a.add_clip_to_collection(ui, h.ids[3], col.id).unwrap_err(),
            ApiError::NotFound
        );
        assert_eq!(
            a.remove_clip_from_collection(ui, ghost_col, h.ids[1])
                .unwrap_err(),
            ApiError::NotFound
        );
        assert_eq!(
            a.reorder_clip_in_collection(ui, col.id, h.ids[0], 0)
                .unwrap_err(),
            ApiError::NotFound
        );
        assert_eq!(
            a.remove_clip_from_collection(ui, col.id, ghost_clip)
                .unwrap_err(),
            ApiError::NotFound
        );
        assert_eq!(
            a.reorder_clip_in_collection(ui, col.id, ghost_clip, 0)
                .unwrap_err(),
            ApiError::NotFound
        );
        // A tag filter that is not a valid name matches nothing.
        let mut bad = query(None);
        bad.tag = Some("no way!".into());
        assert!(a.search_clips(ui, bad).unwrap().items.is_empty());
    }

    #[test]
    fn favorites_flag_filter_and_retention() {
        let h = start(&[("old", false), ("mid", false), ("new", false)], None);
        let a = h.app();
        let ui = &Caller::Ui;
        a.set_favorite(ui, h.ids[0], true).unwrap();
        a.set_favorite(ui, h.ids[0], true).unwrap();
        let all = a.search_clips(ui, query(None)).unwrap();
        for item in &all.items {
            assert_eq!(item.favorite, item.id == h.ids[0]);
        }
        let mut q = query(None);
        q.collection = Some(fav_id());
        assert_eq!(
            ids_of(&a.search_clips(ui, q.clone()).unwrap()),
            vec![h.ids[0]]
        );
        let ghost = ClipId::new_v7(Timestamp(T0));
        assert_eq!(
            a.set_favorite(ui, ghost, true).unwrap_err(),
            ApiError::NotFound
        );
        // Retention keeps the favorite while everything else is evicted.
        let mut s = a.settings();
        s.retention.max_items = 1;
        a.update_settings(s).unwrap();
        let mut left = 3;
        for _ in 0..300 {
            left = a.search_clips(ui, query(None)).unwrap().items.len();
            if left == 1 {
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        assert_eq!(left, 1);
        assert!(a.get_clip(ui, h.ids[0], GetOptions::default()).is_ok());
        a.set_favorite(ui, h.ids[0], false).unwrap();
        assert!(!a.search_clips(ui, query(None)).unwrap().items[0].favorite);
    }

    #[test]
    fn notes_and_tags() {
        let h = start(&[("alpha", false), ("beta", false), ("secret", true)], None);
        let a = h.app();
        let ui = &Caller::Ui;
        a.update_clip_note(ui, h.ids[0], Some("remember this".into()))
            .unwrap();
        let v = a.get_clip(ui, h.ids[0], GetOptions::default()).unwrap();
        assert_eq!(v.clip.note.as_deref(), Some("remember this"));
        a.update_clip_note(ui, h.ids[0], None).unwrap();
        let v = a.get_clip(ui, h.ids[0], GetOptions::default()).unwrap();
        assert_eq!(v.clip.note, None);
        assert!(matches!(
            a.update_clip_note(ui, h.ids[0], Some("n".repeat(4001))),
            Err(ApiError::InvalidArgument(_))
        ));
        let tags = a
            .update_clip_tags(
                ui,
                h.ids[0],
                vec!["Work".into(), "ideas".into(), "work".into()],
            )
            .unwrap();
        let names: Vec<&str> = tags.iter().map(|t| t.name.as_str()).collect();
        assert_eq!(names, ["ideas", "work"]);
        a.update_clip_tags(ui, h.ids[1], vec!["work".into()])
            .unwrap();
        let listed = a.list_tags(ui).unwrap();
        assert_eq!(listed.len(), 2);
        let work = listed.iter().find(|t| t.name == "work").unwrap();
        assert_eq!(work.clip_count, 2);
        let mut q = query(None);
        q.tag = Some("ideas".into());
        let page = a.search_clips(ui, q).unwrap();
        assert_eq!(ids_of(&page), vec![h.ids[0]]);
        assert_eq!(page.items[0].tags, ["ideas", "work"]);
        // Replace, and an unused tag disappears.
        a.update_clip_tags(ui, h.ids[0], vec![]).unwrap();
        assert!(a.list_tags(ui).unwrap().iter().all(|t| t.name != "ideas"));
        // Invalid names, too many tags, unknown clip, masked clip.
        assert!(matches!(
            a.update_clip_tags(ui, h.ids[0], vec!["no good!".into()]),
            Err(ApiError::InvalidArgument(_))
        ));
        let many: Vec<String> = (0..21).map(|i| format!("t{i}")).collect();
        assert!(matches!(
            a.update_clip_tags(ui, h.ids[0], many),
            Err(ApiError::InvalidArgument(_))
        ));
        let ghost = ClipId::new_v7(Timestamp(T0));
        assert_eq!(
            a.update_clip_tags(ui, ghost, vec!["x".into()]).unwrap_err(),
            ApiError::NotFound
        );
        assert_eq!(
            a.update_clip_note(ui, ghost, None).unwrap_err(),
            ApiError::NotFound
        );
        assert_eq!(
            a.update_clip_tags(ui, h.ids[2], vec!["x".into()])
                .unwrap_err(),
            ApiError::NotFound
        );
        assert_eq!(
            a.update_clip_note(ui, h.ids[2], Some("x".into()))
                .unwrap_err(),
            ApiError::NotFound
        );
        assert_eq!(
            a.set_favorite(ui, h.ids[2], true).unwrap_err(),
            ApiError::NotFound
        );
    }

    #[test]
    fn expired_clips_are_not_found_for_organisation_calls() {
        let mut exp = new_clip("expiring", false);
        exp.expires_at = Some(Timestamp(T0 + 60_000));
        let (h, clock) = start_with_clips_clock(&[exp]);
        let a = h.app();
        let col = a.create_collection(&Caller::Ui, "c").unwrap();
        clock.0.store(T0 + 2 * 3_600_000, Ordering::SeqCst);
        let id = h.ids[0];
        let ui = &Caller::Ui;
        assert_eq!(
            a.add_clip_to_collection(ui, id, col.id).unwrap_err(),
            ApiError::NotFound
        );
        assert_eq!(
            a.set_favorite(ui, id, true).unwrap_err(),
            ApiError::NotFound
        );
        assert_eq!(
            a.update_clip_note(ui, id, Some("x".into())).unwrap_err(),
            ApiError::NotFound
        );
        assert_eq!(
            a.update_clip_tags(ui, id, vec!["x".into()]).unwrap_err(),
            ApiError::NotFound
        );
    }

    #[test]
    fn organisation_follows_the_permission_guard() {
        let h = start(&[("a", false), ("m", true)], None);
        let a = h.app();
        let mcp = mcp();
        let col = a.create_collection(&Caller::Ui, "c").unwrap();
        let denied = |e: ApiError| assert!(matches!(e, ApiError::PermissionDenied(_)));
        denied(a.list_collections(&mcp).unwrap_err());
        denied(a.create_collection(&mcp, "x").unwrap_err());
        denied(a.rename_collection(&mcp, col.id, "x").unwrap_err());
        denied(a.delete_collection(&mcp, col.id).unwrap_err());
        denied(
            a.add_clip_to_collection(&mcp, h.ids[0], col.id)
                .unwrap_err(),
        );
        denied(
            a.remove_clip_from_collection(&mcp, col.id, h.ids[0])
                .unwrap_err(),
        );
        denied(
            a.reorder_clip_in_collection(&mcp, col.id, h.ids[0], 0)
                .unwrap_err(),
        );
        denied(a.set_favorite(&mcp, h.ids[0], true).unwrap_err());
        denied(a.update_clip_note(&mcp, h.ids[0], None).unwrap_err());
        denied(a.update_clip_tags(&mcp, h.ids[0], vec![]).unwrap_err());
        denied(a.list_tags(&mcp).unwrap_err());
        // Nothing changed.
        assert_eq!(user_collections(a).len(), 1);
        assert_eq!(user_collections(a)[0].clip_count, 0);
        // The CLI keeps today's guard behaviour (allowed), but never sees masked clips.
        a.add_clip_to_collection(&Caller::Cli, h.ids[0], col.id)
            .unwrap();
        assert_eq!(
            a.add_clip_to_collection(&Caller::Cli, h.ids[1], col.id)
                .unwrap_err(),
            ApiError::NotFound
        );
        assert_eq!(
            a.update_clip_tags(&Caller::Cli, h.ids[1], vec![])
                .unwrap_err(),
            ApiError::NotFound
        );
        assert_eq!(
            a.remove_clip_from_collection(&Caller::Cli, col.id, h.ids[1])
                .unwrap_err(),
            ApiError::NotFound
        );
        assert_eq!(
            a.reorder_clip_in_collection(&Caller::Cli, col.id, h.ids[1], 0)
                .unwrap_err(),
            ApiError::NotFound
        );
    }

    #[test]
    fn mutations_publish_an_organization_event() {
        let h = start(&[("a", false)], None);
        let a = h.app();
        while h.events.try_recv().is_ok() {}
        a.create_collection(&Caller::Ui, "ev").unwrap();
        let got = h.events.recv_timeout(Duration::from_secs(5)).unwrap();
        assert_eq!(got, AppEvent::OrganizationChanged);
        // A failed mutation publishes nothing.
        while h.events.try_recv().is_ok() {}
        assert!(a.create_collection(&Caller::Ui, " ").is_err());
        assert!(h.events.recv_timeout(Duration::from_millis(200)).is_err());
    }

    #[test]
    fn summary_view_serializes_tags_and_favorite_only_when_set() {
        let h = start(&[("alpha", false)], None);
        let a = h.app();
        let page = a.search_clips(&Caller::Ui, query(None)).unwrap();
        let json = serde_json::to_value(&page.items[0]).unwrap();
        assert!(json.get("tags").is_none());
        assert!(json.get("favorite").is_none());
        a.update_clip_tags(&Caller::Ui, h.ids[0], vec!["work".into()])
            .unwrap();
        a.set_favorite(&Caller::Ui, h.ids[0], true).unwrap();
        let page = a.search_clips(&Caller::Ui, query(None)).unwrap();
        let json = serde_json::to_value(&page.items[0]).unwrap();
        assert_eq!(json["tags"][0], "work");
        assert_eq!(json["favorite"], true);
    }

    mod transfer_tests;
    mod transform_ui_tests;
}
