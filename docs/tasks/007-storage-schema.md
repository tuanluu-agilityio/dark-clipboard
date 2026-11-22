# Task 007: Storage schema v1, migrations, insert/dedupe/get, blobs

**Agent:** `specialist` · **Model:** opus · **Branch:** `task/007-storage-schema` · **Review:** `reviewer` (opus)
**Global Constraints:** GC-01, GC-06, GC-10, GC-12.

## Read first
`docs/architecture/data-model.md` §4–7 (the SQL is normative), `docs/adr/0003-storage.md`, Tasks 002 and 004 Interfaces.

## Files
Modify `crates/darkclip-core/src/model.rs` (add `Clip::validate` only). Create `crates/darkclip-storage/src/{lib.rs, db.rs, migrate.rs, blobs.rs, clips.rs, error.rs}`, `crates/darkclip-storage/migrations/0001_init.sql`. Add `rusqlite` (`bundled`) and `png` (thumbnails).

## Interfaces
```rust
pub struct Storage { /* rusqlite::Connection + data dir */ }
pub enum OpenMode { ReadWrite, ReadOnly }
impl Storage {
    pub fn open(data_dir: &Path, mode: OpenMode, clock: Arc<dyn Clock>) -> Result<Self, StorageError>;
    pub fn schema_version(&self) -> Result<u32, StorageError>;
    /// Inserts or, on content_hash conflict, bumps last_copied_at/copy_count/source and replaces representations.
    pub fn upsert_clip(&mut self, new: NewClip) -> Result<UpsertOutcome, StorageError>;
    pub fn get_clip(&self, id: &ClipId) -> Result<Option<Clip>, StorageError>;
    pub fn read_representation(&self, id: &ClipId, ord: u32) -> Result<Vec<u8>, StorageError>;  // inline or blob
    pub fn mark_used(&mut self, id: &ClipId) -> Result<(), StorageError>;
    pub fn set_pinned(&mut self, id: &ClipId, pinned: bool) -> Result<(), StorageError>;
}
pub struct NewClip { pub classified: Classified, pub hash: [u8; 32], pub reps: Vec<RawRepresentation>,
                     pub source: Option<ClipboardSource>, pub sensitivity: Sensitivity, pub expires_at: Option<Timestamp>,
                     pub thumbnail_png: Option<Vec<u8>> }
pub enum UpsertOutcome { Inserted(ClipId), Bumped(ClipId) }
pub enum StorageError { Sqlite(String /* code only, no SQL params */), Io(std::io::ErrorKind), SchemaTooNew { found: u32, supported: u32 }, NotFound, ReadOnly }
```
Validation (added after Task 002 review): add `Clip::validate(&self) -> Result<(), CoreError>` in `darkclip-core/src/model.rs` (this file is in scope for this task) enforcing the schema constraints that serde does not: preview ≤ 512 chars, note ≤ 4000, ≥ 1 representation, `copy_count` ≥ 1, blob storage ⇒ `blob_sha256`, masked ⇒ `expires_at`, ref names 1–80 chars, non-empty `app_id`. `upsert_clip` and any import path call it. Test `clip_validate_rejects_each_violation` (one case per rule).

Rules: reps > 64 KiB or any image → blob file `blobs/<aa>/<hex>` written to `<file>.tmp` then renamed, before the DB transaction commits. Directory permissions 0700 and files 0600 on Unix. `ReadOnly` open of a newer `user_version` → `SchemaTooNew`; `ReadWrite` of a newer one → error, never a downgrade. Pre-migration backup per data-model §7. `StorageError` `Display` never contains clip data.

## Tests
- `open_creates_schema_v1` (`user_version` = 1, WAL on, `secure_delete` on).
- `upsert_same_hash_bumps` (copy_count 2, one row, `Bumped`).
- `large_text_goes_to_blob` (70 KiB) and `small_text_inline`.
- `identical_image_two_clips_one_blob_file`: two clips with different text representations that carry the same PNG representation → exactly one file under `blobs/`, both rows reference it.
- `read_only_cannot_write` → `ReadOnly`.
- `newer_schema_rejected` (set `user_version = 99` manually).
- `migration_backup_created_and_removed`.
- `crash_between_blob_and_commit_leaves_orphan_only` (simulate failure after blob write: next `open` sweeps orphan; no dangling rows).
- `error_display_has_no_content` (force a constraint error containing user text in params; assert `Display` lacks it).

## Not doing
FTS queries (008), retention/deletion (009).
