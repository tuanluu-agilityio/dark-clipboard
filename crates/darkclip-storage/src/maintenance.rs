//! Data management (Task 411): cleanup tools, orphan-blob sweep on demand and compaction.

use crate::retention::checkpoint;
use crate::{blobs, db, Storage, StorageError};
use rusqlite::Connection;

/// Blob files nothing references (and `.tmp` leftovers): how many and how many bytes.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct OrphanReport {
    pub files: u64,
    pub bytes: u64,
}

/// Size of the database file before and after `compact`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct CompactReport {
    pub bytes_before: u64,
    pub bytes_after: u64,
}

/// Pending changes (notes, tags, automatic evictions) before the index is merged (batched,
/// never per keystroke or per capture).
pub(crate) const FTS_BATCH: u32 = 100;

/// Merges the contentless FTS5 index so the tokens of deleted rows leave `clips_fts_data`
/// (F-20). Best-effort: a failure leaves the rows deleted and the next merge retries.
pub(crate) fn fts_optimize(conn: &Connection) {
    let _ = conn.execute("INSERT INTO clips_fts(clips_fts) VALUES('optimize')", []);
}

fn file_len(path: &std::path::Path) -> u64 {
    std::fs::metadata(path).map_or(0, |m| m.len())
}

impl Storage {
    /// Merges the FTS index (counted in tests).
    pub(crate) fn fts_merge(&mut self) {
        #[cfg(test)]
        {
            self.optimize_runs += 1;
        }
        fts_optimize(&self.conn);
    }

    /// Note, tag and automatic-eviction changes waiting for a merge.
    pub fn fts_pending(&self) -> u32 {
        self.fts_dirty
    }

    /// A note or the tags of a clip changed: the old words stay in the index until a merge.
    /// Merges after `FTS_BATCH` changes; the actor merges the rest when idle and at shutdown.
    pub(crate) fn fts_changed(&mut self) {
        self.fts_dirty = self.fts_dirty.saturating_add(1);
        if self.fts_dirty >= FTS_BATCH {
            self.optimize_fts_if_dirty();
        }
    }

    /// Merges the index and truncates the WAL when note or tag changes are waiting.
    pub fn optimize_fts_if_dirty(&mut self) {
        if self.fts_dirty == 0 || self.ensure_writable().is_err() {
            return;
        }
        self.fts_merge();
        checkpoint(&self.conn);
        self.fts_dirty = 0;
    }

    /// Merges the index and truncates the WAL regardless of the pending count (startup).
    pub fn optimize_fts_now(&mut self) {
        if self.ensure_writable().is_err() {
            return;
        }
        self.fts_merge();
        checkpoint(&self.conn);
        self.fts_dirty = 0;
    }

    /// Blob files nothing references: how many and how many bytes (nothing is removed).
    pub fn scan_orphans(&self) -> Result<OrphanReport, StorageError> {
        let c = blobs::scan(&self.conn, &self.dir)?;
        Ok(OrphanReport {
            files: c.cleanup.removed,
            bytes: c.bytes,
        })
    }

    /// The startup sweep on demand: removes unreferenced blob files and `.tmp` leftovers and
    /// reports what it freed. A referenced blob is never removed.
    pub fn sweep_orphans(&mut self) -> Result<OrphanReport, StorageError> {
        self.ensure_writable()?;
        let c = blobs::sweep_counted(&mut self.conn, &self.dir);
        Ok(OrphanReport {
            files: c.cleanup.removed,
            bytes: c.bytes,
        })
    }

    /// `VACUUM` on the writer. Refused with `InsufficientSpace` when `free_bytes` (the free
    /// disk space, measured by the caller: this crate has no OS access) is below twice the
    /// size of the database file (VACUUM writes a full copy next to it). The index is merged first so leftover tokens go too.
    pub fn compact(&mut self, free_bytes: u64) -> Result<CompactReport, StorageError> {
        self.ensure_writable()?;
        let path = self.dir.join(db::DB_FILE);
        let bytes_before = file_len(&path);
        if free_bytes < bytes_before.saturating_mul(2) {
            return Err(StorageError::InsufficientSpace);
        }
        self.fts_merge();
        self.fts_dirty = 0;
        checkpoint(&self.conn);
        // VACUUM must not park its working copy in the system temp folder (a plain copy of the
        // history outside the data folder): keep temporary files in memory meanwhile.
        let prev: i64 = self
            .conn
            .pragma_query_value(None, "temp_store", |r| r.get(0))?;
        self.conn.pragma_update(None, "temp_store", "MEMORY")?;
        let vacuum = self.conn.execute_batch("VACUUM");
        let _ = self.conn.pragma_update(None, "temp_store", prev);
        vacuum?;
        checkpoint(&self.conn);
        Ok(CompactReport {
            bytes_before,
            bytes_after: file_len(&path),
        })
    }
}

#[cfg(test)]
mod tests {
    use crate::retention::test_support::*;
    use crate::{OpenMode, SearchQuery, Storage, StorageError};
    use darkclip_core::{CollectionSort, Mime};
    use std::path::Path;

    const DAY: i64 = 86_400_000;

    fn query(text: &str) -> SearchQuery {
        SearchQuery {
            text: Some(text.into()),
            kinds: vec![],
            pinned_only: false,
            since: None,
            include_masked: false,
            limit: 50,
            cursor: None,
            collection: None,
            tag: None,
        }
    }

    fn hits(s: &Storage, text: &str) -> Vec<String> {
        let (rows, _) = s.search(&query(text)).unwrap();
        rows.into_iter().map(|r| r.preview).collect()
    }

    /// The database file and its write-ahead log, as raw bytes.
    fn db_bytes_on_disk(dir: &Path) -> Vec<u8> {
        let mut all = std::fs::read(dir.join("history.db")).unwrap();
        if let Ok(wal) = std::fs::read(dir.join("history.db-wal")) {
            all.extend(wal);
        }
        all
    }

    fn contains(hay: &[u8], needle: &str) -> bool {
        hay.windows(needle.len()).any(|w| w == needle.as_bytes())
    }

    /// A word that is unique in the file and far from secret-shaped (GC-09).
    const TOKEN: &str = "qzvkjxwunique";

    #[test]
    fn clear_keeps_pinned_and_favorites_and_the_flag_false_deletes_everything() {
        let tmp = tempfile::tempdir().unwrap();
        let mut s = open(tmp.path(), clock());
        let fav = s.favorites_id().unwrap();
        let pinned = ins(&mut s, text_clip("pinned one"));
        let favorite = ins(&mut s, text_clip("favorite one"));
        let plain_a = ins(&mut s, text_clip("plain a"));
        ins(&mut s, text_clip("plain b"));
        let in_col = ins(&mut s, text_clip("in a user collection"));
        s.set_pinned(&pinned, true).unwrap();
        s.add_to_collection(&fav, &favorite).unwrap();
        let col = s
            .create_collection("Work", CollectionSort::Manual, None)
            .unwrap()
            .id;
        s.add_to_collection(&col, &in_col).unwrap();
        assert_eq!(s.count_clearable(true).unwrap(), 3);
        assert_eq!(s.count_clearable(false).unwrap(), 5);
        assert_eq!(s.clear_history(true).unwrap(), 3);
        assert!(s.get_clip(&pinned).unwrap().is_some());
        assert!(s.get_clip(&favorite).unwrap().is_some());
        assert!(s.get_clip(&plain_a).unwrap().is_none());
        assert!(
            s.get_clip(&in_col).unwrap().is_none(),
            "a collection protects nothing"
        );
        assert_eq!(s.clear_history(false).unwrap(), 2);
        assert_eq!(s.stats().unwrap().clips, 0);
    }

    #[test]
    fn stats_count_favorites() {
        let tmp = tempfile::tempdir().unwrap();
        let mut s = open(tmp.path(), clock());
        let fav = s.favorites_id().unwrap();
        let a = ins(&mut s, text_clip("a"));
        ins(&mut s, text_clip("b"));
        s.add_to_collection(&fav, &a).unwrap();
        assert_eq!(s.stats().unwrap().favorites, 1);
    }

    #[test]
    fn delete_older_than_respects_pinned_and_favorites() {
        let tmp = tempfile::tempdir().unwrap();
        let c = clock();
        let mut s = open(tmp.path(), c.clone());
        let fav = s.favorites_id().unwrap();
        set(&c, T0);
        let old_plain = ins(&mut s, text_clip("old plain"));
        let old_pinned = ins(&mut s, text_clip("old pinned"));
        let old_fav = ins(&mut s, text_clip("old favorite"));
        set(&c, T0 + 20 * DAY);
        let recent = ins(&mut s, text_clip("recent"));
        s.set_pinned(&old_pinned, true).unwrap();
        s.add_to_collection(&fav, &old_fav).unwrap();
        set(&c, T0 + 40 * DAY);
        assert_eq!(s.count_older_than(30).unwrap(), 1);
        assert_eq!(s.delete_older_than(30).unwrap(), 1);
        assert!(s.get_clip(&old_plain).unwrap().is_none());
        assert!(s.get_clip(&old_pinned).unwrap().is_some());
        assert!(s.get_clip(&old_fav).unwrap().is_some());
        assert!(s.get_clip(&recent).unwrap().is_some());
        assert_eq!(s.delete_older_than(30).unwrap(), 0);
        // 0 days would mean "everything": refused.
        assert!(matches!(
            s.delete_older_than(0),
            Err(StorageError::InvalidArgument(_))
        ));
        assert!(matches!(
            s.count_older_than(0),
            Err(StorageError::InvalidArgument(_))
        ));
    }

    #[test]
    fn orphan_sweep_reports_counts_and_never_deletes_a_referenced_blob() {
        let tmp = tempfile::tempdir().unwrap();
        let mut s = open(tmp.path(), clock());
        let id = ins(
            &mut s,
            new_clip(vec![raw(Mime::TextPlain, &vec![7u8; 100 * 1024])]),
        );
        let shard = tmp.path().join("blobs").join("ab");
        std::fs::create_dir_all(&shard).unwrap();
        std::fs::write(shard.join("c".repeat(64)), vec![0u8; 1000]).unwrap();
        std::fs::write(
            shard.join(format!("{}.tmp", "d".repeat(64))),
            vec![0u8; 234],
        )
        .unwrap();
        std::fs::write(shard.join("notes.txt"), b"not a blob").unwrap();
        let before = files_under(&tmp.path().join("blobs")).len();
        // A scan changes nothing.
        let scan = s.scan_orphans().unwrap();
        assert_eq!((scan.files, scan.bytes), (2, 1234));
        assert_eq!(files_under(&tmp.path().join("blobs")).len(), before);
        let done = s.sweep_orphans().unwrap();
        assert_eq!((done.files, done.bytes), (2, 1234));
        assert!(shard.join("notes.txt").exists(), "foreign files untouched");
        assert_eq!(files_under(&tmp.path().join("blobs")).len(), before - 2);
        assert_eq!(
            s.read_representation(&id, 0).unwrap(),
            vec![7u8; 100 * 1024]
        );
        assert_eq!(s.sweep_orphans().unwrap(), super::OrphanReport::default());
        assert_eq!(s.scan_orphans().unwrap(), super::OrphanReport::default());
    }

    #[test]
    fn compact_keeps_data_and_search_results_identical() {
        let tmp = tempfile::tempdir().unwrap();
        let mut s = open(tmp.path(), clock());
        let mut keep = Vec::new();
        for i in 0..40 {
            keep.push(ins(
                &mut s,
                text_clip(&format!("compactword number {i} {}", "x".repeat(900))),
            ));
        }
        for id in keep.drain(..30) {
            s.delete_clip(&id).unwrap();
        }
        let before_hits = hits(&s, "compactword");
        let before_stats = s.stats().unwrap();
        let file_before = std::fs::metadata(tmp.path().join("history.db"))
            .unwrap()
            .len();
        let report = s.compact(u64::MAX).unwrap();
        assert_eq!(report.bytes_before, file_before);
        assert!(report.bytes_after <= report.bytes_before);
        assert_eq!(
            report.bytes_after,
            std::fs::metadata(tmp.path().join("history.db"))
                .unwrap()
                .len()
        );
        assert_eq!(hits(&s, "compactword"), before_hits);
        assert_eq!(s.stats().unwrap().clips, before_stats.clips);
        assert_eq!(s.stats().unwrap().by_kind, before_stats.by_kind);
        for id in &keep {
            assert!(s.get_clip(id).unwrap().is_some());
        }
        // The store keeps working after the compaction.
        ins(&mut s, text_clip("after compact"));
        assert_eq!(hits(&s, "after compact").len(), 1);
    }

    #[test]
    fn compact_is_refused_when_free_space_is_below_the_database_size() {
        let tmp = tempfile::tempdir().unwrap();
        let mut s = open(tmp.path(), clock());
        ins(&mut s, text_clip("something"));
        let size = std::fs::metadata(tmp.path().join("history.db"))
            .unwrap()
            .len();
        // VACUUM writes a full copy next to the database: twice its size must be free.
        assert_eq!(
            s.compact(2 * size - 1),
            Err(StorageError::InsufficientSpace)
        );
        assert!(s.compact(2 * size).is_ok());
        let mut ro = Storage::open(tmp.path(), OpenMode::ReadOnly, clock()).unwrap();
        assert_eq!(ro.compact(u64::MAX), Err(StorageError::ReadOnly));
        assert_eq!(ro.delete_older_than(1), Err(StorageError::ReadOnly));
        assert_eq!(ro.sweep_orphans(), Err(StorageError::ReadOnly));
    }

    #[test]
    fn f20_deleting_a_clip_removes_its_tokens_from_the_database_file() {
        let tmp = tempfile::tempdir().unwrap();
        let mut s = open(tmp.path(), clock());
        let id = ins(&mut s, text_clip(&format!("body with {TOKEN} inside")));
        ins(&mut s, text_clip("another clip stays"));
        assert_eq!(hits(&s, TOKEN).len(), 1);
        assert!(contains(&db_bytes_on_disk(tmp.path()), TOKEN));
        s.delete_clip(&id).unwrap();
        assert!(hits(&s, TOKEN).is_empty());
        assert!(!contains(&db_bytes_on_disk(tmp.path()), TOKEN));
    }

    #[test]
    fn f20_clear_removes_tokens_too() {
        let tmp = tempfile::tempdir().unwrap();
        let mut s = open(tmp.path(), clock());
        ins(&mut s, text_clip(&format!("clear me {TOKEN}")));
        assert!(contains(&db_bytes_on_disk(tmp.path()), TOKEN));
        s.clear_history(false).unwrap();
        assert!(!contains(&db_bytes_on_disk(tmp.path()), TOKEN));
    }

    #[test]
    fn f20_replaced_notes_and_removed_tags_lose_their_tokens_after_the_batched_optimize() {
        let tmp = tempfile::tempdir().unwrap();
        let mut s = open(tmp.path(), clock());
        let id = ins(&mut s, text_clip("a clip to annotate"));
        s.set_note(&id, Some(&format!("note {TOKEN}"))).unwrap();
        s.set_clip_tags(&id, &["tagqzvkjtoken"]).unwrap();
        assert!(contains(&db_bytes_on_disk(tmp.path()), TOKEN));
        s.set_note(&id, Some("a different note")).unwrap();
        s.set_clip_tags::<&str>(&id, &[]).unwrap();
        // Batched: the actor calls this when idle and at shutdown.
        s.optimize_fts_if_dirty();
        let _ = s
            .conn
            .query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |_| Ok(()));
        let bytes = db_bytes_on_disk(tmp.path());
        assert!(!contains(&bytes, TOKEN));
        assert!(!contains(&bytes, "tagqzvkjtoken"));
    }

    #[test]
    fn f20_compact_removes_leftover_tokens() {
        let tmp = tempfile::tempdir().unwrap();
        let mut s = open(tmp.path(), clock());
        let id = ins(&mut s, text_clip("clip"));
        s.set_note(&id, Some(&format!("note {TOKEN}"))).unwrap();
        s.set_note(&id, None).unwrap();
        s.compact(u64::MAX).unwrap();
        assert!(!contains(&db_bytes_on_disk(tmp.path()), TOKEN));
    }

    fn evictable_store(dir: &Path) -> (Storage, darkclip_core::RetentionPolicy) {
        let c = clock();
        let mut s = open(dir, c.clone());
        // Distinct times: the oldest clip is the one evicted.
        ins(&mut s, text_clip(&format!("oldest {TOKEN}")));
        set(&c, T0 + 1000);
        ins(&mut s, text_clip("middle"));
        set(&c, T0 + 2000);
        ins(&mut s, text_clip("newest"));
        let p = darkclip_core::RetentionPolicy {
            max_items: 2,
            max_age_days: 0,
            max_storage_mb: 0,
            ..darkclip_core::RetentionPolicy::default()
        };
        (s, p)
    }

    #[test]
    fn b1_automatic_eviction_defers_the_merge_and_the_tick_runs_it() {
        let tmp = tempfile::tempdir().unwrap();
        let (mut s, p) = evictable_store(tmp.path());
        let runs = s.optimize_runs;
        let r = s.apply_retention(&p).unwrap();
        assert_eq!(r.evicted_count, 1);
        assert_eq!(s.optimize_runs, runs, "no merge in the retention path");
        assert_eq!(s.fts_pending(), 1);
        // The 10-minute tick (also startup and shutdown) merges and truncates the WAL.
        s.optimize_fts_if_dirty();
        assert_eq!(s.optimize_runs, runs + 1);
        assert_eq!(s.fts_pending(), 0);
        assert!(!contains(&db_bytes_on_disk(tmp.path()), TOKEN));
        // Nothing pending: the tick does nothing.
        s.optimize_fts_if_dirty();
        assert_eq!(s.optimize_runs, runs + 1);
    }

    #[test]
    fn b1_one_hundred_pending_changes_trigger_the_merge() {
        let tmp = tempfile::tempdir().unwrap();
        let mut s = open(tmp.path(), clock());
        for i in 0..101 {
            ins(&mut s, text_clip(&format!("clip {i}")));
        }
        let p = darkclip_core::RetentionPolicy {
            max_items: 1,
            max_age_days: 0,
            max_storage_mb: 0,
            ..darkclip_core::RetentionPolicy::default()
        };
        let runs = s.optimize_runs;
        assert_eq!(s.apply_retention(&p).unwrap().evicted_count, 100);
        assert_eq!(s.optimize_runs, runs + 1);
        assert_eq!(s.fts_pending(), 0);
    }

    #[test]
    fn b1_user_deletions_still_merge_at_once() {
        let tmp = tempfile::tempdir().unwrap();
        let mut s = open(tmp.path(), clock());
        let a = ins(&mut s, text_clip("a"));
        ins(&mut s, text_clip("b"));
        ins(&mut s, text_clip("c"));
        let runs = s.optimize_runs;
        let _ = a;
        ins(&mut s, text_clip("newest"));
        let p = darkclip_core::RetentionPolicy {
            max_items: 2,
            max_age_days: 0,
            max_storage_mb: 0,
            ..darkclip_core::RetentionPolicy::default()
        };
        s.apply_retention(&p).unwrap();
        s.optimize_fts_if_dirty();
        assert_eq!(s.optimize_runs, runs + 1);
        s.clear_history(false).unwrap();
        assert_eq!(s.optimize_runs, runs + 2);
    }

    #[test]
    fn s2_recapture_as_masked_removes_the_former_plain_tokens_at_once() {
        use darkclip_core::{SensitiveCategory, Sensitivity, SensitivityLevel, Timestamp};
        let tmp = tempfile::tempdir().unwrap();
        let mut s = open(tmp.path(), clock());
        // Over 64 KiB: the body lives in a blob file, so only the index and preview hold the word.
        let text = format!("plain body {TOKEN} {}", "x".repeat(70 * 1024));
        let id = ins(&mut s, text_clip(&text));
        assert!(contains(&db_bytes_on_disk(tmp.path()), TOKEN));
        let mut m = text_clip(&text);
        m.classified.preview = "masked preview".into();
        m.sensitivity = Sensitivity {
            level: SensitivityLevel::Masked,
            categories: vec![SensitiveCategory::ApiKey],
        };
        m.expires_at = Some(Timestamp(T0 + 3_600_000));
        assert_eq!(s.upsert_clip(&m).unwrap(), crate::UpsertOutcome::Bumped(id));
        assert!(!contains(&db_bytes_on_disk(tmp.path()), TOKEN));
    }
}
