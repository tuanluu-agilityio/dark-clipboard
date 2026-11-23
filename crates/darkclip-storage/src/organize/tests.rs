//! Task 401 tests: migration 2, collections, favorites, tags, notes.

use super::*;
use crate::db;
use crate::migrate::{Migration, MIGRATIONS};
use crate::retention::test_support::*;
use crate::OpenMode;
use darkclip_core::{
    CollectionKind, CoreError, SensitiveCategory, Sensitivity, SensitivityLevel, SystemCollection,
    Timestamp,
};
use std::path::Path;

const CLIPS_10K: i64 = 10_000;

/// A version-1 store with `n` synthetic clips (rows, representations, FTS), built in
/// one transaction so the 10,000-clip migration tests stay fast.
fn v1_store(dir: &Path, n: i64) {
    let v1 = [Migration {
        version: 1,
        sql: MIGRATIONS[0].sql,
    }];
    let mut s = db::open_with(dir, OpenMode::ReadWrite, clock(), &v1).unwrap();
    assert_eq!(s.schema_version().unwrap(), 1);
    let tx = s.conn.transaction().unwrap();
    tx.execute(
        "WITH RECURSIVE seq(i) AS (SELECT 0 UNION ALL SELECT i + 1 FROM seq WHERE i + 1 < ?1) \
         INSERT INTO clips (id, kind, preview, content_hash, size_bytes, created_at, \
         last_copied_at) SELECT printf('01900000-0000-7000-8000-%012x', i), 'text', \
         printf('synthetic clip %d quokka', i), CAST(printf('%032d', i) AS BLOB), 24, \
         ?2 + i, ?2 + i FROM seq",
        rusqlite::params![n, T0],
    )
    .unwrap();
    tx.execute_batch(
        "INSERT INTO representations (clip_id, ord, mime, size_bytes, inline_data) \
         SELECT id, 0, 'text/plain;charset=utf-8', length(preview), CAST(preview AS BLOB) \
         FROM clips; \
         INSERT INTO clip_rowid (clip_id) SELECT id FROM clips ORDER BY id; \
         INSERT INTO clips_fts (rowid, preview, body, note, tags) \
         SELECT r.rowid, c.preview, c.preview, '', '' FROM clip_rowid r \
         JOIN clips c ON c.id = r.clip_id;",
    )
    .unwrap();
    tx.commit().unwrap();
}

fn raw_conn(path: &Path) -> rusqlite::Connection {
    rusqlite::Connection::open(path).unwrap()
}

fn raw_count(c: &rusqlite::Connection, sql: &str) -> i64 {
    c.query_row(sql, [], |r| r.get(0)).unwrap()
}

fn user_version(c: &rusqlite::Connection) -> i64 {
    c.pragma_query_value(None, "user_version", |r| r.get(0))
        .unwrap()
}

fn table_exists(c: &rusqlite::Connection, name: &str) -> bool {
    c.query_row(
        "SELECT count(*) FROM sqlite_master WHERE name = ?1",
        [name],
        |r| r.get::<_, i64>(0),
    )
    .unwrap()
        == 1
}

pub(crate) fn fts_hits(s: &Storage, word: &str) -> i64 {
    s.conn
        .query_row(
            "SELECT count(*) FROM clips_fts WHERE clips_fts MATCH ?1",
            [format!("\"{word}\"")],
            |r| r.get(0),
        )
        .unwrap()
}

fn masked(s: &str) -> crate::NewClip {
    let mut n = text_clip(s);
    n.classified.preview = "sk-\u{2026}\u{2022}\u{2022}\u{2022}\u{2022} (API key)".into();
    n.sensitivity = Sensitivity {
        level: SensitivityLevel::Masked,
        categories: vec![SensitiveCategory::ApiKey],
    };
    n.expires_at = Some(Timestamp(T0 + 3_600_000));
    n
}

fn favorites(s: &Storage) -> Collection {
    let fav: Vec<_> = s
        .list_collections()
        .unwrap()
        .into_iter()
        .filter(|c| c.system_key == Some(SystemCollection::Favorites))
        .collect();
    assert_eq!(fav.len(), 1, "exactly one Favorites collection");
    fav.into_iter().next().unwrap()
}

fn ords(s: &Storage, c: &CollectionId) -> Vec<i64> {
    let mut st = s
        .conn
        .prepare("SELECT ord FROM collection_clips WHERE collection_id = ?1 ORDER BY ord")
        .unwrap();
    st.query_map([c.to_string()], |r| r.get(0))
        .unwrap()
        .map(Result::unwrap)
        .collect()
}

fn tag_names(s: &Storage) -> Vec<(String, u64)> {
    s.list_tags()
        .unwrap()
        .into_iter()
        .map(|t| (t.tag.name, t.clip_count))
        .collect()
}

#[test]
fn migration_1_to_2_keeps_10k_clips_and_creates_favorites() {
    let tmp = tempfile::tempdir().unwrap();
    v1_store(tmp.path(), CLIPS_10K);
    let bak = tmp.path().join("history.db.pre-v2.bak");
    assert!(!bak.exists());

    let s = open(tmp.path(), clock());
    assert_eq!(s.schema_version().unwrap(), 2);
    assert_eq!(count(&s, "SELECT count(*) FROM clips"), CLIPS_10K);
    assert_eq!(count(&s, "SELECT count(*) FROM representations"), CLIPS_10K);
    assert_eq!(count(&s, "SELECT count(*) FROM clip_rowid"), CLIPS_10K);
    assert_eq!(fts_hits(&s, "quokka"), CLIPS_10K, "FTS rows survive");
    for t in ["collections", "collection_clips", "tags", "clip_tags"] {
        assert!(table_exists(&s.conn, t), "{t}");
    }
    let fav = favorites(&s);
    assert_eq!(fav.kind, CollectionKind::System);
    assert_eq!(fav.name, "Favorites");
    assert_eq!(fav.clip_count, 0);
    assert_eq!(fav.sort, CollectionSort::Recent);
    assert_eq!(s.favorites_id().unwrap(), fav.id);
    assert_eq!(fav.id.to_string(), darkclip_core::FAVORITES_COLLECTION_ID);
    // Existing clips read back unchanged, with empty organisation.
    let id = ClipId::parse("01900000-0000-7000-8000-000000001234").unwrap();
    let c = s.get_clip(&id).unwrap().unwrap();
    assert_eq!(c.preview, format!("synthetic clip {} quokka", 0x1234));
    assert!(c.tags.is_empty() && c.collections.is_empty() && c.note.is_none());

    // The pre-migration backup is the version-1 database.
    assert!(bak.is_file(), "backup before migrating");
    {
        let b = raw_conn(&bak);
        assert_eq!(user_version(&b), 1);
        assert_eq!(raw_count(&b, "SELECT count(*) FROM clips"), CLIPS_10K);
        assert!(!table_exists(&b, "collections"));
    }
    drop(s);
    // The next successful run at version 2 removes it; Favorites is not duplicated.
    let s = open(tmp.path(), clock());
    assert!(!bak.exists(), "backup removed after a successful run");
    favorites(&s);
    // A brand-new store gets Favorites too, without a backup.
    let fresh = tempfile::tempdir().unwrap();
    let f = open(fresh.path(), clock());
    favorites(&f);
    assert!(!fresh.path().join("history.db.pre-v2.bak").exists());
}

#[test]
fn migration_2_failure_mid_transaction_keeps_v1_and_backup() {
    let good = MIGRATIONS[1].sql;
    let mid = good.replacen(
        "CREATE TABLE tags",
        "SELECT no_such_function();\nCREATE TABLE tags",
        1,
    );
    assert_ne!(mid, good, "failure injected mid-file");
    // Also after `PRAGMA user_version = 2`: the version change must roll back too.
    let end = format!("{good}\nSELECT no_such_function();");
    for bad_sql in [mid, end] {
        let tmp = tempfile::tempdir().unwrap();
        v1_store(tmp.path(), CLIPS_10K);
        let db_path = tmp.path().join("history.db");
        let bak = tmp.path().join("history.db.pre-v2.bak");
        let bad_sql: &'static str = Box::leak(bad_sql.into_boxed_str());
        let bad = [
            Migration {
                version: 1,
                sql: MIGRATIONS[0].sql,
            },
            Migration {
                version: 2,
                sql: bad_sql,
            },
        ];
        assert!(db::open_with(tmp.path(), OpenMode::ReadWrite, clock(), &bad).is_err());
        {
            let c = raw_conn(&db_path);
            assert_eq!(user_version(&c), 1, "still version 1");
            assert_eq!(raw_count(&c, "SELECT count(*) FROM clips"), CLIPS_10K);
            assert_eq!(
                raw_count(&c, "SELECT count(*) FROM representations"),
                CLIPS_10K
            );
            assert_eq!(
                raw_count(
                    &c,
                    "SELECT count(*) FROM clips_fts WHERE clips_fts MATCH 'quokka'"
                ),
                CLIPS_10K
            );
            for t in ["collections", "collection_clips", "tags", "clip_tags"] {
                assert!(!table_exists(&c, t), "{t} rolled back");
            }
        }
        assert!(bak.is_file(), "backup kept after a failed migration");
        {
            let b = raw_conn(&bak);
            assert_eq!(user_version(&b), 1);
            assert_eq!(raw_count(&b, "SELECT count(*) FROM clips"), CLIPS_10K);
        }
        // A later run with the real migration succeeds from the intact version 1.
        let s = open(tmp.path(), clock());
        assert_eq!(s.schema_version().unwrap(), 2);
        assert_eq!(count(&s, "SELECT count(*) FROM clips"), CLIPS_10K);
        favorites(&s);
    }
}

#[test]
fn favorites_cannot_be_renamed_or_deleted() {
    let tmp = tempfile::tempdir().unwrap();
    let mut s = open(tmp.path(), clock());
    let fav = s.favorites_id().unwrap();
    assert!(matches!(
        s.rename_collection(&fav, "Mine"),
        Err(StorageError::InvalidArgument(_))
    ));
    assert!(matches!(
        s.delete_collection(&fav),
        Err(StorageError::InvalidArgument(_))
    ));
    // Not even through SQL: the schema refuses it as well.
    assert!(s
        .conn
        .execute("DELETE FROM collections WHERE system_key = 'favorites'", [])
        .is_err());
    assert!(s
        .conn
        .execute(
            "UPDATE collections SET name = 'x' WHERE system_key = 'favorites'",
            []
        )
        .is_err());
    assert!(s
        .conn
        .execute(
            "INSERT INTO collections (id, name, kind, system_key, sort, created_at, \
             updated_at) VALUES ('01900000-0000-7000-8000-00000000ffff', 'Fav 2', \
             'system', 'favorites', 'recent', 0, 0)",
            []
        )
        .is_err());
    assert_eq!(favorites(&s).name, "Favorites");

    // User collections can be renamed and deleted; names are validated.
    let c = s
        .create_collection("  Project Atlas ", CollectionSort::Manual, Some("#4f8cff"))
        .unwrap();
    assert_eq!(c.name, "Project Atlas");
    assert_eq!(c.kind, CollectionKind::User);
    assert_eq!(c.system_key, None);
    assert_eq!(c.color.as_deref(), Some("#4f8cff"));
    assert_eq!(c.clip_count, 0);
    s.rename_collection(&c.id, "Atlas").unwrap();
    assert!(s
        .list_collections()
        .unwrap()
        .iter()
        .any(|x| x.id == c.id && x.name == "Atlas"));
    assert!(matches!(
        s.rename_collection(&c.id, "   "),
        Err(StorageError::Invalid(CoreError::Invalid { .. }))
    ));
    assert!(matches!(
        s.create_collection(&"n".repeat(81), CollectionSort::Recent, None),
        Err(StorageError::Invalid(CoreError::TooLong { .. }))
    ));
    assert!(matches!(
        s.create_collection("ok", CollectionSort::Recent, Some("blue")),
        Err(StorageError::Invalid(_))
    ));
    s.delete_collection(&c.id).unwrap();
    assert_eq!(s.delete_collection(&c.id), Err(StorageError::NotFound));
    assert_eq!(s.rename_collection(&c.id, "x"), Err(StorageError::NotFound));
    assert_eq!(s.list_collections().unwrap().len(), 1);
    // A read-only store lists but never writes.
    drop(s);
    let mut ro = crate::Storage::open(tmp.path(), OpenMode::ReadOnly, clock()).unwrap();
    assert_eq!(ro.list_collections().unwrap().len(), 1);
    assert_eq!(
        ro.create_collection("x", CollectionSort::Recent, None),
        Err(StorageError::ReadOnly)
    );
}

#[test]
fn collection_membership_idempotent_and_cascades_on_clip_delete() {
    let tmp = tempfile::tempdir().unwrap();
    let mut s = open(tmp.path(), clock());
    let a = ins(&mut s, text_clip("alpha clip"));
    let b = ins(&mut s, text_clip("bravo clip"));
    let col = s
        .create_collection("Work", CollectionSort::Recent, None)
        .unwrap()
        .id;
    let fav = s.favorites_id().unwrap();
    s.add_to_collection(&col, &a).unwrap();
    s.add_to_collection(&col, &a).unwrap();
    s.add_to_collection(&col, &b).unwrap();
    s.add_to_collection(&fav, &a).unwrap();
    assert_eq!(
        count(&s, "SELECT count(*) FROM collection_clips"),
        3,
        "adding twice is one membership"
    );
    let counts = |s: &Storage| -> Vec<(CollectionId, u64)> {
        s.list_collections()
            .unwrap()
            .iter()
            .map(|c| (c.id, c.clip_count))
            .collect()
    };
    assert!(counts(&s).contains(&(col, 2)));
    assert!(counts(&s).contains(&(fav, 1)));
    let got = s.get_clip(&a).unwrap().unwrap();
    let mut names: Vec<_> = got.collections.iter().map(|c| c.name.as_str()).collect();
    names.sort_unstable();
    assert_eq!(names, vec!["Favorites", "Work"]);

    s.remove_from_collection(&col, &b).unwrap();
    s.remove_from_collection(&col, &b).unwrap();
    assert!(counts(&s).contains(&(col, 1)));

    // Deleting the clip removes every membership; the collections stay.
    s.delete_clip(&a).unwrap();
    assert_eq!(count(&s, "SELECT count(*) FROM collection_clips"), 0);
    assert!(counts(&s).contains(&(col, 0)));
    assert!(counts(&s).contains(&(fav, 0)));

    // Deleting a collection keeps its clips.
    s.add_to_collection(&col, &b).unwrap();
    s.delete_collection(&col).unwrap();
    assert!(s.get_clip(&b).unwrap().is_some());
    assert_eq!(count(&s, "SELECT count(*) FROM collection_clips"), 0);

    let missing_clip = ClipId::new_v7(Timestamp(1));
    let missing_col = CollectionId::new_v7(Timestamp(1));
    assert_eq!(
        s.add_to_collection(&fav, &missing_clip),
        Err(StorageError::NotFound)
    );
    assert_eq!(
        s.add_to_collection(&missing_col, &b),
        Err(StorageError::NotFound)
    );
    assert_eq!(
        s.remove_from_collection(&missing_col, &b),
        Err(StorageError::NotFound)
    );
}

#[test]
fn manual_order_reorder_is_dense() {
    let tmp = tempfile::tempdir().unwrap();
    let mut s = open(tmp.path(), clock());
    let ids: Vec<ClipId> = ["one", "two", "three", "four"]
        .iter()
        .map(|t| ins(&mut s, text_clip(t)))
        .collect();
    let (a, b, c, d) = (ids[0], ids[1], ids[2], ids[3]);
    let m = s
        .create_collection("Snippets", CollectionSort::Manual, None)
        .unwrap()
        .id;
    for id in &ids {
        s.add_to_collection(&m, id).unwrap();
    }
    assert_eq!(s.collection_clip_ids(&m).unwrap(), vec![a, b, c, d]);
    assert_eq!(ords(&s, &m), vec![0, 1, 2, 3]);

    s.remove_from_collection(&m, &b).unwrap();
    assert_eq!(s.collection_clip_ids(&m).unwrap(), vec![a, c, d]);
    assert_eq!(ords(&s, &m), vec![0, 1, 2], "dense after removal");

    s.move_in_collection(&m, &d, 0).unwrap();
    assert_eq!(s.collection_clip_ids(&m).unwrap(), vec![d, a, c]);
    assert_eq!(ords(&s, &m), vec![0, 1, 2]);
    s.move_in_collection(&m, &d, 99).unwrap();
    assert_eq!(s.collection_clip_ids(&m).unwrap(), vec![a, c, d], "clamped");
    s.move_in_collection(&m, &d, 1).unwrap();
    assert_eq!(s.collection_clip_ids(&m).unwrap(), vec![a, d, c]);
    assert_eq!(ords(&s, &m), vec![0, 1, 2]);

    // A clip deleted from the history leaves the order dense at the next reorder.
    s.delete_clip(&a).unwrap();
    s.move_in_collection(&m, &c, 0).unwrap();
    assert_eq!(s.collection_clip_ids(&m).unwrap(), vec![c, d]);
    assert_eq!(ords(&s, &m), vec![0, 1]);

    // Newly added clips go to the end.
    s.add_to_collection(&m, &b).unwrap();
    assert_eq!(s.collection_clip_ids(&m).unwrap(), vec![c, d, b]);
    assert_eq!(ords(&s, &m), vec![0, 1, 2]);

    assert_eq!(s.move_in_collection(&m, &a, 0), Err(StorageError::NotFound));
    let fav = s.favorites_id().unwrap();
    s.add_to_collection(&fav, &b).unwrap();
    assert!(matches!(
        s.move_in_collection(&fav, &b, 0),
        Err(StorageError::InvalidArgument(_))
    ));
}

#[test]
fn tags_normalised_and_unused_removed() {
    let tmp = tempfile::tempdir().unwrap();
    let mut s = open(tmp.path(), clock());
    let a = ins(&mut s, text_clip("alpha clip"));
    let b = ins(&mut s, text_clip("bravo clip"));
    let set = s
        .set_clip_tags(&a, &[" SQL ", "sql", "Tiếng-Việt"])
        .unwrap();
    let mut names: Vec<_> = set.iter().map(|t| t.name.as_str()).collect();
    names.sort_unstable();
    assert_eq!(
        names,
        vec!["sql", "tiếng-việt"],
        "normalised and deduplicated"
    );
    s.set_clip_tags(&b, &["Sql"]).unwrap();
    assert_eq!(
        tag_names(&s),
        vec![("sql".into(), 2), ("tiếng-việt".into(), 1)]
    );
    let got = s.get_clip(&a).unwrap().unwrap();
    let mut on_a: Vec<_> = got.tags.iter().map(|t| t.name.as_str()).collect();
    on_a.sort_unstable();
    assert_eq!(on_a, vec!["sql", "tiếng-việt"]);

    // Replacing the set removes the tag nobody uses any more, in the same call.
    s.set_clip_tags(&a, &["rust"]).unwrap();
    assert_eq!(tag_names(&s), vec![("rust".into(), 1), ("sql".into(), 1)]);
    assert_eq!(count(&s, "SELECT count(*) FROM tags"), 2);

    // Invalid input changes nothing.
    let before = tag_names(&s);
    assert_eq!(
        s.set_clip_tags(&a, &["ok", "a b"]),
        Err(StorageError::Invalid(CoreError::InvalidTag))
    );
    assert_eq!(tag_names(&s), before);
    let many: Vec<String> = (0..21).map(|i| format!("t{i}")).collect();
    assert!(matches!(
        s.set_clip_tags(&a, &many),
        Err(StorageError::InvalidArgument(_))
    ));
    assert_eq!(tag_names(&s), before);
    assert!(s.set_clip_tags(&a, &many[..20]).is_ok(), "20 is allowed");
    s.set_clip_tags(&a, &["rust"]).unwrap();
    assert_eq!(tag_names(&s), before);
    let missing = ClipId::new_v7(Timestamp(1));
    assert_eq!(
        s.set_clip_tags(&missing, &["x"]),
        Err(StorageError::NotFound)
    );

    // Deleting a clip cascades its tags and drops the unused ones.
    s.delete_clip(&b).unwrap();
    assert_eq!(tag_names(&s), vec![("rust".into(), 1)]);
    s.set_clip_tags::<&str>(&a, &[]).unwrap();
    assert!(tag_names(&s).is_empty());
    assert_eq!(count(&s, "SELECT count(*) FROM clip_tags"), 0);
}

#[test]
fn vietnamese_tag_searchable_without_accents() {
    let tmp = tempfile::tempdir().unwrap();
    let mut s = open(tmp.path(), clock());
    let id = ins(&mut s, text_clip("weekly meeting minutes"));
    ins(&mut s, text_clip("unrelated words"));
    let tags = s.set_clip_tags(&id, &["TIẾNG-Việt"]).unwrap();
    assert_eq!(tags[0].name, "tiếng-việt", "diacritics kept as written");
    for text in ["tieng", "viet", "tiếng", "Việt", "TIENG VIET"] {
        let q = crate::SearchQuery {
            text: Some(text.into()),
            kinds: vec![],
            pinned_only: false,
            since: None,
            include_masked: true,
            limit: 50,
            cursor: None,
            collection: None,
            tag: None,
        };
        let ids: Vec<_> = s.search(&q).unwrap().0.iter().map(|c| c.id).collect();
        assert_eq!(ids, vec![id], "{text}");
    }
}

#[test]
fn note_indexed_and_reindexed_on_change() {
    let tmp = tempfile::tempdir().unwrap();
    let mut s = open(tmp.path(), clock());
    // Large enough to live in a blob: the re-index reads the body back from it.
    let body = format!("bodymarker {}", "filler ".repeat(12_000));
    let id = ins(&mut s, text_clip(&body));
    assert_eq!(
        s.get_clip(&id).unwrap().unwrap().representations[0].storage,
        darkclip_core::Storage::Blob
    );
    s.set_clip_tags(&id, &["zebra"]).unwrap();
    s.set_note(&id, Some("remember the quokka")).unwrap();
    assert_eq!(
        s.get_clip(&id).unwrap().unwrap().note.as_deref(),
        Some("remember the quokka")
    );
    assert_eq!(fts_hits(&s, "quokka"), 1);
    assert_eq!(fts_hits(&s, "bodymarker"), 1, "body kept");
    assert_eq!(fts_hits(&s, "zebra"), 1, "tags kept");

    s.set_note(&id, Some("now a wombat")).unwrap();
    assert_eq!(fts_hits(&s, "quokka"), 0, "old note gone from the index");
    assert_eq!(fts_hits(&s, "wombat"), 1);
    assert_eq!(fts_hits(&s, "bodymarker"), 1);
    assert_eq!(fts_hits(&s, "zebra"), 1);

    // A bump keeps the note and tags in the index.
    assert_eq!(
        s.upsert_clip(&text_clip(&body)).unwrap(),
        crate::UpsertOutcome::Bumped(id)
    );
    assert_eq!(fts_hits(&s, "wombat"), 1);
    assert_eq!(fts_hits(&s, "zebra"), 1);
    assert_eq!(fts_hits(&s, "bodymarker"), 1);

    s.set_note(&id, None).unwrap();
    assert_eq!(s.get_clip(&id).unwrap().unwrap().note, None);
    assert_eq!(fts_hits(&s, "wombat"), 0);
    assert_eq!(fts_hits(&s, "bodymarker"), 1);
    assert_eq!(count(&s, "SELECT count(*) FROM clips_fts"), 1);

    assert!(matches!(
        s.set_note(&id, Some(&"n".repeat(4001))),
        Err(StorageError::Invalid(CoreError::TooLong { .. }))
    ));
    assert!(s.set_note(&id, Some(&"n".repeat(4000))).is_ok());
    let missing = ClipId::new_v7(Timestamp(1));
    assert_eq!(s.set_note(&missing, Some("x")), Err(StorageError::NotFound));
}

#[test]
fn note_and_tags_on_masked_clip_refused() {
    let tmp = tempfile::tempdir().unwrap();
    let mut s = open(tmp.path(), clock());
    let secret = concat!("sk-", "synthetic0000000000000000");
    let id = ins(&mut s, masked(secret));
    assert!(matches!(
        s.set_note(&id, Some("platypus note")),
        Err(StorageError::InvalidArgument(_))
    ));
    assert!(matches!(
        s.set_clip_tags(&id, &["platypus"]),
        Err(StorageError::InvalidArgument(_))
    ));
    let c = s.get_clip(&id).unwrap().unwrap();
    assert!(c.note.is_none() && c.tags.is_empty());
    assert_eq!(fts_hits(&s, "platypus"), 0);
    assert_eq!(count(&s, "SELECT count(*) FROM tags"), 0);
    // Clearing is always allowed.
    s.set_note(&id, None).unwrap();
    s.set_clip_tags::<&str>(&id, &[]).unwrap();

    // A clip that carries a note and tags and is then re-captured as masked indexes
    // neither: only the masked preview.
    let tmp2 = tempfile::tempdir().unwrap();
    let mut s2 = open(tmp2.path(), clock());
    let p = ins(&mut s2, text_clip(secret));
    s2.set_note(&p, Some("echidna note")).unwrap();
    s2.set_clip_tags(&p, &["echidna"]).unwrap();
    assert_eq!(fts_hits(&s2, "echidna"), 1);
    assert_eq!(
        s2.upsert_clip(&masked(secret)).unwrap(),
        crate::UpsertOutcome::Bumped(p)
    );
    assert_eq!(
        fts_hits(&s2, "echidna"),
        0,
        "masked: note and tags not indexed"
    );
    assert_eq!(fts_hits(&s2, "synthetic0000000000000000"), 0);
    assert_eq!(count(&s2, "SELECT count(*) FROM clips_fts"), 1);
}

#[test]
fn recapture_as_masked_clears_note_and_tags() {
    let tmp = tempfile::tempdir().unwrap();
    let mut s = open(tmp.path(), clock());
    let secret = concat!("sk-", "synthetic1111111111111111");
    let id = ins(&mut s, text_clip(secret));
    let col = s
        .create_collection("Keys", CollectionSort::Recent, None)
        .unwrap()
        .id;
    s.add_to_collection(&col, &id).unwrap();
    s.set_note(&id, Some("echidna note")).unwrap();
    s.set_clip_tags(&id, &["echidna", "shared"]).unwrap();
    let other = ins(&mut s, text_clip("another clip"));
    s.set_clip_tags(&other, &["shared"]).unwrap();

    assert_eq!(
        s.upsert_clip(&masked(secret)).unwrap(),
        crate::UpsertOutcome::Bumped(id)
    );
    let c = s.get_clip(&id).unwrap().unwrap();
    assert_eq!(c.note, None, "note cleared");
    assert!(c.tags.is_empty(), "tags cleared");
    assert_eq!(c.collections.len(), 1, "memberships stay");
    assert_eq!(
        tag_names(&s),
        vec![("shared".into(), 1)],
        "unused tag deleted"
    );
    let q = crate::SearchQuery {
        text: None,
        kinds: vec![],
        pinned_only: false,
        since: None,
        include_masked: true,
        limit: 50,
        cursor: None,
        collection: None,
        tag: Some("echidna".into()),
    };
    assert!(s.search(&q).unwrap().0.is_empty(), "tag filter misses it");
    assert_eq!(fts_hits(&s, "echidna"), 0);
}

#[test]
fn set_clip_tags_stops_at_the_limit() {
    let tmp = tempfile::tempdir().unwrap();
    let mut s = open(tmp.path(), clock());
    let a = ins(&mut s, text_clip("alpha clip"));
    // 21 distinct names, then a huge tail that ends in an invalid name: the call must
    // stop at the 21st distinct name instead of walking (and validating) the whole list.
    let mut huge: Vec<String> = (0..21).map(|i| format!("t{i}")).collect();
    huge.extend(std::iter::repeat_n("t0".to_owned(), 200_000));
    huge.push("not a tag".into());
    assert!(matches!(
        s.set_clip_tags(&a, &huge),
        Err(StorageError::InvalidArgument(_))
    ));
    // Many duplicates of few names are fine.
    let dups: Vec<&str> = std::iter::repeat_n("dup", 100_000).collect();
    let set = s.set_clip_tags(&a, &dups).unwrap();
    assert_eq!(set.len(), 1);
    assert_eq!(tag_names(&s), vec![("dup".into(), 1)]);
}

#[test]
fn vietnamese_d_stroke_folds_in_tags_and_notes() {
    let tmp = tempfile::tempdir().unwrap();
    let mut s = open(tmp.path(), clock());
    let id = ins(&mut s, text_clip("travel plans"));
    ins(&mut s, text_clip("unrelated words"));
    let tags = s.set_clip_tags(&id, &["Đà-Nẵng"]).unwrap();
    assert_eq!(tags[0].name, "đà-nẵng", "stored as written");
    let note_id = ins(&mut s, text_clip("hotel list"));
    s.set_note(&note_id, Some("Khách sạn Đông Hà")).unwrap();
    let find = |s: &Storage, text: &str| -> Vec<ClipId> {
        let q = crate::SearchQuery {
            text: Some(text.into()),
            kinds: vec![],
            pinned_only: false,
            since: None,
            include_masked: true,
            limit: 50,
            cursor: None,
            collection: None,
            tag: None,
        };
        s.search(&q).unwrap().0.iter().map(|c| c.id).collect()
    };
    for text in ["da-nang", "da nang", "đà", "Đà Nẵng", "nang"] {
        assert_eq!(find(&s, text), vec![id], "{text}");
    }
    assert_eq!(
        find(&s, "dan"),
        Vec::<ClipId>::new(),
        "prefix of a folded word only"
    );
    for text in ["dong ha", "đông", "Dong"] {
        assert_eq!(find(&s, text), vec![note_id], "{text}");
    }
    // The tag filter accepts the name as written (any case).
    let mut q = crate::SearchQuery {
        text: None,
        kinds: vec![],
        pinned_only: false,
        since: None,
        include_masked: true,
        limit: 50,
        cursor: None,
        collection: None,
        tag: Some("ĐÀ-NẴNG".into()),
    };
    let ids: Vec<_> = s.search(&q).unwrap().0.iter().map(|c| c.id).collect();
    assert_eq!(ids, vec![id]);
    q.tag = Some("other".into());
    assert!(s.search(&q).unwrap().0.is_empty());
}

#[test]
fn clip_labels_batch_matches_the_single_lookup() {
    let tmp = tempfile::tempdir().unwrap();
    let mut s = open(tmp.path(), clock());
    let ids: Vec<ClipId> = ["one", "two", "three"]
        .iter()
        .map(|t| match s.upsert_clip(&text_clip(t)).unwrap() {
            crate::UpsertOutcome::Inserted(id) | crate::UpsertOutcome::Bumped(id) => id,
        })
        .collect();
    let fav = s.favorites_id().unwrap();
    s.add_to_collection(&fav, &ids[0]).unwrap();
    s.set_clip_tags(&ids[0], &["work", "ideas"]).unwrap();
    s.set_clip_tags(&ids[1], &["work"]).unwrap();
    let ghost = ClipId::new_v7(Timestamp(T0));
    let mut asked = ids.clone();
    asked.push(ghost);
    let batch = s.clip_labels_batch(&asked).unwrap();
    for id in &ids {
        assert_eq!(batch.get(id).cloned().unwrap(), s.clip_labels(id).unwrap());
    }
    assert_eq!(batch[&ids[0]], (true, vec!["ideas".into(), "work".into()]));
    assert_eq!(batch[&ids[2]], (false, Vec::<String>::new()));
    assert!(s.clip_labels_batch(&[]).unwrap().is_empty());
}
