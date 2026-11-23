//! Backup, restore, export and import on the actor thread (Task 405). The actor owns the
//! only writable `Storage` (D-028), so the restore swap and every imported clip go through it,
//! serialized with capture (no clipboard change is processed while one runs).
//!
//! Import trusts nothing in the file but the bytes it verified: each clip's representations
//! go through capture's own `prepare` (classify, kind check, sensitive scan and handling per
//! settings, decode and hash), so kind, preview, metadata, hash and sensitivity are
//! re-derived; the source is dropped; sizes are checked against the History limits; timestamps
//! are clamped to now; pin state and counters are not taken. Nothing in a file is executed,
//! opened or fetched. Counts only are reported, never content (GC-06). No logging here.

use super::actor::{size_caps, Actor};
use super::wipe::{wipe_bytes, wipe_reps};
use crate::events::{map_storage, ApiError};
use darkclip_core::policy::{SizeCaps, SkipReason};
use darkclip_core::{
    t, CaptureMode, Clip, ClipId, ClipType, Collection, CollectionId, CollectionKind,
    CollectionSort, Mime, Msg, RawRepresentation, SensitivityLevel, SystemCollection, Tag,
    Timestamp, TAGS_PER_CLIP_MAX,
};
use darkclip_storage::{
    ArchiveLimits, BackupReport, ExportArchive, ExportReport, RestoreReport, UpsertOutcome,
};
use std::path::PathBuf;

/// The version written into backups and exports.
pub const APP_VERSION: &str = env!("CARGO_PKG_VERSION");

/// A backup, restore, export or import. Paths are the user's choice; never printed.
pub enum TransferOp {
    Backup(PathBuf),
    Restore(PathBuf),
    Export {
        ids: Vec<ClipId>,
        collection: Collection,
        dest: PathBuf,
    },
    Import(PathBuf),
}

/// Prints the operation only, never a path or a collection name (GC-06).
impl std::fmt::Debug for TransferOp {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Backup(_) => "Backup",
            Self::Restore(_) => "Restore",
            Self::Export { .. } => "Export",
            Self::Import(_) => "Import",
        })
    }
}

/// What a `TransferOp` produced.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransferReply {
    Backup(BackupReport),
    Restore(RestoreReport),
    Export(ExportReport),
    Import(ImportReport),
}

/// What an import did, as counts per outcome (never content).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ImportReport {
    /// New clips stored.
    pub imported: u64,
    /// Clips already in history (same content): bumped, no second row.
    pub merged: u64,
    /// Secret-shaped text dropped under the "drop" setting.
    pub skipped_sensitive: u64,
    /// Kinds switched off in Privacy settings, and file lists (paths from another machine
    /// are never imported).
    pub skipped_kind: u64,
    /// Over the size limits of History settings.
    pub skipped_too_large: u64,
    /// Missing or damaged data in the file.
    pub skipped_invalid: u64,
}

impl ImportReport {
    /// Every clip that was not stored.
    pub fn skipped(&self) -> u64 {
        self.skipped_sensitive
            .saturating_add(self.skipped_kind)
            .saturating_add(self.skipped_too_large)
            .saturating_add(self.skipped_invalid)
    }
}

/// Why one imported clip was not stored.
enum Skip {
    Sensitive,
    Kind,
    TooLarge,
    Invalid,
}

/// One clip that was stored.
struct Stored {
    id: ClipId,
    merged: bool,
    masked: bool,
}

/// The `platform_format` stored for imported data: the MIME name, never the file's.
fn format_name(m: Mime) -> &'static str {
    match m {
        Mime::TextPlain => "text/plain",
        Mime::TextHtml => "text/html",
        Mime::TextRtf => "text/rtf",
        Mime::ImagePng => "image/png",
        Mime::UriList => "text/uri-list",
    }
}

impl Actor {
    pub(super) fn transfer(&mut self, op: TransferOp) -> Result<TransferReply, ApiError> {
        match op {
            TransferOp::Backup(dest) => self
                .storage
                .write_backup(&dest, APP_VERSION)
                .map(TransferReply::Backup)
                .map_err(|e| map_storage(&e)),
            TransferOp::Restore(path) => {
                let r = self
                    .storage
                    .restore_backup(&path)
                    .map_err(|e| map_storage(&e))?;
                // A different history: its index has nothing pending; expiries and limits
                // apply to it at once (a clipboard change waiting in the debounce is stored
                // into it as usual).
                self.fts_merge_due = None;
                self.run_retention();
                Ok(TransferReply::Restore(r))
            }
            TransferOp::Export {
                ids,
                collection,
                dest,
            } => self
                .storage
                .write_export(&ids, &collection, &dest, APP_VERSION)
                .map(TransferReply::Export)
                .map_err(|e| map_storage(&e)),
            TransferOp::Import(path) => self.import(path).map(TransferReply::Import),
        }
    }

    fn import(&mut self, path: PathBuf) -> Result<ImportReport, ApiError> {
        // Private mode stores nothing at all (like "save as new clip", D-061).
        if self.policy.mode == CaptureMode::Private {
            return Err(ApiError::InvalidArgument(crate::events::IMPORT_PRIVATE));
        }
        let mut file =
            ExportArchive::open(&path, ArchiveLimits::EXPORT).map_err(|e| map_storage(&e))?;
        let meta = file.collection().clone();
        let clips: Vec<Clip> = file.clips().to_vec();
        let caps = size_caps(&self.retention);
        let mut report = ImportReport::default();
        let mut target: Option<CollectionId> = None;
        for clip in &clips {
            let now = self.clock.now();
            match self.import_one(&mut file, clip, caps, now)? {
                Err(Skip::Sensitive) => report.skipped_sensitive += 1,
                Err(Skip::Kind) => report.skipped_kind += 1,
                Err(Skip::TooLarge) => report.skipped_too_large += 1,
                Err(Skip::Invalid) => report.skipped_invalid += 1,
                Ok(stored) => {
                    if stored.merged {
                        report.merged += 1;
                    } else {
                        report.imported += 1;
                        // Never later than now, never later than its last copy.
                        let _ = self.storage.import_created_at(&stored.id, clip.created_at);
                    }
                    // A masked clip is never organised (D-060).
                    if !stored.masked {
                        let col = match target {
                            Some(c) => c,
                            None => {
                                let c = self.import_collection(&meta)?;
                                target = Some(c);
                                c
                            }
                        };
                        self.organize_imported(&stored.id, clip, &col);
                    }
                }
            }
        }
        Ok(report)
    }

    /// Stores one clip from the file. The outer `Err` aborts the import (storage failure);
    /// the inner one counts a skipped clip.
    fn import_one(
        &mut self,
        file: &mut ExportArchive,
        clip: &Clip,
        caps: SizeCaps,
        now: Timestamp,
    ) -> Result<Result<Stored, Skip>, ApiError> {
        if clip.representations.is_empty() {
            return Ok(Err(Skip::Invalid));
        }
        let mut reps: Vec<RawRepresentation> = Vec::with_capacity(clip.representations.len());
        for rep in &clip.representations {
            let skip = (|| {
                let sha = rep.blob_sha256.ok_or(Skip::Invalid)?;
                let mut bytes = file.blob(&sha).map_err(|_| Skip::Invalid)?;
                let cap = if rep.mime == Mime::ImagePng {
                    caps.image_max_bytes
                } else {
                    caps.text_max_bytes
                };
                if bytes.len() as u64 > cap {
                    wipe_bytes(&mut bytes);
                    return Err(Skip::TooLarge);
                }
                if rep.mime == Mime::ImagePng {
                    // Only pictures that decode within the caps are accepted.
                    match crate::image::decode_png(
                        &bytes,
                        crate::image::MAX_PIXELS,
                        crate::image::MAX_RGBA_BYTES,
                    ) {
                        Ok(mut img) => wipe_bytes(&mut img.rgba),
                        Err(_) => {
                            wipe_bytes(&mut bytes);
                            return Err(Skip::Invalid);
                        }
                    }
                }
                Ok(RawRepresentation {
                    mime: rep.mime,
                    platform_format: format_name(rep.mime).to_string(),
                    bytes,
                    width: None,
                    height: None,
                    truncated: false,
                })
            })();
            match skip {
                Ok(r) => reps.push(r),
                Err(s) => {
                    wipe_reps(&mut reps);
                    return Ok(Err(s));
                }
            }
        }
        // File lists name paths on another machine: never imported.
        if darkclip_core::classify(&reps).kind == ClipType::Files {
            wipe_reps(&mut reps);
            return Ok(Err(Skip::Kind));
        }
        let mut new = match self.prepare(reps, None, now) {
            Ok(n) => n,
            Err(SkipReason::Sensitive) => return Ok(Err(Skip::Sensitive)),
            Err(SkipReason::KindDisabled) => return Ok(Err(Skip::Kind)),
            Err(_) => return Ok(Err(Skip::Invalid)),
        };
        let masked = new.sensitivity.level == SensitivityLevel::Masked;
        // A duplicate keeps its own source (the file's is never used).
        let existing = self
            .storage
            .clip_id_by_hash(&new.hash)
            .map_err(|e| map_storage(&e))?;
        if let Some(id) = existing {
            new.source = self
                .storage
                .get_clip(&id)
                .map_err(|e| map_storage(&e))?
                .and_then(|c| c.source);
        }
        match self.insert_new(new) {
            Some(UpsertOutcome::Inserted(id)) => Ok(Ok(Stored {
                id,
                merged: false,
                masked,
            })),
            Some(UpsertOutcome::Bumped(id)) => Ok(Ok(Stored {
                id,
                merged: true,
                masked,
            })),
            None => Err(ApiError::Storage),
        }
    }

    /// The collection imported clips join: Favorites for an export of Favorites; otherwise a
    /// user collection with the file's name (an existing one with the same name, ignoring
    /// case, is reused; D-063), created if needed. An invalid name, or one that matches a
    /// system collection, becomes "Imported clips".
    fn import_collection(&mut self, meta: &Collection) -> Result<CollectionId, ApiError> {
        let st = &mut self.storage;
        if meta.kind == CollectionKind::System
            && meta.system_key == Some(SystemCollection::Favorites)
        {
            return st.favorites_id().map_err(|e| map_storage(&e));
        }
        let all = st.list_collections().map_err(|e| map_storage(&e))?;
        let fallback = t(self.cfg.lang, Msg::ImportCollectionName).to_string();
        let name = Collection::validate_name(&meta.name)
            .ok()
            .filter(|n| {
                !all.iter().any(|c| {
                    c.kind == CollectionKind::System && c.name.to_lowercase() == n.to_lowercase()
                })
            })
            .unwrap_or(fallback);
        if let Some(c) = all.iter().find(|c| {
            c.kind == CollectionKind::User && c.name.to_lowercase() == name.to_lowercase()
        }) {
            return Ok(c.id);
        }
        let color = meta
            .color
            .as_deref()
            .filter(|c| Collection::validate_color(c).is_ok());
        st.create_collection(&name, CollectionSort::Manual, color)
            .map(|c| c.id)
            .map_err(|e| map_storage(&e))
    }

    /// Membership, the note (only when the clip has none) and the tags (added to its own,
    /// invalid names dropped, at most 20). Best-effort: a refused value is left out.
    fn organize_imported(&mut self, id: &ClipId, clip: &Clip, col: &CollectionId) {
        let st = &mut self.storage;
        let _ = st.add_to_collection(col, id);
        let Ok(Some(current)) = st.get_clip(id) else {
            return;
        };
        if current.note.is_none() {
            if let Some(note) = clip.note.as_deref().filter(|n| !n.trim().is_empty()) {
                let _ = st.set_note(id, Some(note));
            }
        }
        let mut names: Vec<String> = current.tags.iter().map(|t| t.name.clone()).collect();
        let before = names.len();
        for tag in &clip.tags {
            if let Ok(n) = Tag::validate_name(&tag.name) {
                if !names.contains(&n) && names.len() < TAGS_PER_CLIP_MAX {
                    names.push(n);
                }
            }
        }
        if names.len() > before {
            let _ = st.set_clip_tags(id, &names);
        }
    }
}
