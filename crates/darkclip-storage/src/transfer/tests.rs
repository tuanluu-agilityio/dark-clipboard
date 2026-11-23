//! Task 405 tests: export files and the reader the import uses.

use crate::archive::ArchiveLimits;
use crate::blobs;
use crate::retention::test_support::*;
use crate::{ArchiveError, ExportArchive, Storage, StorageError};
use darkclip_core::{
    ClipId, Collection, CollectionKind, CollectionSort, Mime, SchemaVersion, SensitiveCategory,
    Sensitivity, SensitivityLevel, Timestamp,
};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

const SECRET_TAIL: &str = "Hh7Gg6Ff5Ee4Dd3Cc2Bb1Aa0";

fn secret() -> String {
    // Runtime-assembled (GC-09).
    format!("{}{}", concat!("gh", "p_"), SECRET_TAIL)
}

fn email() -> String {
    format!("{}@{}", "someone", "example.org")
}

struct Fixture {
    text: ClipId,
    image: ClipId,
    masked: ClipId,
    stored_sensitive: ClipId,
    png: Vec<u8>,
    col: Collection,
}

fn fixture(s: &mut Storage) -> Fixture {
    let text = ins(s, text_clip("export me please"));
    let png = png_bytes(3);
    let mut img = raw(Mime::ImagePng, &png);
    img.width = Some(1);
    img.height = Some(1);
    let mut n = new_clip(vec![img]);
    n.thumbnail_png = Some(png_bytes(4));
    let image = ins(s, n);
    let mut m = text_clip(&secret());
    m.classified.preview = "ghp_\u{2026}\u{2022}\u{2022}\u{2022}\u{2022}".into();
    m.sensitivity = Sensitivity {
        level: SensitivityLevel::Masked,
        categories: vec![SensitiveCategory::Token],
    };
    m.expires_at = Some(Timestamp(T0 + 3_600_000));
    let masked = ins(s, m);
    // "Store" handling: level none, but a category was found. Never exported either.
    let mut e = text_clip(&email());
    e.sensitivity = Sensitivity {
        level: SensitivityLevel::None,
        categories: vec![SensitiveCategory::Email],
    };
    let stored_sensitive = ins(s, e);
    let col = s
        .create_collection("Snippets", CollectionSort::Manual, None)
        .unwrap();
    for id in [text, image, masked, stored_sensitive] {
        // Masked clips cannot be added (D-060): only the others become members.
        let _ = s.add_to_collection(&col.id, &id);
    }
    s.set_note(&text, Some("a note travels")).unwrap();
    s.set_clip_tags(&text, &["shared"]).unwrap();
    Fixture {
        text,
        image,
        masked,
        stored_sensitive,
        png,
        col,
    }
}

fn export_all(s: &Storage, f: &Fixture, dest: &Path) -> crate::ExportReport {
    let missing = ClipId::new_v7(Timestamp(1));
    s.write_export(
        &[f.text, f.image, f.masked, f.stored_sensitive, missing],
        &f.col,
        dest,
        "0.0.1",
    )
    .unwrap()
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

type Entries = Vec<(String, Vec<u8>)>;

fn rebuild(src: &Path, dst: &Path, edit: impl FnOnce(&mut Vec<(String, Vec<u8>)>)) {
    let mut e = entries_of(src);
    edit(&mut e);
    write_zip(dst, &e);
}

fn edit_json(e: &mut [(String, Vec<u8>)], f: impl FnOnce(&mut serde_json::Value)) {
    let j = e.iter_mut().find(|(n, _)| n == "export.json").unwrap();
    let mut v: serde_json::Value = serde_json::from_slice(&j.1).unwrap();
    f(&mut v);
    j.1 = serde_json::to_vec(&v).unwrap();
}

fn made() -> (tempfile::TempDir, Storage, Fixture, PathBuf) {
    let tmp = tempfile::tempdir().unwrap();
    let mut s = open(&tmp.path().join("data"), clock());
    let f = fixture(&mut s);
    let out = tmp.path().join("snippets.dcexport");
    (tmp, s, f, out)
}

#[test]
fn export_excludes_masked_and_sensitive_and_reports_the_count() {
    let (_tmp, s, f, out) = made();
    let r = export_all(&s, &f, &out);
    assert_eq!(r.exported, 2);
    assert_eq!(r.excluded_sensitive, 2);
    assert_eq!(r.missing, 1);
    let a = ExportArchive::open(&out, ArchiveLimits::EXPORT).unwrap();
    let ids: Vec<ClipId> = a.clips().iter().map(|c| c.id).collect();
    assert_eq!(ids, [f.text, f.image]);
    for c in a.clips() {
        assert_eq!(c.sensitivity.level, SensitivityLevel::None);
        assert!(c.sensitivity.categories.is_empty());
    }
    // Neither secret is anywhere in the file.
    for (_, bytes) in entries_of(&out) {
        let hay = String::from_utf8_lossy(&bytes);
        assert!(!hay.contains(SECRET_TAIL));
        assert!(!hay.contains(&email()));
    }
    assert!(!out.with_extension("dcexport.partial").exists());
}

#[test]
fn export_reads_back_with_its_blobs() {
    let (_tmp, s, f, out) = made();
    export_all(&s, &f, &out);
    let mut a = ExportArchive::open(&out, ArchiveLimits::EXPORT).unwrap();
    assert_eq!(a.collection().name, "Snippets");
    assert_eq!(a.collection().clip_count, 2);
    let clips = a.clips().to_vec();
    let text = &clips[0];
    assert_eq!(text.note.as_deref(), Some("a note travels"));
    assert_eq!(text.tags.len(), 1);
    assert_eq!(text.source, None, "source fields are not exported");
    assert_eq!(text.collections.len(), 1);
    assert_eq!(text.collections[0].id, f.col.id);
    let sha = text.representations[0].blob_sha256.unwrap();
    assert_eq!(a.blob(&sha).unwrap(), b"export me please");
    let image = &clips[1];
    assert_eq!(image.metadata.image_thumb_sha256, None);
    let sha = image.representations[0].blob_sha256.unwrap();
    assert_eq!(a.blob(&sha).unwrap(), f.png);
    // A hash that is not in the file.
    assert!(a.blob(&[9; 32]).is_err());
}

fn compile_export_schema() -> (boon::Schemas, boon::SchemaIndex) {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/architecture/schemas");
    let mut c = boon::Compiler::new();
    c.enable_format_assertions();
    let mut export_id = String::new();
    for e in std::fs::read_dir(&dir).unwrap() {
        let p = e.unwrap().path();
        let v: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&p).unwrap()).unwrap();
        let id = v["$id"].as_str().unwrap().to_string();
        if p.file_name().unwrap() == "export.schema.json" {
            export_id = id.clone();
        }
        c.add_resource(&id, v).unwrap();
    }
    let mut schemas = boon::Schemas::new();
    let idx = c.compile(&export_id, &mut schemas).unwrap();
    (schemas, idx)
}

#[test]
fn export_json_validates_against_the_schema() {
    let (_tmp, s, f, out) = made();
    export_all(&s, &f, &out);
    let json = entries_of(&out)
        .into_iter()
        .find(|(n, _)| n == "export.json")
        .unwrap()
        .1;
    let v: serde_json::Value = serde_json::from_slice(&json).unwrap();
    let (schemas, idx) = compile_export_schema();
    if let Err(e) = schemas.validate(&v, idx) {
        panic!("export.json does not match export.schema.json: {e}");
    }
    // The validator does reject a broken document (the test is not vacuous).
    let mut broken = v.clone();
    broken["clips"][0]["kind"] = serde_json::json!("spreadsheet");
    assert!(schemas.validate(&broken, idx).is_err());
}

#[test]
fn an_export_of_selected_clips_uses_the_collection_it_is_given() {
    let (_tmp, s, f, out) = made();
    let synthetic = Collection {
        schema_version: SchemaVersion,
        id: darkclip_core::CollectionId::new_v7(Timestamp(T0)),
        name: "Selected clips".into(),
        kind: CollectionKind::User,
        system_key: None,
        color: None,
        created_at: Timestamp(T0),
        updated_at: Timestamp(T0),
        clip_count: 0,
        sort: CollectionSort::Manual,
    };
    let r = s
        .write_export(&[f.image], &synthetic, &out, "0.0.1")
        .unwrap();
    assert_eq!((r.exported, r.excluded_sensitive, r.missing), (1, 0, 0));
    let a = ExportArchive::open(&out, ArchiveLimits::EXPORT).unwrap();
    assert_eq!(a.collection().id, synthetic.id);
    assert_eq!(a.collection().clip_count, 1);
}

#[test]
fn an_unsafe_or_mismatched_export_is_refused() {
    let (tmp, s, f, out) = made();
    export_all(&s, &f, &out);
    let crafted = tmp.path().join("crafted.dcexport");
    let refused = |want: ArchiveError, edit: &dyn Fn(&mut Entries)| {
        rebuild(&out, &crafted, |e| edit(e));
        let got = ExportArchive::open(&crafted, ArchiveLimits::EXPORT).map(|_| ());
        assert_eq!(got, Err(StorageError::Archive(want)));
    };
    refused(ArchiveError::BadEntryName, &|e| {
        e.push(("../escape.txt".into(), b"x".to_vec()));
    });
    refused(ArchiveError::BadEntryName, &|e| {
        e.push(("history.db".into(), b"x".to_vec()));
    });
    refused(ArchiveError::NotAnArchive, &|e| {
        e.retain(|(n, _)| n != "export.json");
    });
    refused(ArchiveError::BadManifest, &|e| {
        e.retain(|(n, _)| !n.starts_with("blobs/"));
    });
    refused(ArchiveError::BadManifest, &|e| {
        edit_json(e, |v| v["surprise"] = serde_json::json!(1));
    });
    refused(ArchiveError::BadManifest, &|e| {
        let j = e.iter_mut().find(|(n, _)| n == "export.json").unwrap();
        j.1 = b"[1, 2".to_vec();
    });
    refused(ArchiveError::NewerFormat, &|e| {
        edit_json(e, |v| v["schema_version"] = serde_json::json!(2));
    });
    // A blob list entry whose path does not match its hash.
    refused(ArchiveError::BadManifest, &|e| {
        edit_json(e, |v| {
            v["blobs"][0]["path"] = serde_json::json!(format!("blobs/00/{}", "0".repeat(64)));
        });
    });
    // Too many entries for the limits.
    rebuild(&out, &crafted, |_| {});
    let few = ArchiveLimits {
        max_entries: 2,
        ..ArchiveLimits::EXPORT
    };
    assert_eq!(
        ExportArchive::open(&crafted, few).map(|_| ()),
        Err(StorageError::Archive(ArchiveError::TooManyEntries))
    );
    // A blob whose bytes do not match its name is refused when it is read.
    rebuild(&out, &crafted, |e| {
        let b = e.iter_mut().find(|(n, _)| n.starts_with("blobs/")).unwrap();
        b.1[0] ^= 0xff;
    });
    let mut a = ExportArchive::open(&crafted, ArchiveLimits::EXPORT).unwrap();
    let shas: Vec<[u8; 32]> = a
        .clips()
        .iter()
        .flat_map(|c| c.representations.iter().filter_map(|r| r.blob_sha256))
        .collect();
    let results: Vec<_> = shas.iter().map(|sha| a.blob(sha)).collect();
    assert!(results.contains(&Err(StorageError::Archive(ArchiveError::ChecksumMismatch))));
}

#[test]
fn import_created_at_is_clamped_to_now() {
    let tmp = tempfile::tempdir().unwrap();
    let c = clock();
    let mut s = open(tmp.path(), c.clone());
    let id = ins(&mut s, text_clip("imported"));
    let now = Timestamp(T0);
    assert_eq!(
        s.import_created_at(&id, Timestamp(T0 + 86_400_000))
            .unwrap(),
        now
    );
    let clip = s.get_clip(&id).unwrap().unwrap();
    assert!(clip.created_at <= now && clip.created_at <= clip.last_copied_at);
    let past = Timestamp(T0 - 86_400_000);
    assert_eq!(s.import_created_at(&id, past).unwrap(), past);
    assert_eq!(s.get_clip(&id).unwrap().unwrap().created_at, past);
    assert_eq!(
        s.import_created_at(&id, Timestamp(-5)).unwrap(),
        Timestamp(0)
    );
    let missing = ClipId::new_v7(Timestamp(1));
    assert_eq!(
        s.import_created_at(&missing, past),
        Err(StorageError::NotFound)
    );
    let _ = blobs::hex(&[0; 32]);
}

// ---- Task 405 security review ----

#[test]
fn a_no_export_clip_is_never_exported_and_the_flag_is_sticky() {
    let (tmp, mut s, f, out) = made();
    let mut n = text_clip("the source said: no cloud");
    n.no_export = true;
    let flagged = ins(&mut s, n);
    assert!(s.is_no_export(&flagged).unwrap());
    assert!(!s.is_no_export(&f.text).unwrap());
    // A later copy of the same content without the marker does not clear it.
    let mut again = text_clip("the source said: no cloud");
    again.no_export = false;
    s.upsert_clip(&again).unwrap();
    assert!(s.is_no_export(&flagged).unwrap());
    // A marked copy of an unflagged clip sets it.
    let mut marked = text_clip("export me please");
    marked.no_export = true;
    s.upsert_clip(&marked).unwrap();
    assert!(s.is_no_export(&f.text).unwrap());
    let r = s
        .write_export(&[flagged, f.text, f.image], &f.col, &out, "0.0.1")
        .unwrap();
    assert_eq!((r.exported, r.excluded_sensitive), (1, 2));
    let a = ExportArchive::open(&out, ArchiveLimits::EXPORT).unwrap();
    assert_eq!(a.clips().len(), 1);
    assert_eq!(a.clips()[0].id, f.image);
    let _ = tmp;
}

#[test]
fn an_export_its_own_import_would_refuse_is_not_written() {
    let (_tmp, s, f, out) = made();
    for limits in [
        ArchiveLimits {
            max_entries: 2,
            ..ArchiveLimits::EXPORT
        },
        ArchiveLimits {
            max_total_bytes: 32,
            ..ArchiveLimits::EXPORT
        },
        ArchiveLimits {
            max_json_bytes: 64,
            ..ArchiveLimits::EXPORT
        },
    ] {
        assert_eq!(
            s.write_export_with(&[f.text, f.image], &f.col, &out, "0.0.1", limits),
            Err(StorageError::Archive(ArchiveError::TooLargeToWrite)),
            "{limits:?}"
        );
        assert!(!out.exists());
    }
}

#[test]
fn a_shared_blob_counts_once_against_the_total() {
    let (_tmp, mut s, f, out) = made();
    // A second clip with the same picture: one blob in the file, read twice by an import.
    let mut img = raw(Mime::ImagePng, &f.png);
    img.width = Some(1);
    img.height = Some(1);
    let twin = ins(
        &mut s,
        new_clip(vec![raw(Mime::TextPlain, b"caption"), img]),
    );
    s.write_export(&[f.image, twin], &f.col, &out, "0.0.1")
        .unwrap();
    let json_len = entries_of(&out)
        .into_iter()
        .find(|(n, _)| n == "export.json")
        .unwrap()
        .1
        .len() as u64;
    let limits = ArchiveLimits {
        max_total_bytes: json_len + f.png.len() as u64 + 16,
        ..ArchiveLimits::EXPORT
    };
    let mut a = ExportArchive::open(&out, limits).unwrap();
    let sha = a.clips()[0].representations[0].blob_sha256.unwrap();
    assert_eq!(a.blob_size(&sha), Some(f.png.len() as u64));
    assert_eq!(a.blob_size(&[1; 32]), None);
    assert_eq!(a.blob(&sha).unwrap(), f.png);
    assert_eq!(
        a.blob(&sha).unwrap(),
        f.png,
        "a second read is not counted again"
    );
}

#[test]
fn migration_3_adds_the_no_export_flag_to_a_v2_store() {
    let tmp = tempfile::tempdir().unwrap();
    let id = "01900000-0000-7000-8000-000000000002";
    {
        let v2 = &crate::migrate::MIGRATIONS[..2];
        let s = crate::db::open_with(tmp.path(), crate::OpenMode::ReadWrite, clock(), v2).unwrap();
        assert_eq!(s.schema_version().unwrap(), 2);
        s.conn
            .execute(
                "INSERT INTO clips (id, kind, preview, content_hash, size_bytes, created_at, \
                 last_copied_at) VALUES (?1, 'text', 'kept', ?2, 4, ?3, ?3)",
                rusqlite::params![id, [5u8; 32], T0],
            )
            .unwrap();
        s.conn
            .execute_batch(
                "INSERT INTO representations (clip_id, ord, mime, size_bytes, inline_data) \
                 SELECT id, 0, 'text/plain;charset=utf-8', 4, CAST('kept' AS BLOB) FROM clips; \
                 INSERT INTO clip_rowid (clip_id) SELECT id FROM clips;",
            )
            .unwrap();
    }
    let s = open(tmp.path(), clock());
    assert_eq!(s.schema_version().unwrap(), 3);
    assert!(tmp.path().join("history.db.pre-v3.bak").is_file());
    let cid = ClipId::parse(id).unwrap();
    assert!(
        !s.is_no_export(&cid).unwrap(),
        "clips from before carry no flag"
    );
    assert_eq!(s.read_representation(&cid, 0).unwrap(), b"kept");
    drop(s);
    let s = open(tmp.path(), clock());
    assert!(!tmp.path().join("history.db.pre-v3.bak").exists());
    assert_eq!(s.schema_version().unwrap(), 3);
}
