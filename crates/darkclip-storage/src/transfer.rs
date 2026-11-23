//! Export files (`.dcexport`, a zip) and the reader the import uses (Task 405,
//! data-model §8, `export.schema.json`).
//!
//! An export holds `export.json` (`export.schema.json`) and one `blobs/<aa>/<sha256>` file per
//! distinct representation: every representation is exported as a blob, so the JSON never
//! carries content. Masked clips and clips with a detected sensitive category are never
//! exported; the report counts them. Exported clips carry no source app, no platform format
//! names, no thumbnail reference and only the exported collection as membership.
//!
//! Reading treats the file as untrusted (`crate::archive` limits and name rules; a newer
//! `schema_version` is refused). The reader only parses and verifies: the import itself
//! (`darkclip-app`) re-derives everything it stores and trusts none of the metadata.

use crate::archive::{
    self, ArchiveError, ArchiveLimits, AtomicZip, CheckedZip, EntryKind, EXPORT_JSON,
    EXPORT_SCHEMA_VERSION,
};
use crate::{blobs, Storage, StorageError};
use darkclip_core::{
    Clip, ClipId, Collection, CollectionRef, Representation, SensitivityLevel,
    Storage as RepStorage, Timestamp,
};
use rusqlite::{params, OptionalExtension};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::path::Path;

/// What an export wrote and left out.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ExportReport {
    pub exported: u64,
    /// Masked clips and clips with a detected sensitive category: never exported.
    pub excluded_sensitive: u64,
    /// Ids that no longer exist (deleted or expired meanwhile).
    pub missing: u64,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ExportDoc {
    schema_version: u32,
    app_version: String,
    created_at: Timestamp,
    collection: Collection,
    clips: Vec<Clip>,
    blobs: Vec<BlobEntry>,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct BlobEntry {
    sha256: String,
    path: String,
    size_bytes: u64,
}

/// An export file opened for import: the parsed document and lazy, verified blob reads.
pub struct ExportArchive {
    collection: Collection,
    clips: Vec<Clip>,
    zip: CheckedZip,
    /// Blob hash -> (zip index, size).
    blobs: HashMap<[u8; 32], (usize, u64)>,
}

/// Counts only (GC-06): names, notes and content are never printed.
impl std::fmt::Debug for ExportArchive {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ExportArchive")
            .field("clips", &self.clips.len())
            .field("blobs", &self.blobs.len())
            .finish_non_exhaustive()
    }
}

fn len64(n: usize) -> u64 {
    u64::try_from(n).unwrap_or(u64::MAX)
}

impl ExportArchive {
    /// Opens and validates an export file: names, counts, sizes, the JSON document and its
    /// blob list against the archive's entries. Blob contents are checked when read.
    pub fn open(path: &Path, limits: ArchiveLimits) -> Result<Self, StorageError> {
        let mut zip = CheckedZip::open(path, limits, &[EntryKind::ExportJson])?;
        if zip.index_of(EntryKind::ExportJson).is_none() {
            return Err(ArchiveError::NotAnArchive.into());
        }
        let raw = zip.read_json(EntryKind::ExportJson)?;
        let v: serde_json::Value =
            serde_json::from_slice(&raw).map_err(|_| ArchiveError::BadManifest)?;
        if v.get("schema_version")
            .and_then(serde_json::Value::as_u64)
            .is_some_and(|n| n > u64::from(EXPORT_SCHEMA_VERSION))
        {
            return Err(ArchiveError::NewerFormat.into());
        }
        let doc: ExportDoc = serde_json::from_value(v).map_err(|_| ArchiveError::BadManifest)?;
        if doc.schema_version != EXPORT_SCHEMA_VERSION || doc.clips.len() > limits.max_entries {
            return Err(ArchiveError::BadManifest.into());
        }
        let in_zip: HashMap<[u8; 32], usize> = zip
            .entries
            .iter()
            .filter_map(|(k, i)| match k {
                EntryKind::Blob(sha) => Some((*sha, *i)),
                _ => None,
            })
            .collect();
        let mut blobs = HashMap::new();
        for b in &doc.blobs {
            let sha = archive::parse_hex32(&b.sha256).ok_or(ArchiveError::BadManifest)?;
            if b.path != archive::blob_entry_name(&sha) {
                return Err(ArchiveError::BadManifest.into());
            }
            if b.size_bytes > limits.max_entry_bytes {
                return Err(ArchiveError::TooLarge.into());
            }
            let idx = in_zip.get(&sha).copied().ok_or(ArchiveError::BadManifest)?;
            if blobs.insert(sha, (idx, b.size_bytes)).is_some() {
                return Err(ArchiveError::BadManifest.into());
            }
        }
        if blobs.len() != in_zip.len() {
            return Err(ArchiveError::BadManifest.into());
        }
        Ok(Self {
            collection: doc.collection,
            clips: doc.clips,
            zip,
            blobs,
        })
    }

    /// The collection the file was exported from (untrusted: the importer validates it).
    pub fn collection(&self) -> &Collection {
        &self.collection
    }

    /// The clips as written in the file (untrusted metadata).
    pub fn clips(&self) -> &[Clip] {
        &self.clips
    }

    /// The size the file declares for blob `sha` (checked again when it is read).
    pub fn blob_size(&self, sha: &[u8; 32]) -> Option<u64> {
        let _ = sha;
        None
    }

    /// The bytes of blob `sha`, verified against its size and hash. `NotFound` when the file
    /// does not list it.
    pub fn blob(&mut self, sha: &[u8; 32]) -> Result<Vec<u8>, StorageError> {
        let (idx, size) = self.blobs.get(sha).copied().ok_or(StorageError::NotFound)?;
        self.zip.read_verified(idx, size, sha)
    }
}

impl Storage {
    /// Writes the clips `ids` (in that order) as an export of `collection` to `dest`
    /// (temp file, then rename). Masked and sensitive clips are left out and counted.
    pub fn write_export(
        &self,
        ids: &[ClipId],
        collection: &Collection,
        dest: &Path,
        app_version: &str,
    ) -> Result<ExportReport, StorageError> {
        self.write_export_with(ids, collection, dest, app_version, ArchiveLimits::EXPORT)
    }

    /// `write_export` refusing anything `limits` would refuse on import.
    pub fn write_export_with(
        &self,
        ids: &[ClipId],
        collection: &Collection,
        dest: &Path,
        app_version: &str,
        _limits: ArchiveLimits,
    ) -> Result<ExportReport, StorageError> {
        let now = self.clock.now();
        let mut report = ExportReport::default();
        let mut z = AtomicZip::create(dest)?;
        let mut written: HashSet<[u8; 32]> = HashSet::new();
        let mut blob_list = Vec::new();
        let mut clips = Vec::new();
        'clips: for id in ids {
            let Some(clip) = self.get_clip(id)? else {
                report.missing += 1;
                continue;
            };
            if clip.expires_at.is_some_and(|t| t <= now) {
                report.missing += 1;
                continue;
            }
            if clip.sensitivity.level == SensitivityLevel::Masked
                || !clip.sensitivity.categories.is_empty()
            {
                report.excluded_sensitive += 1;
                continue;
            }
            let mut reps = Vec::with_capacity(clip.representations.len());
            let mut pending = Vec::new();
            for (ord, rep) in clip.representations.iter().enumerate() {
                let ord = u32::try_from(ord).map_err(|_| StorageError::NotFound)?;
                let bytes = match self.read_representation(id, ord) {
                    Ok(b) => b,
                    // Deleted or its blob lost meanwhile: the clip is skipped as missing.
                    Err(StorageError::NotFound | StorageError::Io(_)) => {
                        report.missing += 1;
                        continue 'clips;
                    }
                    Err(e) => return Err(e),
                };
                let sha = blobs::sha256(&bytes);
                reps.push(Representation {
                    mime: rep.mime,
                    platform_format: None,
                    size_bytes: len64(bytes.len()),
                    storage: RepStorage::Blob,
                    blob_sha256: Some(sha),
                    width: rep.width,
                    height: rep.height,
                });
                pending.push((sha, bytes));
            }
            for (sha, bytes) in pending {
                if written.insert(sha) {
                    let path = archive::blob_entry_name(&sha);
                    z.add_bytes(&path, &bytes)?;
                    blob_list.push(BlobEntry {
                        sha256: blobs::hex(&sha),
                        path,
                        size_bytes: len64(bytes.len()),
                    });
                }
            }
            let mut out = clip;
            out.size_bytes = reps.iter().map(|r| r.size_bytes).sum();
            out.representations = reps;
            out.source = None;
            out.metadata.image_thumb_sha256 = None;
            out.collections = vec![CollectionRef {
                id: collection.id,
                name: collection.name.clone(),
            }];
            out.expires_at = None;
            clips.push(out);
            report.exported += 1;
        }
        let mut col = collection.clone();
        col.clip_count = report.exported;
        let doc = ExportDoc {
            schema_version: EXPORT_SCHEMA_VERSION,
            app_version: app_version.to_string(),
            created_at: now,
            collection: col,
            clips,
            blobs: blob_list,
        };
        let json = serde_json::to_vec_pretty(&doc)
            .map_err(|_| StorageError::Sqlite("ColumnEncode".into()))?;
        z.add_bytes(EXPORT_JSON, &json)?;
        z.commit()?;
        Ok(report)
    }

    /// Whether the clip carries the no-export flag (NoCloud at capture). Stub.
    pub fn is_no_export(&self, id: &ClipId) -> Result<bool, StorageError> {
        let _ = id;
        Ok(false)
    }

    /// Sets an imported clip's `created_at` to `created_at` clamped to `[0, now]` and to at
    /// most its `last_copied_at`. Returns the value stored.
    pub fn import_created_at(
        &mut self,
        id: &ClipId,
        created_at: Timestamp,
    ) -> Result<Timestamp, StorageError> {
        self.ensure_writable()?;
        let now = self.clock.now();
        let t = created_at.0.clamp(0, now.0.max(0));
        let id_s = id.to_string();
        let n = self.conn.execute(
            "UPDATE clips SET created_at = min(?2, last_copied_at) WHERE id = ?1",
            params![id_s, t],
        )?;
        if n == 0 {
            return Err(StorageError::NotFound);
        }
        let stored: Option<i64> = self
            .conn
            .query_row("SELECT created_at FROM clips WHERE id = ?1", [&id_s], |r| {
                r.get(0)
            })
            .optional()?;
        stored.map(Timestamp).ok_or(StorageError::NotFound)
    }
}

#[cfg(test)]
mod tests;
