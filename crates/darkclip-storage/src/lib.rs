//! Storage: SQLite, FTS5, blobs and migrations (`docs/architecture/data-model.md` §4-7).
//!
//! Layout under the data directory: `history.db` (WAL) and `blobs/<aa>/<sha256 hex>`.
//!
//! Permissions: on Unix the data directory and `blobs/` subdirectories are 0700 and
//! every file this crate creates (database, blobs, backups) is 0600. SQLite creates its
//! `-wal`/`-shm` files with the main database file's mode. On Windows no ACL is set:
//! the data directory lives in the per-user profile (`%LOCALAPPDATA%`), whose default
//! ACL already restricts access to the user, SYSTEM and administrators.
//!
//! Must not depend on: OS crates, `tauri`.

#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used, clippy::panic))]

mod blobs;
mod clips;
mod db;
mod error;
mod maintenance;
mod migrate;
mod organize;
mod retention;
mod search;
mod stats;

use darkclip_core::{
    Classified, ClipId, ClipboardSource, Clock, RawRepresentation, Sensitivity, Timestamp,
};
use std::path::PathBuf;
use std::sync::Arc;

pub use error::StorageError;
pub use maintenance::{CompactReport, OrphanReport};
pub use organize::TagCount;
pub use retention::RetentionReport;
pub use search::{ClipSummary, Cursor, SearchQuery};
pub use stats::StorageStats;

/// The clip store: one SQLite connection plus the data directory.
pub struct Storage {
    conn: rusqlite::Connection,
    dir: PathBuf,
    mode: OpenMode,
    clock: Arc<dyn Clock>,
    /// Note and tag changes not yet merged into the FTS index (F-20).
    fts_dirty: u32,
    /// Test hook: how many times the FTS index was merged.
    #[cfg(test)]
    optimize_runs: u32,
    /// Test hook: fail `upsert_clip` after blobs are written, before commit.
    #[cfg(test)]
    fail_before_commit: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OpenMode {
    ReadWrite,
    ReadOnly,
}

/// A classified capture ready to store. `classified.preview` must already be the
/// masked preview for masked clips; storage stores what it is given. `upsert_clip` borrows it, so the
/// capture actor can retry once after `StorageFull` (F-02) with the same value and wipe it
/// afterwards.
pub struct NewClip {
    pub classified: Classified,
    pub hash: [u8; 32],
    pub reps: Vec<RawRepresentation>,
    pub source: Option<ClipboardSource>,
    pub sensitivity: Sensitivity,
    pub expires_at: Option<Timestamp>,
    pub thumbnail_png: Option<Vec<u8>>,
}

/// Prints kind, representation formats and sizes only (GC-06).
impl std::fmt::Debug for NewClip {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("NewClip")
            .field("kind", &self.classified.kind)
            .field("reps", &self.reps)
            .field("sensitivity", &self.sensitivity)
            .field("thumbnail_len", &self.thumbnail_png.as_ref().map(Vec::len))
            .finish_non_exhaustive()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UpsertOutcome {
    Inserted(ClipId),
    Bumped(ClipId),
}

#[cfg(test)]
mod tests {
    use super::*;
    use darkclip_core::{
        canonical_hash, classify, ClipType, Mime, Platform, SensitiveCategory, SensitivityLevel,
        SourceConfidence, Storage as RepStorage,
    };
    use std::path::Path;
    use std::sync::atomic::{AtomicI64, Ordering};

    struct TestClock(AtomicI64);
    impl Clock for TestClock {
        fn now(&self) -> Timestamp {
            Timestamp(self.0.fetch_add(1000, Ordering::SeqCst))
        }
    }

    fn clock() -> Arc<dyn Clock> {
        Arc::new(TestClock(AtomicI64::new(1_795_320_000_000)))
    }

    fn open(dir: &Path) -> Storage {
        Storage::open(dir, OpenMode::ReadWrite, clock()).unwrap()
    }

    fn raw(mime: Mime, bytes: &[u8]) -> RawRepresentation {
        RawRepresentation {
            mime,
            platform_format: "test-format".into(),
            bytes: bytes.to_vec(),
            width: None,
            height: None,
            truncated: false,
        }
    }

    fn new_clip(reps: Vec<RawRepresentation>) -> NewClip {
        let classified = classify(&reps);
        let hash = canonical_hash(classified.kind, &reps, None);
        NewClip {
            classified,
            hash,
            reps,
            source: Some(ClipboardSource {
                app_id: "org.example.editor".into(),
                app_name: Some("Editor".into()),
                platform: Platform::Windows,
                confidence: SourceConfidence::Owner,
            }),
            sensitivity: Sensitivity::default(),
            expires_at: None,
            thumbnail_png: None,
        }
    }

    fn text_clip(s: &str) -> NewClip {
        new_clip(vec![raw(Mime::TextPlain, s.as_bytes())])
    }

    /// A tiny valid PNG (1x1), built at runtime.
    fn png_bytes(seed: u8) -> Vec<u8> {
        let mut out = Vec::new();
        {
            let mut enc = png::Encoder::new(&mut out, 1, 1);
            enc.set_color(png::ColorType::Rgba);
            enc.set_depth(png::BitDepth::Eight);
            let mut w = enc.write_header().unwrap();
            w.write_image_data(&[seed, 2, 3, 255]).unwrap();
        }
        out
    }

    fn files_under(dir: &Path) -> Vec<std::path::PathBuf> {
        let mut out = Vec::new();
        let Ok(rd) = std::fs::read_dir(dir) else {
            return out;
        };
        for e in rd {
            let p = e.unwrap().path();
            if p.is_dir() {
                out.extend(files_under(&p));
            } else {
                out.push(p);
            }
        }
        out
    }

    fn count(s: &Storage, sql: &str) -> i64 {
        s.conn.query_row(sql, [], |r| r.get(0)).unwrap()
    }

    #[test]
    fn open_creates_schema_v2() {
        let tmp = tempfile::tempdir().unwrap();
        let s = open(tmp.path());
        // Migration 2 (Task 401) runs on every new store: the current version is 2.
        assert_eq!(s.schema_version().unwrap(), 2);
        let uv: u32 = s
            .conn
            .pragma_query_value(None, "user_version", |r| r.get(0))
            .unwrap();
        assert_eq!(uv, 2);
        let jm: String = s
            .conn
            .pragma_query_value(None, "journal_mode", |r| r.get(0))
            .unwrap();
        assert_eq!(jm.to_lowercase(), "wal");
        let sd: i64 = s
            .conn
            .pragma_query_value(None, "secure_delete", |r| r.get(0))
            .unwrap();
        assert_eq!(sd, 1);
        let fk: i64 = s
            .conn
            .pragma_query_value(None, "foreign_keys", |r| r.get(0))
            .unwrap();
        assert_eq!(fk, 1);
        for t in ["clips", "representations", "clips_fts", "clip_rowid"] {
            let n = s
                .conn
                .query_row(
                    "SELECT count(*) FROM sqlite_master WHERE name = ?1",
                    [t],
                    |r| r.get::<_, i64>(0),
                )
                .unwrap();
            assert_eq!(n, 1, "{t}");
        }
        assert!(tmp.path().join("history.db").is_file());
        assert!(tmp.path().join("blobs").is_dir());
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = |p: &Path| std::fs::metadata(p).unwrap().permissions().mode() & 0o777;
            assert_eq!(mode(tmp.path()), 0o700);
            assert_eq!(mode(&tmp.path().join("blobs")), 0o700);
            assert_eq!(mode(&tmp.path().join("history.db")), 0o600);
        }
    }

    #[test]
    fn upsert_same_hash_bumps() {
        let tmp = tempfile::tempdir().unwrap();
        let mut s = open(tmp.path());
        let UpsertOutcome::Inserted(id) = s.upsert_clip(&text_clip("hello world")).unwrap() else {
            panic!("expected Inserted");
        };
        let first = s.get_clip(&id).unwrap().unwrap();
        let mut again = text_clip("hello world");
        again.source = Some(ClipboardSource {
            app_id: "org.example.browser".into(),
            app_name: None,
            platform: Platform::Windows,
            confidence: SourceConfidence::Frontmost,
        });
        assert_eq!(s.upsert_clip(&again).unwrap(), UpsertOutcome::Bumped(id));
        assert_eq!(count(&s, "SELECT count(*) FROM clips"), 1);
        assert_eq!(count(&s, "SELECT count(*) FROM representations"), 1);
        let c = s.get_clip(&id).unwrap().unwrap();
        assert_eq!(c.copy_count, 2);
        assert!(c.last_copied_at > first.last_copied_at);
        assert_eq!(c.created_at, first.created_at);
        assert_eq!(
            c.source.as_ref().map(|s| s.app_id.as_str()),
            Some("org.example.browser")
        );
        assert_eq!(c.validate(), Ok(()));
    }

    #[test]
    fn small_text_inline() {
        let tmp = tempfile::tempdir().unwrap();
        let mut s = open(tmp.path());
        let UpsertOutcome::Inserted(id) = s.upsert_clip(&text_clip("short text")).unwrap() else {
            panic!("expected Inserted");
        };
        let c = s.get_clip(&id).unwrap().unwrap();
        assert_eq!(c.kind, ClipType::Text);
        assert_eq!(c.preview, "short text");
        assert_eq!(c.representations.len(), 1);
        assert_eq!(c.representations[0].storage, RepStorage::Inline);
        assert_eq!(c.representations[0].blob_sha256, None);
        assert_eq!(c.representations[0].size_bytes, 10);
        assert_eq!(c.size_bytes, 10);
        assert_eq!(c.metadata.char_count, Some(10));
        assert_eq!(s.read_representation(&id, 0).unwrap(), b"short text");
        assert!(files_under(&tmp.path().join("blobs")).is_empty());
        assert_eq!(s.read_representation(&id, 1), Err(StorageError::NotFound));
    }

    #[test]
    fn large_text_goes_to_blob() {
        let tmp = tempfile::tempdir().unwrap();
        let mut s = open(tmp.path());
        let text = "x".repeat(70 * 1024);
        let UpsertOutcome::Inserted(id) = s.upsert_clip(&text_clip(&text)).unwrap() else {
            panic!("expected Inserted");
        };
        let c = s.get_clip(&id).unwrap().unwrap();
        let rep = &c.representations[0];
        assert_eq!(rep.storage, RepStorage::Blob);
        let sha = rep.blob_sha256.unwrap();
        let files = files_under(&tmp.path().join("blobs"));
        assert_eq!(files.len(), 1);
        let hex: String = sha.iter().map(|b| format!("{b:02x}")).collect();
        assert_eq!(
            files[0],
            tmp.path().join("blobs").join(&hex[..2]).join(&hex)
        );
        assert_eq!(
            count(
                &s,
                "SELECT count(*) FROM representations WHERE inline_data IS NOT NULL"
            ),
            0
        );
        assert_eq!(s.read_representation(&id, 0).unwrap(), text.as_bytes());
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = |p: &Path| std::fs::metadata(p).unwrap().permissions().mode() & 0o777;
            assert_eq!(mode(&files[0]), 0o600);
            assert_eq!(mode(files[0].parent().unwrap()), 0o700);
        }
        // exactly 64 KiB stays inline
        let edge = "y".repeat(64 * 1024);
        let UpsertOutcome::Inserted(id2) = s.upsert_clip(&text_clip(&edge)).unwrap() else {
            panic!("expected Inserted");
        };
        let c2 = s.get_clip(&id2).unwrap().unwrap();
        assert_eq!(c2.representations[0].storage, RepStorage::Inline);
    }

    #[test]
    fn identical_image_two_clips_one_blob_file() {
        let tmp = tempfile::tempdir().unwrap();
        let mut s = open(tmp.path());
        let png = png_bytes(9);
        let mut img = raw(Mime::ImagePng, &png);
        img.width = Some(1);
        img.height = Some(1);
        let a = new_clip(vec![raw(Mime::TextPlain, b"caption one"), img.clone()]);
        let b = new_clip(vec![raw(Mime::TextPlain, b"caption two"), img]);
        let UpsertOutcome::Inserted(ia) = s.upsert_clip(&a).unwrap() else {
            panic!("expected Inserted");
        };
        let UpsertOutcome::Inserted(ib) = s.upsert_clip(&b).unwrap() else {
            panic!("expected Inserted");
        };
        assert_ne!(ia, ib);
        assert_eq!(files_under(&tmp.path().join("blobs")).len(), 1);
        let ca = s.get_clip(&ia).unwrap().unwrap();
        let cb = s.get_clip(&ib).unwrap().unwrap();
        assert_eq!(ca.representations[0].storage, RepStorage::Inline);
        assert_eq!(ca.representations[1].storage, RepStorage::Blob);
        assert_eq!(ca.representations[1].width, Some(1));
        assert!(ca.representations[1].blob_sha256.is_some());
        assert_eq!(
            ca.representations[1].blob_sha256,
            cb.representations[1].blob_sha256
        );
        assert_eq!(s.read_representation(&ib, 1).unwrap(), png);
    }

    #[test]
    fn thumbnail_stored_as_blob_and_referenced() {
        let tmp = tempfile::tempdir().unwrap();
        let mut s = open(tmp.path());
        let png = png_bytes(1);
        let mut img = raw(Mime::ImagePng, &png);
        img.width = Some(1);
        img.height = Some(1);
        let mut n = new_clip(vec![img]);
        let thumb = png_bytes(2);
        n.thumbnail_png = Some(thumb.clone());
        let UpsertOutcome::Inserted(id) = s.upsert_clip(&n).unwrap() else {
            panic!("expected Inserted");
        };
        let c = s.get_clip(&id).unwrap().unwrap();
        assert_eq!(c.kind, ClipType::Image);
        let t = c.metadata.image_thumb_sha256.unwrap();
        assert_eq!(files_under(&tmp.path().join("blobs")).len(), 2);
        assert_eq!(blobs::read(tmp.path(), &t).unwrap(), thumb);
        drop(s);
        // the orphan sweep at open keeps the thumbnail blob
        let _s = open(tmp.path());
        assert_eq!(files_under(&tmp.path().join("blobs")).len(), 2);
    }

    #[test]
    fn read_thumbnail_returns_the_blob_and_none_without_one() {
        let tmp = tempfile::tempdir().unwrap();
        let mut s = open(tmp.path());
        let mut n = new_clip(vec![raw(Mime::ImagePng, &png_bytes(1))]);
        let thumb = png_bytes(2);
        n.thumbnail_png = Some(thumb.clone());
        let UpsertOutcome::Inserted(with) = s.upsert_clip(&n).unwrap() else {
            panic!("expected Inserted");
        };
        assert_eq!(s.read_thumbnail(&with).unwrap(), Some(thumb));
        let UpsertOutcome::Inserted(text) = s.upsert_clip(&text_clip("no picture")).unwrap() else {
            panic!("expected Inserted");
        };
        assert_eq!(s.read_thumbnail(&text).unwrap(), None);
        let missing = ClipId::new_v7(Timestamp(1));
        assert_eq!(s.read_thumbnail(&missing).unwrap(), None);
    }

    #[test]
    fn mark_used_and_set_pinned() {
        let tmp = tempfile::tempdir().unwrap();
        let mut s = open(tmp.path());
        let UpsertOutcome::Inserted(id) = s.upsert_clip(&text_clip("pin me")).unwrap() else {
            panic!("expected Inserted");
        };
        s.mark_used(&id).unwrap();
        s.set_pinned(&id, true).unwrap();
        let c = s.get_clip(&id).unwrap().unwrap();
        assert_eq!(c.use_count, 1);
        assert!(c.last_used_at.is_some());
        assert!(c.pinned);
        s.set_pinned(&id, false).unwrap();
        assert!(!s.get_clip(&id).unwrap().unwrap().pinned);
        let missing = ClipId::new_v7(Timestamp(1));
        assert_eq!(s.mark_used(&missing), Err(StorageError::NotFound));
        assert_eq!(s.set_pinned(&missing, true), Err(StorageError::NotFound));
        assert_eq!(s.get_clip(&missing), Ok(None));
    }

    #[test]
    fn masked_clip_requires_expiry_and_roundtrips() {
        let tmp = tempfile::tempdir().unwrap();
        let mut s = open(tmp.path());
        let mut n = text_clip(concat!("sk-", "synthetic0000000000000000"));
        n.classified.preview = "sk-\u{2026}\u{2022}\u{2022}\u{2022}\u{2022} (API key)".into();
        n.sensitivity = Sensitivity {
            level: SensitivityLevel::Masked,
            categories: vec![SensitiveCategory::ApiKey],
        };
        assert!(matches!(
            s.upsert_clip(&n),
            Err(StorageError::Invalid(darkclip_core::CoreError::Invalid {
                field: "expires_at"
            }))
        ));
        assert_eq!(count(&s, "SELECT count(*) FROM clips"), 0);
        let mut n = text_clip(concat!("sk-", "synthetic0000000000000000"));
        n.sensitivity = Sensitivity {
            level: SensitivityLevel::Masked,
            categories: vec![SensitiveCategory::ApiKey],
        };
        n.expires_at = Some(Timestamp(1_795_320_120_000));
        let UpsertOutcome::Inserted(id) = s.upsert_clip(&n).unwrap() else {
            panic!("expected Inserted");
        };
        let c = s.get_clip(&id).unwrap().unwrap();
        assert_eq!(c.sensitivity.level, SensitivityLevel::Masked);
        assert_eq!(c.sensitivity.categories, vec![SensitiveCategory::ApiKey]);
        assert_eq!(c.expires_at, Some(Timestamp(1_795_320_120_000)));
    }

    #[test]
    fn upsert_rejects_invalid_clip_without_writing() {
        let tmp = tempfile::tempdir().unwrap();
        let mut s = open(tmp.path());
        let mut n = new_clip(vec![]);
        n.hash = [3; 32];
        assert!(matches!(s.upsert_clip(&n), Err(StorageError::Invalid(_))));
        let mut big = text_clip(&"z".repeat(70 * 1024));
        big.classified.preview = "p".repeat(600);
        assert!(matches!(s.upsert_clip(&big), Err(StorageError::Invalid(_))));
        assert_eq!(count(&s, "SELECT count(*) FROM clips"), 0);
        assert!(files_under(&tmp.path().join("blobs")).is_empty());
    }

    #[test]
    fn read_only_cannot_write() {
        let tmp = tempfile::tempdir().unwrap();
        let id = {
            let mut s = open(tmp.path());
            match s.upsert_clip(&text_clip("stored")).unwrap() {
                UpsertOutcome::Inserted(id) => id,
                UpsertOutcome::Bumped(_) => panic!("expected Inserted"),
            }
        };
        let mut ro = Storage::open(tmp.path(), OpenMode::ReadOnly, clock()).unwrap();
        assert_eq!(ro.schema_version().unwrap(), 2);
        assert!(ro.get_clip(&id).unwrap().is_some());
        assert_eq!(ro.read_representation(&id, 0).unwrap(), b"stored");
        assert_eq!(
            ro.upsert_clip(&text_clip("new")),
            Err(StorageError::ReadOnly)
        );
        assert_eq!(ro.mark_used(&id), Err(StorageError::ReadOnly));
        assert_eq!(ro.set_pinned(&id, true), Err(StorageError::ReadOnly));
        // the connection itself is read-only too
        assert!(ro.conn.execute("DELETE FROM clips", []).is_err());
        assert_eq!(count(&ro, "SELECT count(*) FROM clips"), 1);
        // read-only open of a missing store does not create it
        let empty = tempfile::tempdir().unwrap();
        let missing = empty.path().join("none");
        assert!(Storage::open(&missing, OpenMode::ReadOnly, clock()).is_err());
        assert!(!missing.exists());
    }

    #[test]
    fn newer_schema_rejected() {
        let tmp = tempfile::tempdir().unwrap();
        drop(open(tmp.path()));
        {
            let c = rusqlite::Connection::open(tmp.path().join("history.db")).unwrap();
            c.pragma_update(None, "user_version", 99).unwrap();
        }
        let want = StorageError::SchemaTooNew {
            found: 99,
            supported: 2,
        };
        assert_eq!(
            Storage::open(tmp.path(), OpenMode::ReadOnly, clock()).err(),
            Some(want.clone())
        );
        assert_eq!(
            Storage::open(tmp.path(), OpenMode::ReadWrite, clock()).err(),
            Some(want)
        );
        let c = rusqlite::Connection::open(tmp.path().join("history.db")).unwrap();
        let uv: u32 = c
            .pragma_query_value(None, "user_version", |r| r.get(0))
            .unwrap();
        assert_eq!(uv, 99, "never downgraded");
        assert!(!tmp.path().join("history.db.pre-v1.bak").exists());
    }

    #[test]
    fn migration_backup_created_and_removed() {
        let tmp = tempfile::tempdir().unwrap();
        let db = tmp.path().join("history.db");
        // Named after the target version (2 since Task 401).
        let bak = tmp.path().join("history.db.pre-v2.bak");
        {
            // an existing version-0 database (e.g. created by an interrupted first run)
            let c = rusqlite::Connection::open(&db).unwrap();
            c.execute_batch("CREATE TABLE legacy(x); INSERT INTO legacy VALUES (42);")
                .unwrap();
        }
        // a fresh store gets no backup; this one existed, so it does
        let s = open(tmp.path());
        assert_eq!(s.schema_version().unwrap(), 2);
        assert!(bak.is_file(), "backup created before migrating");
        {
            let b = rusqlite::Connection::open(&bak).unwrap();
            let uv: u32 = b
                .pragma_query_value(None, "user_version", |r| r.get(0))
                .unwrap();
            assert_eq!(uv, 0);
            let x: i64 = b
                .query_row("SELECT x FROM legacy", [], |r| r.get(0))
                .unwrap();
            assert_eq!(x, 42);
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                std::fs::metadata(&bak).unwrap().permissions().mode() & 0o777,
                0o600
            );
        }
        drop(s);
        // the next successful run at version 2 removes it
        let s = open(tmp.path());
        assert_eq!(s.schema_version().unwrap(), 2);
        assert!(!bak.exists(), "backup removed after a successful run");

        // a brand-new store never creates one
        let fresh = tempfile::tempdir().unwrap();
        drop(open(fresh.path()));
        assert!(!fresh.path().join("history.db.pre-v2.bak").exists());
    }

    #[test]
    fn failed_migration_keeps_backup_and_version() {
        let tmp = tempfile::tempdir().unwrap();
        let db = tmp.path().join("history.db");
        {
            let c = rusqlite::Connection::open(&db).unwrap();
            c.execute_batch("CREATE TABLE legacy(x);").unwrap();
        }
        let bad = [migrate::Migration {
            version: 1,
            sql: "CREATE TABLE half(a); SELECT no_such_function();",
        }];
        assert!(db::open_with(tmp.path(), OpenMode::ReadWrite, clock(), &bad).is_err());
        assert!(tmp.path().join("history.db.pre-v1.bak").is_file());
        let c = rusqlite::Connection::open(&db).unwrap();
        let uv: u32 = c
            .pragma_query_value(None, "user_version", |r| r.get(0))
            .unwrap();
        assert_eq!(uv, 0);
        let half: i64 = c
            .query_row(
                "SELECT count(*) FROM sqlite_master WHERE name = 'half'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(half, 0, "migration rolled back");
    }

    #[test]
    fn crash_between_blob_and_commit_leaves_orphan_only() {
        let tmp = tempfile::tempdir().unwrap();
        let mut s = open(tmp.path());
        s.fail_before_commit = true;
        let text = "q".repeat(70 * 1024);
        assert!(s.upsert_clip(&text_clip(&text)).is_err());
        assert_eq!(
            files_under(&tmp.path().join("blobs")).len(),
            1,
            "orphan blob"
        );
        assert_eq!(count(&s, "SELECT count(*) FROM clips"), 0);
        assert_eq!(count(&s, "SELECT count(*) FROM representations"), 0);
        assert_eq!(count(&s, "SELECT count(*) FROM clip_rowid"), 0);
        // a leftover temp file from a crash mid-write is swept as well
        let stray = tmp.path().join("blobs").join("ab");
        std::fs::create_dir_all(&stray).unwrap();
        std::fs::write(stray.join(format!("{}.tmp", "ab".repeat(32))), b"partial").unwrap();
        drop(s);
        let s = open(tmp.path());
        assert!(files_under(&tmp.path().join("blobs")).is_empty(), "swept");
        assert_eq!(count(&s, "SELECT count(*) FROM clips"), 0);
        assert_eq!(
            count(
                &s,
                "SELECT count(*) FROM representations r \
                 LEFT JOIN clips c ON c.id = r.clip_id WHERE c.id IS NULL"
            ),
            0
        );
    }

    #[test]
    fn bump_replaces_representations_and_drops_unreferenced_blob() {
        let tmp = tempfile::tempdir().unwrap();
        let mut s = open(tmp.path());
        // same plain text, so same hash; first copy carried a large HTML rep (blob)
        let html = format!("<p>{}</p>", "h".repeat(70 * 1024));
        let first = new_clip(vec![
            raw(Mime::TextPlain, b"para"),
            raw(Mime::TextHtml, html.as_bytes()),
        ]);
        let UpsertOutcome::Inserted(id) = s.upsert_clip(&first).unwrap() else {
            panic!("expected Inserted");
        };
        assert_eq!(files_under(&tmp.path().join("blobs")).len(), 1);
        let second = new_clip(vec![
            raw(Mime::TextPlain, b"para"),
            raw(Mime::TextHtml, b"<p>para</p>"),
        ]);
        assert_eq!(s.upsert_clip(&second).unwrap(), UpsertOutcome::Bumped(id));
        let c = s.get_clip(&id).unwrap().unwrap();
        assert_eq!(c.representations.len(), 2);
        assert_eq!(c.representations[1].storage, RepStorage::Inline);
        assert_eq!(s.read_representation(&id, 1).unwrap(), b"<p>para</p>");
        assert!(
            files_under(&tmp.path().join("blobs")).is_empty(),
            "old blob unlinked after commit"
        );
    }

    #[test]
    fn error_display_has_no_content() {
        let secret = concat!("TOP", "SECRET-", "clip-text-4242");
        let tmp = tempfile::tempdir().unwrap();
        let s = open(tmp.path());
        // a real constraint error whose parameters carry user text (NOT NULL on kind)
        let e = s
            .conn
            .execute(
                "INSERT INTO clips (id, kind, preview, content_hash, size_bytes, created_at, \
                 last_copied_at) VALUES (?1, NULL, ?1, ?2, 1, 0, 0)",
                rusqlite::params![secret, [0u8; 32]],
            )
            .unwrap_err();
        let se = StorageError::from(e);
        assert!(matches!(se, StorageError::Sqlite(_)));
        for text in [se.to_string(), format!("{se:?}")] {
            assert!(!text.contains(secret), "{text}");
            assert!(!text.contains("clips.kind"), "no SQLite message: {text}");
        }
        // a SQLite message that echoes user text is dropped
        let echoed = rusqlite::Error::SqliteFailure(
            rusqlite::ffi::Error::new(rusqlite::ffi::SQLITE_CONSTRAINT),
            Some(format!("constraint failed: {secret}")),
        );
        let se = StorageError::from(echoed);
        assert!(!se.to_string().contains(secret));
        assert!(!format!("{se:?}").contains(secret));
        // a conversion error that would echo the value is dropped as well
        let conv = rusqlite::Error::FromSqlConversionFailure(
            0,
            rusqlite::types::Type::Text,
            Box::new(std::io::Error::other(secret)),
        );
        let se = StorageError::from(conv);
        assert!(!se.to_string().contains(secret));
        assert!(!format!("{se:?}").contains(secret));
        // io errors keep the kind only
        let io = std::io::Error::new(std::io::ErrorKind::NotFound, secret);
        assert!(!StorageError::from(io).to_string().contains(secret));
        // NewClip Debug carries no content either
        let dbg = format!("{:?}", text_clip(secret));
        assert!(!dbg.contains(secret), "{dbg}");
    }

    #[test]
    fn sweep_failure_does_not_fail_open() {
        let tmp = tempfile::tempdir().unwrap();
        drop(open(tmp.path()));
        let blobs = tmp.path().join("blobs");
        // An orphan that cannot be removed: on Windows the file is held open without
        // FILE_SHARE_DELETE (std deletes read-only files there, so the read-only
        // attribute is not enough); on Unix its shard directory is read-only.
        let stuck_dir = blobs.join("aa");
        std::fs::create_dir_all(&stuck_dir).unwrap();
        let stuck = stuck_dir.join("aa".repeat(32));
        std::fs::write(&stuck, b"orphan").unwrap();
        #[cfg(windows)]
        let lock = {
            use std::os::windows::fs::OpenOptionsExt;
            std::fs::OpenOptions::new()
                .read(true)
                .share_mode(0)
                .open(&stuck)
                .unwrap()
        };
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&stuck_dir, std::fs::Permissions::from_mode(0o500)).unwrap();
        }
        // a removable orphan in another shard is still swept
        let free_dir = blobs.join("bb");
        std::fs::create_dir_all(&free_dir).unwrap();
        let free = free_dir.join("bb".repeat(32));
        std::fs::write(&free, b"orphan").unwrap();

        let s = Storage::open(tmp.path(), OpenMode::ReadWrite, clock());
        assert!(s.is_ok(), "open must not fail because of the sweep");
        assert!(!free.exists(), "the removable orphan is swept");
        let mut s = s.unwrap();
        let again = blobs::sweep(&mut s.conn, tmp.path());
        #[cfg(windows)]
        assert_eq!(
            again,
            blobs::Cleanup {
                removed: 0,
                failed: 1
            }
        );
        let _ = again;
        drop(s);
        #[cfg(windows)]
        {
            assert!(stuck.exists());
            drop(lock);
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&stuck_dir, std::fs::Permissions::from_mode(0o700)).unwrap();
        }
        // the next open retries and succeeds
        drop(open(tmp.path()));
        assert!(!stuck.exists());
    }

    #[test]
    fn stale_backup_removal_failure_is_ignored() {
        let tmp = tempfile::tempdir().unwrap();
        let bak = tmp.path().join("history.db.pre-v2.bak");
        {
            let c = rusqlite::Connection::open(tmp.path().join("history.db")).unwrap();
            c.execute_batch("CREATE TABLE legacy(x);").unwrap();
        }
        drop(open(tmp.path()));
        assert!(bak.is_file());
        // Make the removal fail on every OS: a non-empty directory at the backup path.
        std::fs::remove_file(&bak).unwrap();
        std::fs::create_dir(&bak).unwrap();
        std::fs::write(bak.join("x"), b"x").unwrap();
        let s = Storage::open(tmp.path(), OpenMode::ReadWrite, clock());
        assert!(s.is_ok(), "open must not fail on a stuck backup");
        drop(s);
        assert!(bak.exists());
        // once removable again, the next open deletes it
        std::fs::remove_dir_all(&bak).unwrap();
        std::fs::write(&bak, b"stale").unwrap();
        drop(open(tmp.path()));
        assert!(!bak.exists());
    }

    #[test]
    fn corrupted_blob_is_repaired_on_rewrite() {
        let tmp = tempfile::tempdir().unwrap();
        let mut s = open(tmp.path());
        let text = "r".repeat(70 * 1024);
        let UpsertOutcome::Inserted(id) = s.upsert_clip(&text_clip(&text)).unwrap() else {
            panic!("expected Inserted");
        };
        let files = files_under(&tmp.path().join("blobs"));
        assert_eq!(files.len(), 1);
        // same size, different bytes
        std::fs::write(&files[0], "s".repeat(70 * 1024)).unwrap();
        assert_eq!(
            s.read_representation(&id, 0),
            Err(StorageError::Io(std::io::ErrorKind::InvalidData))
        );
        assert_eq!(
            s.upsert_clip(&text_clip(&text)).unwrap(),
            UpsertOutcome::Bumped(id)
        );
        assert_eq!(s.read_representation(&id, 0).unwrap(), text.as_bytes());
        assert_eq!(files_under(&tmp.path().join("blobs")).len(), 1);
    }

    #[test]
    fn deleting_clip_removes_fts_row() {
        let tmp = tempfile::tempdir().unwrap();
        let mut s = open(tmp.path());
        let UpsertOutcome::Inserted(id) = s.upsert_clip(&text_clip("findable words")).unwrap()
        else {
            panic!("expected Inserted");
        };
        let rowid: i64 = s
            .conn
            .query_row(
                "SELECT rowid FROM clip_rowid WHERE clip_id = ?1",
                [id.to_string()],
                |r| r.get(0),
            )
            .unwrap();
        s.conn
            .execute(
                "INSERT INTO clips_fts (rowid, preview, body, note, tags) \
                 VALUES (?1, 'findable words', 'findable words', '', '')",
                [rowid],
            )
            .unwrap();
        let hits = "SELECT count(*) FROM clips_fts WHERE clips_fts MATCH 'findable'";
        assert_eq!(count(&s, hits), 1);
        s.conn
            .execute("DELETE FROM clips WHERE id = ?1", [id.to_string()])
            .unwrap();
        assert_eq!(count(&s, "SELECT count(*) FROM clip_rowid"), 0);
        assert_eq!(count(&s, hits), 0, "cascade fired the FTS delete trigger");
    }
}
