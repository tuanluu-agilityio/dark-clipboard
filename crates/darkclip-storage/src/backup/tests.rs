//! Task 405 tests: backup and restore.

use crate::archive::ArchiveLimits;
use crate::blobs;
use crate::retention::test_support::*;
use crate::{ArchiveError, SearchQuery, Storage, StorageError};
use darkclip_core::{
    ClipId, CollectionSort, Mime, SensitiveCategory, Sensitivity, SensitivityLevel, Timestamp,
};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

const SECRET_TAIL: &str = "Zq9Xw8Vu7Ts6Rp5On4Ml3Kj2";
/// Free disk space the tests report: plenty.
const FREE: Option<u64> = Some(u64::MAX);

fn secret() -> String {
    // Runtime-assembled (GC-09).
    format!("{}{}", concat!("s", "k-", "live-"), SECRET_TAIL)
}

struct Fixture {
    text: ClipId,
    image: ClipId,
    big: ClipId,
    masked: ClipId,
    png: Vec<u8>,
    thumb: Vec<u8>,
}

/// Text (note, tags, favorite, collection), an image with a thumbnail (blobs), a large text
/// (blob) and a masked clip.
fn fixture(s: &mut Storage) -> Fixture {
    let text = ins(s, text_clip("alpha bravo quokka"));
    let png = png_bytes(7);
    let thumb = png_bytes(8);
    let mut img = raw(Mime::ImagePng, &png);
    img.width = Some(1);
    img.height = Some(1);
    let mut n = new_clip(vec![img]);
    n.thumbnail_png = Some(thumb.clone());
    let image = ins(s, n);
    let big = ins(s, text_clip(&"w".repeat(70 * 1024)));
    let mut m = text_clip(&secret());
    m.classified.preview = "sk-\u{2026}\u{2022}\u{2022}\u{2022}\u{2022}".into();
    m.sensitivity = Sensitivity {
        level: SensitivityLevel::Masked,
        categories: vec![SensitiveCategory::ApiKey],
    };
    m.expires_at = Some(Timestamp(T0 + 3_600_000));
    let masked = ins(s, m);
    let col = s
        .create_collection("Work", CollectionSort::Manual, None)
        .unwrap();
    s.add_to_collection(&col.id, &text).unwrap();
    s.add_to_collection(&col.id, &image).unwrap();
    let fav = s.favorites_id().unwrap();
    s.add_to_collection(&fav, &text).unwrap();
    s.set_clip_tags(&text, &["rust", "đồ"]).unwrap();
    s.set_note(&text, Some("remember the quokka")).unwrap();
    Fixture {
        text,
        image,
        big,
        masked,
        png,
        thumb,
    }
}

fn entries_of(path: &Path) -> Vec<(String, Vec<u8>)> {
    let mut z = zip::ZipArchive::new(std::fs::File::open(path).unwrap()).unwrap();
    let mut out = Vec::new();
    for i in 0..z.len() {
        let mut f = z.by_index(i).unwrap();
        let mut b = Vec::new();
        f.read_to_end(&mut b).unwrap();
        out.push((f.name().unwrap().to_string(), b));
    }
    out
}

fn write_zip(path: &Path, entries: &[(String, Vec<u8>)]) {
    let mut z = zip::ZipWriter::new(std::fs::File::create(path).unwrap());
    for (name, bytes) in entries {
        z.start_file(name.as_str(), zip::write::SimpleFileOptions::default())
            .unwrap();
        z.write_all(bytes).unwrap();
    }
    z.finish().unwrap();
}

fn hex(b: &[u8]) -> String {
    blobs::hex(&blobs::sha256(b))
}

/// A manifest naming every entry except itself, with the fields given.
fn manifest(entries: &[(String, Vec<u8>)], format: u32, schema: u32) -> Vec<u8> {
    let files: Vec<serde_json::Value> = entries
        .iter()
        .filter(|(n, _)| n != "manifest.json")
        .map(|(n, b)| serde_json::json!({ "path": n, "size": b.len(), "sha256": hex(b) }))
        .collect();
    serde_json::to_vec(&serde_json::json!({
        "format": format,
        "schema_version": schema,
        "app_version": "0.0.1",
        "created_at": "2026-11-22T00:00:00.000Z",
        "clip_count": 3,
        "files": files,
    }))
    .unwrap()
}

/// Rewrites a real backup: `edit` may change entries; the manifest is rebuilt when `remanifest`.
fn rebuild(
    src: &Path,
    dst: &Path,
    remanifest: bool,
    edit: impl FnOnce(&mut Vec<(String, Vec<u8>)>),
) {
    let mut e = entries_of(src);
    edit(&mut e);
    if remanifest {
        e.retain(|(n, _)| n != "manifest.json");
        // The schema version the database itself carries (header bytes 60..64).
        let schema = e
            .iter()
            .find(|(n, _)| n == "history.db")
            .and_then(|(_, b)| b.get(60..64))
            .map(|v| u32::from_be_bytes([v[0], v[1], v[2], v[3]]))
            // Not a real header (a garbage database in a test): the current version.
            .filter(|v| *v < 1000)
            .unwrap_or(3);
        let m = manifest(&e, 1, schema);
        e.push(("manifest.json".into(), m));
    }
    write_zip(dst, &e);
}

/// A store with one clip, to restore into.
fn live(dir: &Path) -> (Storage, ClipId) {
    let mut s = open(dir, clock());
    let id = ins(&mut s, text_clip("live data stays"));
    (s, id)
}

fn assert_live_unchanged(s: &Storage, id: &ClipId) {
    assert_eq!(count(s, "SELECT count(*) FROM clips"), 1);
    assert_eq!(s.read_representation(id, 0).unwrap(), b"live data stays");
}

fn names_in(dir: &Path) -> Vec<String> {
    std::fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect()
}

fn pre_restore_files(dir: &Path) -> Vec<String> {
    names_in(dir)
        .into_iter()
        .filter(|n| n.starts_with("history.db.pre-restore-"))
        .collect()
}

/// No staging directory is left behind in the data folder.
fn assert_no_staging(dir: &Path) {
    let left: Vec<String> = names_in(dir)
        .into_iter()
        .filter(|n| n.contains("staging"))
        .collect();
    assert!(left.is_empty(), "{left:?}");
}

/// A backup of the fixture store; returns (temp dirs kept alive, path, fixture).
fn make_backup() -> (tempfile::TempDir, PathBuf, Fixture) {
    let tmp = tempfile::tempdir().unwrap();
    let src = tmp.path().join("src");
    let mut s = open(&src, clock());
    let f = fixture(&mut s);
    let out = tmp.path().join("history.dcbackup");
    let r = s.write_backup(&out, "0.0.1").unwrap();
    assert_eq!(r.clips, 3, "the masked clip is not backed up");
    (tmp, out, f)
}

fn query(text: &str) -> SearchQuery {
    SearchQuery {
        text: Some(text.into()),
        kinds: Vec::new(),
        pinned_only: false,
        since: None,
        include_masked: true,
        limit: 50,
        cursor: None,
        collection: None,
        tag: None,
    }
}

#[test]
fn backup_restore_round_trip_with_image_collections_and_tags() {
    let tmp = tempfile::tempdir().unwrap();
    let src = tmp.path().join("src");
    let mut a = open(&src, clock());
    let f = fixture(&mut a);
    let out = tmp.path().join("history.dcbackup");
    assert_eq!(a.write_backup(&out, "0.0.1").unwrap().clips, 3);
    let dst = tmp.path().join("dst");
    let (mut b, old) = live(&dst);
    let r = b.restore_backup(&out, FREE).unwrap();
    assert_eq!(r.clips, 3);

    assert_eq!(count(&b, "SELECT count(*) FROM clips"), 3);
    assert_eq!(
        b.get_clip(&old).unwrap(),
        None,
        "the old history was replaced"
    );
    assert_eq!(
        b.get_clip(&f.masked).unwrap(),
        None,
        "masked clips are not restored"
    );
    for id in [f.text, f.image, f.big] {
        assert_eq!(b.get_clip(&id).unwrap(), a.get_clip(&id).unwrap());
    }
    let text = b.get_clip(&f.text).unwrap().unwrap();
    assert_eq!(text.note.as_deref(), Some("remember the quokka"));
    let mut tags: Vec<String> = text.tags.iter().map(|t| t.name.clone()).collect();
    tags.sort();
    assert_eq!(tags, ["rust", "đồ"]);
    assert_eq!(text.collections.len(), 2, "Work and Favorites");
    assert_eq!(b.read_representation(&f.image, 0).unwrap(), f.png);
    assert_eq!(b.read_thumbnail(&f.image).unwrap(), Some(f.thumb.clone()));
    assert_eq!(
        b.read_representation(&f.big, 0).unwrap(),
        "w".repeat(70 * 1024).as_bytes()
    );
    let work = b
        .list_collections()
        .unwrap()
        .into_iter()
        .find(|c| c.name == "Work")
        .unwrap();
    assert_eq!(b.collection_clip_ids(&work.id).unwrap(), [f.text, f.image]);
    // The search index came with the data: words, notes and tags are found.
    for word in ["quokka", "remember", "rust"] {
        let (rows, _) = b.search(&query(word)).unwrap();
        assert_eq!(rows.len(), 1, "{word}");
    }

    // The previous data is kept until the next successful start, then removed.
    let pre = pre_restore_files(&dst);
    assert_eq!(pre.len(), 1, "{pre:?}");
    {
        let c = rusqlite::Connection::open(dst.join(&pre[0])).unwrap();
        let n: i64 = c
            .query_row("SELECT count(*) FROM clips", [], |r| r.get(0))
            .unwrap();
        assert_eq!(n, 1);
    }
    assert_no_staging(&dst);
    drop(b);
    let b = open(&dst, clock());
    assert!(pre_restore_files(&dst).is_empty());
    assert_eq!(count(&b, "SELECT count(*) FROM clips"), 3);
    assert_eq!(b.read_representation(&f.image, 0).unwrap(), f.png);
}

#[test]
fn backup_lists_every_file_and_leaves_no_partial_file() {
    let (tmp, out, f) = make_backup();
    assert!(!tmp
        .path()
        .join(format!(
            "history.dcbackup{}",
            crate::archive::PARTIAL_SUFFIX
        ))
        .exists());
    let entries = entries_of(&out);
    let names: Vec<&str> = entries.iter().map(|(n, _)| n.as_str()).collect();
    assert!(names.contains(&"manifest.json"));
    assert!(names.contains(&"history.db"));
    // image, thumbnail and the large text
    assert_eq!(names.iter().filter(|n| n.starts_with("blobs/")).count(), 3);
    let m: serde_json::Value = serde_json::from_slice(
        &entries
            .iter()
            .find(|(n, _)| n == "manifest.json")
            .unwrap()
            .1,
    )
    .unwrap();
    assert_eq!(m["format"], 1);
    assert_eq!(m["schema_version"], 3);
    assert_eq!(m["app_version"], "0.0.1");
    assert_eq!(m["clip_count"], 3);
    assert!(m["created_at"].as_str().unwrap().ends_with('Z'));
    let files = m["files"].as_array().unwrap();
    assert_eq!(files.len(), entries.len() - 1);
    for (name, bytes) in entries.iter().filter(|(n, _)| n != "manifest.json") {
        let e = files.iter().find(|e| e["path"] == name.as_str()).unwrap();
        assert_eq!(e["size"], bytes.len());
        assert_eq!(e["sha256"], hex(bytes));
    }
    // The masked clip's content is nowhere in the archive (GC-06, privacy-design §4).
    for (_, bytes) in &entries {
        let hay = String::from_utf8_lossy(bytes);
        assert!(!hay.contains(SECRET_TAIL));
    }
    let _ = f;
}

#[test]
fn zip_slip_and_unknown_entries_are_refused_and_nothing_is_written() {
    let (tmp, out, _) = make_backup();
    let dst = tmp.path().join("dst");
    let (mut b, id) = live(&dst);
    let sha = "ab".repeat(32);
    for bad in [
        "../evil.db".to_string(),
        format!("blobs/../../evil{sha}"),
        format!("blobs/ab/../../../evil{sha}"),
        "/tmp/evil".to_string(),
        "C:/evil".to_string(),
        "C:\\evil".to_string(),
        format!("blobs\\ab\\{sha}"),
        "sub/history.db".to_string(),
        "export.json".to_string(),
        "blobs/".to_string(),
    ] {
        let crafted = tmp.path().join("crafted.dcbackup");
        rebuild(&out, &crafted, true, |e| {
            e.push((bad.clone(), b"payload".to_vec()));
        });
        assert_eq!(
            b.restore_backup(&crafted, FREE),
            Err(StorageError::Archive(ArchiveError::BadEntryName)),
            "{bad}"
        );
        assert_live_unchanged(&b, &id);
        assert_no_staging(&dst);
        assert!(pre_restore_files(&dst).is_empty());
    }
    for d in [tmp.path(), &dst, tmp.path().parent().unwrap()] {
        assert!(
            !names_in(d).iter().any(|n| n.contains("evil")),
            "nothing was written outside"
        );
    }
}

#[test]
fn too_many_or_too_large_entries_are_refused() {
    let (tmp, out, _) = make_backup();
    let dst = tmp.path().join("dst");
    let (mut b, id) = live(&dst);
    let n = entries_of(&out).len();
    let db_len = entries_of(&out)
        .into_iter()
        .find(|(n, _)| n == "history.db")
        .unwrap()
        .1
        .len() as u64;
    let base = ArchiveLimits::BACKUP;
    let cases = [
        (
            ArchiveLimits {
                max_entries: n - 1,
                ..base
            },
            ArchiveError::TooManyEntries,
        ),
        (
            ArchiveLimits {
                max_entry_bytes: db_len - 1,
                ..base
            },
            ArchiveError::TooLarge,
        ),
        (
            ArchiveLimits {
                max_total_bytes: db_len,
                ..base
            },
            ArchiveError::TooLarge,
        ),
        (
            ArchiveLimits {
                max_json_bytes: 16,
                ..base
            },
            ArchiveError::TooLarge,
        ),
    ];
    for (limits, want) in cases {
        assert_eq!(
            b.restore_backup_with(&out, limits, FREE),
            Err(StorageError::Archive(want)),
            "{limits:?}"
        );
        assert_live_unchanged(&b, &id);
        assert_no_staging(&dst);
    }
    // A highly compressible entry that expands past the per-entry limit (a zip bomb).
    let bomb = tmp.path().join("bomb.dcbackup");
    rebuild(&out, &bomb, true, |e| {
        e.push((format!("blobs/00/00{}", "0".repeat(62)), vec![0u8; 4 << 20]));
    });
    let small = ArchiveLimits {
        max_entry_bytes: 1 << 20,
        ..base
    };
    assert_eq!(
        b.restore_backup_with(&bomb, small, FREE),
        Err(StorageError::Archive(ArchiveError::TooLarge))
    );
    assert_live_unchanged(&b, &id);
    // Not a zip at all.
    let junk = tmp.path().join("junk.dcbackup");
    std::fs::write(&junk, b"definitely not a zip").unwrap();
    assert_eq!(
        b.restore_backup(&junk, FREE),
        Err(StorageError::Archive(ArchiveError::NotAnArchive))
    );
    assert_live_unchanged(&b, &id);
}

#[test]
fn checksum_or_manifest_mismatch_is_refused() {
    let (tmp, out, _) = make_backup();
    let dst = tmp.path().join("dst");
    let (mut b, id) = live(&dst);
    let crafted = tmp.path().join("crafted.dcbackup");

    // One blob byte flipped, manifest unchanged.
    rebuild(&out, &crafted, false, |e| {
        let blob = e.iter_mut().find(|(n, _)| n.starts_with("blobs/")).unwrap();
        blob.1[0] ^= 0xff;
    });
    assert_eq!(
        b.restore_backup(&crafted, FREE),
        Err(StorageError::Archive(ArchiveError::ChecksumMismatch))
    );
    assert_live_unchanged(&b, &id);

    // The database changed, manifest unchanged.
    rebuild(&out, &crafted, false, |e| {
        let db = e.iter_mut().find(|(n, _)| n == "history.db").unwrap();
        let last = db.1.len() - 1;
        db.1[last] ^= 0x01;
    });
    assert_eq!(
        b.restore_backup(&crafted, FREE),
        Err(StorageError::Archive(ArchiveError::ChecksumMismatch))
    );
    assert_live_unchanged(&b, &id);

    // A file the manifest does not name, and a named file that is missing.
    rebuild(&out, &crafted, false, |e| {
        e.push((format!("blobs/cd/cd{}", "1".repeat(62)), b"extra".to_vec()));
    });
    assert_eq!(
        b.restore_backup(&crafted, FREE),
        Err(StorageError::Archive(ArchiveError::BadManifest))
    );
    rebuild(&out, &crafted, false, |e| {
        e.retain(|(n, _)| !n.starts_with("blobs/"));
    });
    assert_eq!(
        b.restore_backup(&crafted, FREE),
        Err(StorageError::Archive(ArchiveError::BadManifest))
    );
    // No manifest, no database, a manifest that is not JSON, an unknown manifest field.
    rebuild(&out, &crafted, false, |e| {
        e.retain(|(n, _)| n != "manifest.json")
    });
    assert_eq!(
        b.restore_backup(&crafted, FREE),
        Err(StorageError::Archive(ArchiveError::NotAnArchive))
    );
    rebuild(&out, &crafted, true, |e| {
        e.retain(|(n, _)| n != "history.db")
    });
    assert_eq!(
        b.restore_backup(&crafted, FREE),
        Err(StorageError::Archive(ArchiveError::BadManifest))
    );
    rebuild(&out, &crafted, false, |e| {
        let m = e.iter_mut().find(|(n, _)| n == "manifest.json").unwrap();
        m.1 = b"{ not json".to_vec();
    });
    assert_eq!(
        b.restore_backup(&crafted, FREE),
        Err(StorageError::Archive(ArchiveError::BadManifest))
    );
    rebuild(&out, &crafted, false, |e| {
        let m = e.iter_mut().find(|(n, _)| n == "manifest.json").unwrap();
        let mut v: serde_json::Value = serde_json::from_slice(&m.1).unwrap();
        v["surprise"] = serde_json::json!(true);
        m.1 = serde_json::to_vec(&v).unwrap();
    });
    assert_eq!(
        b.restore_backup(&crafted, FREE),
        Err(StorageError::Archive(ArchiveError::BadManifest))
    );
    assert_live_unchanged(&b, &id);
    assert_no_staging(&dst);
}

#[test]
fn newer_backup_is_refused_and_live_data_is_unchanged() {
    let (tmp, out, _) = make_backup();
    let dst = tmp.path().join("dst");
    let (mut b, id) = live(&dst);
    let crafted = tmp.path().join("crafted.dcbackup");
    let with_manifest = |format: u32, schema: u32| {
        rebuild(&out, &crafted, false, |e| {
            e.retain(|(n, _)| n != "manifest.json");
            let m = manifest(e, format, schema);
            e.push(("manifest.json".into(), m));
        });
    };
    with_manifest(2, 2);
    assert_eq!(
        b.restore_backup(&crafted, FREE),
        Err(StorageError::Archive(ArchiveError::NewerFormat))
    );
    assert_live_unchanged(&b, &id);
    with_manifest(1, 4);
    assert_eq!(
        b.restore_backup(&crafted, FREE),
        Err(StorageError::SchemaTooNew {
            found: 4,
            supported: 3
        })
    );
    assert_live_unchanged(&b, &id);
    // The manifest claims version 2 but the database itself is newer.
    rebuild(&out, &crafted, true, |e| {
        let db = e.iter_mut().find(|(n, _)| n == "history.db").unwrap();
        let tmpdb = tmp.path().join("newer.db");
        std::fs::write(&tmpdb, &db.1).unwrap();
        {
            let c = rusqlite::Connection::open(&tmpdb).unwrap();
            c.pragma_update(None, "user_version", 99).unwrap();
        }
        db.1 = std::fs::read(&tmpdb).unwrap();
    });
    assert_eq!(
        b.restore_backup(&crafted, FREE),
        Err(StorageError::SchemaTooNew {
            found: 99,
            supported: 3
        })
    );
    assert_live_unchanged(&b, &id);
    assert_no_staging(&dst);
    assert!(pre_restore_files(&dst).is_empty());
}

#[test]
fn an_older_backup_is_migrated_on_restore() {
    let (tmp, out, _) = make_backup();
    let dst = tmp.path().join("dst");
    let (mut b, _) = live(&dst);
    // A genuine version-1 database (migration 1 only) with one clip.
    let v1 = tmp.path().join("v1.db");
    let id = "01900000-0000-7000-8000-000000000001";
    {
        let mut c = rusqlite::Connection::open(&v1).unwrap();
        crate::migrate::run(&mut c, &crate::migrate::MIGRATIONS[..1]).unwrap();
        c.execute(
            "INSERT INTO clips (id, kind, preview, content_hash, size_bytes, created_at, \
             last_copied_at) VALUES (?1, 'text', 'old words', ?2, 9, ?3, ?3)",
            rusqlite::params![id, [7u8; 32], T0],
        )
        .unwrap();
        c.execute_batch(
            "INSERT INTO representations (clip_id, ord, mime, size_bytes, inline_data) \
             SELECT id, 0, 'text/plain;charset=utf-8', 9, CAST('old words' AS BLOB) FROM clips; \
             INSERT INTO clip_rowid (clip_id) SELECT id FROM clips;",
        )
        .unwrap();
    }
    let crafted = tmp.path().join("v1.dcbackup");
    rebuild(&out, &crafted, true, |e| {
        e.retain(|(n, _)| !n.starts_with("blobs/"));
        let db = e.iter_mut().find(|(n, _)| n == "history.db").unwrap();
        db.1 = std::fs::read(&v1).unwrap();
    });
    let r = b.restore_backup(&crafted, FREE).unwrap();
    assert_eq!(r.clips, 1);
    assert_eq!(b.schema_version().unwrap(), 3);
    assert!(b.favorites_id().is_ok());
    let cid = ClipId::parse(id).unwrap();
    assert_eq!(b.read_representation(&cid, 0).unwrap(), b"old words");
    assert!(!b.is_no_export(&cid).unwrap(), "older clips carry no flag");
}

#[test]
fn failure_during_the_swap_leaves_the_live_data() {
    let (tmp, out, _) = make_backup();
    let dst = tmp.path().join("dst");
    let (mut b, id) = live(&dst);
    b.fail_restore_swap = true;
    // The fault fires after the first pages were copied into the live database.
    assert_eq!(
        b.restore_backup(&out, FREE),
        Err(StorageError::Io(std::io::ErrorKind::Interrupted))
    );
    assert_live_unchanged(&b, &id);
    assert_no_staging(&dst);
    assert_eq!(
        b.favorites_id().unwrap().to_string(),
        darkclip_core::FAVORITES_COLLECTION_ID
    );
    drop(b);
    // The store still opens and holds the old data.
    let b = open(&dst, clock());
    assert_live_unchanged(&b, &id);
    // Without the fault the same file restores.
    let mut b = b;
    assert_eq!(b.restore_backup(&out, FREE).unwrap().clips, 3);
}

#[test]
fn a_crafted_database_is_refused() {
    let (tmp, out, _) = make_backup();
    let dst = tmp.path().join("dst");
    let (mut b, id) = live(&dst);
    let crafted = tmp.path().join("crafted.dcbackup");
    let edit_db = |sql: &'static str| {
        let tmpdb = tmp.path().join("edit.db");
        rebuild(&out, &crafted, true, |e| {
            let db = e.iter_mut().find(|(n, _)| n == "history.db").unwrap();
            std::fs::write(&tmpdb, &db.1).unwrap();
            {
                let c = rusqlite::Connection::open(&tmpdb).unwrap();
                c.execute_batch(sql).unwrap();
            }
            db.1 = std::fs::read(&tmpdb).unwrap();
            let _ = std::fs::remove_file(&tmpdb);
        });
    };
    // A trigger would run on every later capture: never accepted.
    edit_db(
        "CREATE TRIGGER keep_copies AFTER DELETE ON clips \
         BEGIN INSERT INTO clips_shadow VALUES (OLD.preview); END; \
         CREATE TABLE clips_shadow (p TEXT);",
    );
    assert_eq!(
        b.restore_backup(&crafted, FREE),
        Err(StorageError::Archive(ArchiveError::Corrupt))
    );
    assert_live_unchanged(&b, &id);
    // A row that does not decode as a clip.
    edit_db("UPDATE clips SET kind = 'no-such-kind';");
    assert_eq!(
        b.restore_backup(&crafted, FREE),
        Err(StorageError::Archive(ArchiveError::Corrupt))
    );
    // Favorites removed (the system row is required).
    edit_db(
        "DROP TRIGGER collections_system_no_delete; DELETE FROM collections; \
         CREATE TRIGGER collections_system_no_delete BEFORE DELETE ON collections \
         WHEN OLD.kind = 'system' BEGIN SELECT RAISE(ABORT, 'system collection'); END;",
    );
    assert_eq!(
        b.restore_backup(&crafted, FREE),
        Err(StorageError::Archive(ArchiveError::Corrupt))
    );
    // Not a database at all, with a matching checksum.
    rebuild(&out, &crafted, true, |e| {
        let db = e.iter_mut().find(|(n, _)| n == "history.db").unwrap();
        db.1 = b"SQLite format 3\0 but not really".repeat(200);
    });
    assert_eq!(
        b.restore_backup(&crafted, FREE),
        Err(StorageError::Archive(ArchiveError::Corrupt))
    );
    assert_live_unchanged(&b, &id);
    assert_no_staging(&dst);
}

#[test]
fn errors_name_no_content_or_path() {
    let (tmp, out, _) = make_backup();
    let dst = tmp.path().join("dst");
    let (mut b, _) = live(&dst);
    let crafted = tmp.path().join(format!("{SECRET_TAIL}.dcbackup"));
    rebuild(&out, &crafted, true, |e| {
        e.push((format!("../{SECRET_TAIL}"), b"x".to_vec()));
    });
    let e = b.restore_backup(&crafted, FREE).unwrap_err();
    for text in [e.to_string(), format!("{e:?}")] {
        assert!(!text.contains(SECRET_TAIL), "{text}");
    }
    let missing = tmp.path().join(format!("{SECRET_TAIL}-missing.dcbackup"));
    let e = b.restore_backup(&missing, FREE).unwrap_err();
    assert!(!format!("{e:?}").contains(SECRET_TAIL));
}

// ---- Task 405 security review ----

/// Rewrites the database inside a backup with `sql` run on it (checksums fixed).
fn with_db_edit(out: &Path, crafted: &Path, scratch: &Path, sql: &str) {
    rebuild(out, crafted, true, |e| {
        let db = e.iter_mut().find(|(n, _)| n == "history.db").unwrap();
        std::fs::write(scratch, &db.1).unwrap();
        {
            let c = rusqlite::Connection::open(scratch).unwrap();
            c.execute_batch(sql).unwrap();
        }
        db.1 = std::fs::read(scratch).unwrap();
        let _ = std::fs::remove_file(scratch);
    });
}

fn pragma_i64(s: &Storage, name: &str) -> i64 {
    s.conn.pragma_query_value(None, name, |r| r.get(0)).unwrap()
}

fn pragma_text(s: &Storage, name: &str) -> String {
    s.conn.pragma_query_value(None, name, |r| r.get(0)).unwrap()
}

#[test]
fn a_utf16_database_is_refused_and_the_live_data_still_works() {
    let (tmp, out, _) = make_backup();
    let dst = tmp.path().join("dst");
    let (mut b, id) = live(&dst);
    // A database with the store's schema, but in UTF-16.
    let u16db = tmp.path().join("u16.db");
    {
        let mut c = rusqlite::Connection::open(&u16db).unwrap();
        c.execute_batch("PRAGMA encoding = 'UTF-16le';").unwrap();
        crate::migrate::run(&mut c, crate::migrate::MIGRATIONS).unwrap();
        let enc: String = c.query_row("PRAGMA encoding", [], |r| r.get(0)).unwrap();
        assert_eq!(enc, "UTF-16le");
    }
    let crafted = tmp.path().join("u16.dcbackup");
    rebuild(&out, &crafted, true, |e| {
        e.retain(|(n, _)| !n.starts_with("blobs/"));
        let db = e.iter_mut().find(|(n, _)| n == "history.db").unwrap();
        db.1 = std::fs::read(&u16db).unwrap();
    });
    assert_eq!(
        b.restore_backup(&crafted, FREE),
        Err(StorageError::Archive(ArchiveError::Corrupt))
    );
    assert_live_unchanged(&b, &id);
    assert_eq!(pragma_text(&b, "encoding"), "UTF-8");
    assert_no_staging(&dst);
}

#[test]
fn header_fields_of_a_backup_never_reach_the_live_database() {
    let (tmp, out, f) = make_backup();
    let dst = tmp.path().join("dst");
    let (mut b, _) = live(&dst);
    let before_av = pragma_i64(&b, "auto_vacuum");
    let crafted = tmp.path().join("hdr.dcbackup");
    with_db_edit(
        &out,
        &crafted,
        &tmp.path().join("hdr.db"),
        "PRAGMA auto_vacuum = FULL; VACUUM; PRAGMA application_id = 1234;",
    );
    assert_eq!(b.restore_backup(&crafted, FREE).unwrap().clips, 3);
    assert_eq!(pragma_i64(&b, "auto_vacuum"), before_av);
    assert_eq!(pragma_i64(&b, "application_id"), 0);
    assert_eq!(pragma_i64(&b, "user_version"), 3);
    assert_eq!(pragma_text(&b, "journal_mode").to_lowercase(), "wal");
    assert_eq!(pragma_text(&b, "encoding"), "UTF-8");
    assert!(b.get_clip(&f.text).unwrap().is_some());
    drop(b);
    // The store opens again normally and keeps WAL.
    let b = open(&dst, clock());
    assert_eq!(pragma_text(&b, "journal_mode").to_lowercase(), "wal");
    assert_eq!(count(&b, "SELECT count(*) FROM clips"), 3);
}

#[test]
fn a_different_page_size_is_converted() {
    let (tmp, out, f) = make_backup();
    let dst = tmp.path().join("dst");
    let (mut b, _) = live(&dst);
    let live_page = pragma_i64(&b, "page_size");
    let crafted = tmp.path().join("page.dcbackup");
    let other = if live_page == 8192 { 16384 } else { 8192 };
    with_db_edit(
        &out,
        &crafted,
        &tmp.path().join("page.db"),
        &format!("PRAGMA page_size = {other}; VACUUM;"),
    );
    assert_eq!(b.restore_backup(&crafted, FREE).unwrap().clips, 3);
    assert_eq!(pragma_i64(&b, "page_size"), live_page);
    assert_eq!(b.read_representation(&f.image, 0).unwrap(), f.png);
    assert_eq!(pragma_text(&b, "journal_mode").to_lowercase(), "wal");
}

#[test]
fn an_object_outside_the_known_schemas_is_refused_before_any_migration() {
    let (tmp, out, _) = make_backup();
    let dst = tmp.path().join("dst");
    let (mut b, id) = live(&dst);
    let crafted = tmp.path().join("view.dcbackup");
    with_db_edit(
        &out,
        &crafted,
        &tmp.path().join("view.db"),
        "CREATE VIEW peek AS SELECT preview FROM clips;",
    );
    assert_eq!(
        b.restore_backup(&crafted, FREE),
        Err(StorageError::Archive(ArchiveError::Corrupt))
    );
    assert_live_unchanged(&b, &id);
    // A manifest whose schema version is not the database's own.
    rebuild(&out, &crafted, false, |e| {
        e.retain(|(n, _)| n != "manifest.json");
        let m = manifest(e, 1, 1);
        e.push(("manifest.json".into(), m));
    });
    assert_eq!(
        b.restore_backup(&crafted, FREE),
        Err(StorageError::Archive(ArchiveError::BadManifest))
    );
    assert_live_unchanged(&b, &id);
}

#[test]
fn duplicate_entry_names_are_refused() {
    let (tmp, out, _) = make_backup();
    let dst = tmp.path().join("dst");
    let (mut b, id) = live(&dst);
    // Two entries, then the second name is patched to equal the first in both headers.
    let mut e = entries_of(&out);
    let name = e
        .iter()
        .find(|(n, _)| n.starts_with("blobs/"))
        .unwrap()
        .0
        .clone();
    let mut twin = name.clone().into_bytes();
    let last = twin.len() - 1;
    twin[last] = if twin[last] == b'0' { b'1' } else { b'0' };
    let twin = String::from_utf8(twin).unwrap();
    e.push((twin.clone(), b"other bytes".to_vec()));
    let crafted = tmp.path().join("dup.dcbackup");
    write_zip(&crafted, &e);
    let mut bytes = std::fs::read(&crafted).unwrap();
    let (from, to) = (twin.as_bytes(), name.as_bytes());
    let mut i = 0;
    let mut patched = 0;
    while i + from.len() <= bytes.len() {
        if &bytes[i..i + from.len()] == from {
            bytes[i..i + from.len()].copy_from_slice(to);
            patched += 1;
        }
        i += 1;
    }
    assert_eq!(patched, 2, "local and central header");
    std::fs::write(&crafted, &bytes).unwrap();
    assert_eq!(
        b.restore_backup(&crafted, FREE),
        Err(StorageError::Archive(ArchiveError::Inconsistent))
    );
    assert_live_unchanged(&b, &id);
}

#[test]
fn a_bomb_within_the_entry_limit_is_refused_by_the_total() {
    let (tmp, out, _) = make_backup();
    let dst = tmp.path().join("dst");
    let (mut b, id) = live(&dst);
    let crafted = tmp.path().join("bomb.dcbackup");
    rebuild(&out, &crafted, true, |e| {
        for k in 0..2u8 {
            let data = vec![k; 2 << 20];
            e.push((crate::archive::blob_entry_name(&blobs::sha256(&data)), data));
        }
    });
    // Each entry is far below the per-entry limit; together they break the total.
    let limits = ArchiveLimits {
        max_total_bytes: 3 << 20,
        max_json_bytes: 1 << 20,
        ..ArchiveLimits::BACKUP
    };
    assert!(limits.max_entry_bytes > 2 << 20);
    assert_eq!(
        b.restore_backup_with(&crafted, limits, FREE),
        Err(StorageError::Archive(ArchiveError::TooLarge))
    );
    assert_live_unchanged(&b, &id);
    assert_no_staging(&dst);
    // Within the default limits the same file is fine (the bound was the total).
    assert_eq!(b.restore_backup(&crafted, FREE).unwrap().clips, 3);
}

#[test]
fn a_far_too_compressed_archive_is_refused_before_extraction() {
    let (tmp, out, _) = make_backup();
    let dst = tmp.path().join("dst");
    let (mut b, id) = live(&dst);
    let crafted = tmp.path().join("ratio.dcbackup");
    rebuild(&out, &crafted, true, |e| {
        for k in 0..3u8 {
            let data = vec![k; 30 << 20];
            e.push((crate::archive::blob_entry_name(&blobs::sha256(&data)), data));
        }
    });
    assert!(std::fs::metadata(&crafted).unwrap().len() < 2 << 20);
    assert_eq!(
        b.restore_backup(&crafted, FREE),
        Err(StorageError::Archive(ArchiveError::TooLarge))
    );
    assert_live_unchanged(&b, &id);
    assert_no_staging(&dst);
}

#[test]
fn a_restore_needs_known_and_sufficient_free_space() {
    let (tmp, out, _) = make_backup();
    let dst = tmp.path().join("dst");
    let (mut b, id) = live(&dst);
    for free in [None, Some(0), Some(1024)] {
        assert_eq!(
            b.restore_backup(&out, free),
            Err(StorageError::InsufficientSpace),
            "{free:?}"
        );
        assert_live_unchanged(&b, &id);
        assert_no_staging(&dst);
    }
}

#[test]
fn a_backup_its_own_restore_would_refuse_is_not_written() {
    let tmp = tempfile::tempdir().unwrap();
    let mut s = open(&tmp.path().join("src"), clock());
    fixture(&mut s);
    let out = tmp.path().join("small.dcbackup");
    for limits in [
        ArchiveLimits {
            max_entries: 3,
            ..ArchiveLimits::BACKUP
        },
        ArchiveLimits {
            max_total_bytes: 64 * 1024,
            ..ArchiveLimits::BACKUP
        },
        ArchiveLimits {
            max_entry_bytes: 70 * 1024 - 1,
            ..ArchiveLimits::BACKUP
        },
    ] {
        assert_eq!(
            s.write_backup_with(&out, "0.0.1", limits),
            Err(StorageError::Archive(ArchiveError::TooLargeToWrite)),
            "{limits:?}"
        );
        assert!(!out.exists());
        assert!(names_in(tmp.path())
            .iter()
            .all(|n| !n.ends_with(crate::archive::PARTIAL_SUFFIX)));
    }
}

#[test]
fn a_no_export_clip_survives_backup_and_restore_with_its_flag() {
    let tmp = tempfile::tempdir().unwrap();
    let mut a = open(&tmp.path().join("src"), clock());
    let mut n = text_clip("cloud says no");
    n.no_export = true;
    let id = ins(&mut a, n);
    assert!(a.is_no_export(&id).unwrap());
    let out = tmp.path().join("b.dcbackup");
    assert_eq!(a.write_backup(&out, "0.0.1").unwrap().clips, 1);
    let (mut b, _) = live(&tmp.path().join("dst"));
    b.restore_backup(&out, FREE).unwrap();
    assert!(b.is_no_export(&id).unwrap());
}
