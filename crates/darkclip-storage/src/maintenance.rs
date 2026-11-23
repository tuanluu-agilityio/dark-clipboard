//! Data management (Task 411): cleanup tools, orphan-blob sweep on demand and compaction.

use crate::{Storage, StorageError};

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

impl Storage {
    pub fn count_clearable(&self, _keep_pinned: bool) -> Result<u64, StorageError> {
        Err(StorageError::NotFound)
    }

    pub fn count_older_than(&self, _days: u32) -> Result<u64, StorageError> {
        Err(StorageError::NotFound)
    }

    pub fn delete_older_than(&mut self, _days: u32) -> Result<u64, StorageError> {
        Err(StorageError::NotFound)
    }

    pub fn scan_orphans(&self) -> Result<OrphanReport, StorageError> {
        Err(StorageError::NotFound)
    }

    pub fn sweep_orphans(&mut self) -> Result<OrphanReport, StorageError> {
        Err(StorageError::NotFound)
    }

    pub fn compact(&mut self, _free_bytes: u64) -> Result<CompactReport, StorageError> {
        Err(StorageError::NotFound)
    }

    pub fn optimize_fts_if_dirty(&mut self) {}
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
        assert_eq!(s.compact(size - 1), Err(StorageError::InsufficientSpace));
        assert!(s.compact(size).is_ok());
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
}
