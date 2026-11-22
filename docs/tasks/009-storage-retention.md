# Task 009: Retention, deletion, clear history, stats

**Agent:** `builder` · **Model:** sonnet · **Branch:** `task/009-storage-retention` · **Review:** `reviewer` (opus; deletion correctness)
**Global Constraints:** GC-06, GC-10.

## Read first
`docs/architecture/data-model.md` §3 RetentionPolicy, §5 (blob GC), §6, Task 007 Interfaces.

## Files
Create `crates/darkclip-storage/src/{retention.rs, stats.rs}`; modify `clips.rs`, `blobs.rs`.

## Interfaces
```rust
impl Storage {
    pub fn delete_clip(&mut self, id: &ClipId) -> Result<(), StorageError>;
    pub fn clear_history(&mut self, keep_pinned: bool) -> Result<u64, StorageError>;   // returns deleted count
    pub fn apply_retention(&mut self, p: &RetentionPolicy) -> Result<RetentionReport, StorageError>;
    pub fn stats(&self) -> Result<StorageStats, StorageError>;
    pub fn sweep_orphan_blobs(&mut self) -> Result<u64, StorageError>;
}
pub struct RetentionReport { pub expired: u64, pub evicted_count: u64, pub evicted_age: u64, pub evicted_size: u64, pub bytes_freed: u64 }
pub struct StorageStats { pub clips: u64, pub pinned: u64, pub by_kind: Vec<(ClipType, u64)>, pub db_bytes: u64, pub blob_bytes: u64 }
```
Order: expired (`expires_at ≤ now`) → age → count → size. Pinned always kept; favorites kept when `keep_favorites` (the column arrives in P1; treat as none now). Blob files are unlinked after commit, only when unreferenced. After deleting > 100 rows: `PRAGMA wal_checkpoint(TRUNCATE)`.

## Tests
- `delete_removes_row_fts_and_blob`.
- `delete_keeps_shared_blob`. Review Focus 5.
- `retention_count_keeps_pinned`: `max_items` counts all clips, pinned included; pinned are never evicted. 1,005 clips of which 10 pinned, max 1,000 → 1,000 remain, all 10 pinned among them, the 5 oldest unpinned evicted.
- `retention_count_pinned_exceed_limit`: max 5, 10 pinned + 3 unpinned → the 10 pinned remain, 0 unpinned.
- `retention_age`, `retention_size` (blob bytes count toward size), `expired_masked_removed_first`.
- `clear_history_keep_pinned_true_and_false`.
- `stats_counts`.
- `sweep_orphans_after_simulated_crash`.

## Not doing
Scheduling (the app runs retention, Task 010/011).

## Interface additions from Task 007 (D-024)
- `darkclip_core::CoreError::Invalid { field: &'static str }`; `Clip::validate` returns it (fields `representations`, `copy_count`, `blob_sha256`, `expires_at`, `tags`, `collections`, `source.app_id`), and length violations use `CoreError::TooLong { field, max }`.
- `darkclip_storage::StorageError::Invalid(CoreError)` from `upsert_clip`.
- The `clip_rowid_ad` trigger deletes the FTS row whenever a clip is deleted (cascade), so deletion code only deletes from `clips`.
- Crate-internal `blobs::sweep(&mut Connection, &Path) -> blobs::Cleanup { removed, failed }`: best-effort, under `BEGIN IMMEDIATE`, never fails `open`. `read_representation` can return a transient `Io(NotFound)` if a writer unlinks the blob concurrently.
