//! Stable fuzz harness for backup and import validation (Task 406). `#[ignore]`: run on
//! demand, in release mode, as documented in `docs/testing/fuzzing.md`. Every input is
//! synthetic; scratch files live in the system temp folder and are removed at the end.

use super::*;
use crate::archive::BACKUP_FORMAT;
use darkclip_fuzzgen::{Fuzz, Rng, TrackingAlloc};
use sha2::{Digest, Sha256};
use std::io::{Cursor, Write};
use std::time::Duration;
use zip::write::SimpleFileOptions;
use zip::CompressionMethod;

#[global_allocator]
static ALLOC: TrackingAlloc = TrackingAlloc;

const MIB: usize = 1024 * 1024;

const BAD_NAMES: [&str; 12] = [
    "../manifest.json",
    "/history.db",
    "C:/x",
    "blobs\\aa\\x",
    "blobs/aa/../../x",
    "blobs/",
    "blobs/zz/zz00",
    "./manifest.json",
    "Manifest.json",
    "export.json/",
    "blobs/AB/AB00000000000000000000000000000000000000000000000000000000000000",
    "",
];

fn sha(bytes: &[u8]) -> [u8; 32] {
    Sha256::digest(bytes).into()
}

fn hex(sha: &[u8; 32]) -> String {
    crate::blobs::hex(sha)
}

/// Random file entries: their bytes, the name they get in the zip.
fn blob_entries(rng: &mut Rng) -> Vec<(String, Vec<u8>)> {
    (0..rng.below(5))
        .map(|_| {
            let n = rng.below(1024);
            let mut bytes = rng.bytes(n);
            if rng.one_in(3) {
                // Compressible content, so deflate entries are smaller than they claim.
                bytes = vec![b'a'; n * 8];
            }
            (blob_entry_name(&sha(&bytes)), bytes)
        })
        .collect()
}

/// Zips `entries` (stored or deflated at random), sometimes adding a disallowed name or a
/// directory entry. Never fails: the writer's own errors just end the archive early.
fn zip_of(rng: &mut Rng, entries: &[(String, Vec<u8>)]) -> Vec<u8> {
    let mut z = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for (name, bytes) in entries {
        let method = if rng.one_in(2) {
            CompressionMethod::Stored
        } else {
            CompressionMethod::Deflated
        };
        let opts = SimpleFileOptions::default().compression_method(method);
        if z.start_file(name.as_str(), opts).is_err() || z.write_all(bytes).is_err() {
            break;
        }
    }
    if rng.one_in(12) {
        let name = BAD_NAMES[rng.below(BAD_NAMES.len())];
        if z.start_file(name, SimpleFileOptions::default()).is_ok() {
            let _ = z.write_all(b"x");
        }
    }
    if rng.one_in(24) {
        let _ = z.add_directory("blobs/", SimpleFileOptions::default());
    }
    z.finish().map(Cursor::into_inner).unwrap_or_default()
}

/// A backup: `history.db`, blobs and a manifest that mostly matches them.
fn gen_backup(rng: &mut Rng) -> Vec<u8> {
    let mut files = blob_entries(rng);
    let db_len = rng.below(2048);
    let mut db = b"SQLite format 3\0".to_vec();
    db.extend(rng.bytes(db_len));
    files.insert(0, ("history.db".to_string(), db));
    let supported = migrate::target_version(migrate::MIGRATIONS);
    let mut listed: Vec<serde_json::Value> = files
        .iter()
        .map(|(name, bytes)| {
            let mut size = bytes.len() as u64;
            let mut digest = hex(&sha(bytes));
            match rng.below(24) {
                0 => size += 1,
                1 => digest = hex(&[7; 32]),
                2 => digest = digest.to_uppercase(),
                _ => {}
            }
            serde_json::json!({ "path": name, "size": size, "sha256": digest })
        })
        .collect();
    match rng.below(16) {
        0 => {
            listed.pop();
        }
        1 => {
            if let Some(first) = listed.first().cloned() {
                listed.push(first);
            }
        }
        2 => listed.push(serde_json::json!({
            "path": "../escape", "size": 1, "sha256": hex(&[1; 32])
        })),
        _ => {}
    }
    let mut manifest = serde_json::json!({
        "format": if rng.one_in(16) { BACKUP_FORMAT + 1 } else { BACKUP_FORMAT },
        "schema_version": match rng.below(16) { 0 => 0, 1 => supported + 1, _ => supported },
        "app_version": "0.0.1",
        "created_at": "2026-11-24T00:00:00.000Z",
        "clip_count": rng.below(100),
        "files": listed,
    });
    if rng.one_in(16) {
        manifest["extra"] = serde_json::json!(true);
    }
    let raw = serde_json::to_vec(&manifest).unwrap_or_default();
    let at = if rng.one_in(2) { 0 } else { files.len() };
    files.insert(at, ("manifest.json".to_string(), raw));
    let mut out = vec![rng.next_u64() as u8];
    out.extend(zip_of(rng, &files));
    out
}

/// An export: `export.json` (the documented collection and clip examples) and blobs.
fn gen_export(rng: &mut Rng) -> Vec<u8> {
    let collection: serde_json::Value = serde_json::from_str(include_str!(
        "../../../../docs/architecture/examples/collection.example.json"
    ))
    .unwrap_or_default();
    let clip_text: serde_json::Value = serde_json::from_str(include_str!(
        "../../../../docs/architecture/examples/clip.text.example.json"
    ))
    .unwrap_or_default();
    let clip_image: serde_json::Value = serde_json::from_str(include_str!(
        "../../../../docs/architecture/examples/clip.image.example.json"
    ))
    .unwrap_or_default();
    let mut files = blob_entries(rng);
    let mut blobs: Vec<serde_json::Value> = files
        .iter()
        .map(|(name, bytes)| {
            let digest = sha(bytes);
            let size = bytes.len() as u64 + u64::from(rng.one_in(24));
            let path = if rng.one_in(24) {
                "blobs/00/x".to_string()
            } else {
                name.clone()
            };
            serde_json::json!({ "sha256": hex(&digest), "path": path, "size_bytes": size })
        })
        .collect();
    if rng.one_in(16) {
        blobs.pop();
    }
    let clips: Vec<serde_json::Value> = (0..rng.below(4))
        .map(|_| {
            if rng.one_in(2) {
                clip_text.clone()
            } else {
                clip_image.clone()
            }
        })
        .collect();
    let doc = serde_json::json!({
        "schema_version": match rng.below(16) { 0 => 2, 1 => 0, _ => 1 },
        "app_version": "0.0.1",
        "created_at": "2026-11-24T00:00:00.000Z",
        "collection": collection,
        "clips": clips,
        "blobs": blobs,
    });
    let raw = serde_json::to_vec(&doc).unwrap_or_default();
    files.insert(0, ("export.json".to_string(), raw));
    let mut out = vec![rng.next_u64() as u8];
    out.extend(zip_of(rng, &files));
    out
}

/// An entry name: allowed names, near misses and path tricks.
fn gen_name(rng: &mut Rng) -> Vec<u8> {
    match rng.below(4) {
        0 => BAD_NAMES[rng.below(BAD_NAMES.len())].as_bytes().to_vec(),
        1 => ["manifest.json", "history.db", "export.json"][rng.below(3)]
            .as_bytes()
            .to_vec(),
        _ => {
            let n = rng.below(64);
            let s = sha(&rng.bytes(n));
            blob_entry_name(&s).into_bytes()
        }
    }
}

/// A manifest on its own (taken from a generated backup's JSON).
fn gen_manifest(rng: &mut Rng) -> Vec<u8> {
    let zip_bytes = gen_backup(rng);
    let Ok(mut z) = zip::ZipArchive::new(Cursor::new(zip_bytes.get(1..).unwrap_or_default()))
    else {
        return b"{}".to_vec();
    };
    let Ok(mut f) = z.by_name("manifest.json") else {
        return b"{}".to_vec();
    };
    let mut out = Vec::new();
    let _ = std::io::Read::read_to_end(&mut f, &mut out);
    out
}

const ZIP_DICT: &[&[u8]] = &[
    b"PK\x03\x04",
    b"PK\x01\x02",
    b"PK\x05\x06",
    b"PK\x06\x06",
    b"PK\x06\x07",
    b"manifest.json",
    b"history.db",
    b"export.json",
    b"blobs/",
    b"../",
    b"\\",
    b"\xff\xff\xff\xff",
    b"\x08\x00",
    b"\x63\x00",
    b"\"size\":",
    b"\"sha256\":\"",
    b"\"files\":[",
    b"\"format\":1",
    b"\"schema_version\":",
    b"{\"a\":",
    b"[[[[",
];

fn fuzz<'a>(name: &'static str, max_len: usize, mem: usize) -> Fuzz<'a> {
    Fuzz {
        name,
        max_len,
        dict: ZIP_DICT,
        mem_budget: mem,
        time_limit: Duration::from_secs(10),
        fixup: None,
    }
}

#[test]
#[ignore = "fuzz harness: run on demand in release mode (docs/testing/fuzzing.md)"]
fn fuzz_archive_names() {
    fuzz("archive_names", 256, MIB).run(gen_name, archive_names);
}

#[test]
#[ignore = "fuzz harness: run on demand in release mode (docs/testing/fuzzing.md)"]
fn fuzz_backup_manifest() {
    fuzz("backup_manifest", 16 * 1024, 8 * MIB).run(gen_manifest, backup_manifest);
}

#[test]
#[ignore = "fuzz harness: run on demand in release mode (docs/testing/fuzzing.md)"]
fn fuzz_backup_archive() {
    // With the real limits a 64 KiB deflate input may legitimately expand to about 66 MB
    // (deflate tops out near 1:1032), read into a growing buffer (by design: JSON up to
    // 64 MiB, blobs up to their declared size are read into memory).
    let r = std::panic::catch_unwind(|| {
        fuzz("backup_archive", 64 * 1024, 300 * MIB).run(gen_backup, backup_archive)
    });
    remove_scratch();
    if let Err(e) = r {
        std::panic::resume_unwind(e);
    }
}

#[test]
#[ignore = "fuzz harness: run on demand in release mode (docs/testing/fuzzing.md)"]
fn fuzz_export_archive() {
    // As for backups.
    let r = std::panic::catch_unwind(|| {
        fuzz("export_archive", 64 * 1024, 300 * MIB).run(gen_export, export_archive)
    });
    remove_scratch();
    if let Err(e) = r {
        std::panic::resume_unwind(e);
    }
}

/// Writes the committed seed corpus (`fuzz/corpus/<target>/`). Run by hand when the
/// generators change; the files are small and synthetic.
#[test]
#[ignore = "writes fuzz/corpus; run by hand"]
fn write_seed_corpus() {
    type Gen = fn(&mut Rng) -> Vec<u8>;
    let gens: [(&str, Gen); 4] = [
        ("archive_names", gen_name),
        ("backup_manifest", gen_manifest),
        ("backup_archive", gen_backup),
        ("export_archive", gen_export),
    ];
    for (i, (name, gen)) in gens.into_iter().enumerate() {
        let mut rng = Rng::new(300 + i as u64);
        let inputs: Vec<Vec<u8>> = (0..6).map(|_| gen(&mut rng)).collect();
        darkclip_fuzzgen::write_corpus(name, &inputs).unwrap();
    }
}

/// The generators must reach past the zip and manifest checks, or the runs above only
/// exercise the first rejection. Not ignored: fast and deterministic.
#[test]
fn generators_reach_the_deep_checks() {
    let mut rng = Rng::new(42);
    let supported = migrate::target_version(migrate::MIGRATIONS);
    let mut backups_ok = 0;
    let mut exports_ok = 0;
    for _ in 0..200 {
        let b = gen_backup(&mut rng);
        let zip_bytes = &b[1..];
        let allowed = [EntryKind::Manifest, EntryKind::HistoryDb];
        if let Ok(mut z) = CheckedZip::from_reader(
            Cursor::new(zip_bytes),
            zip_bytes.len() as u64,
            SMALL,
            &allowed,
        ) {
            let m = z.read_json(EntryKind::Manifest).ok();
            let m = m.and_then(|raw| parse_manifest(&raw, supported).ok());
            if m.is_some_and(|m| expected_files(&m, &z).is_ok()) {
                backups_ok += 1;
            }
        }
        let e = gen_export(&mut rng);
        let zip_bytes = &e[1..];
        let ok = CheckedZip::from_reader(
            Cursor::new(zip_bytes),
            zip_bytes.len() as u64,
            SMALL,
            &[EntryKind::ExportJson],
        )
        .and_then(|z| ExportArchive::from_zip(z, SMALL));
        if ok.is_ok() {
            exports_ok += 1;
        }
    }
    assert!(
        backups_ok >= 50,
        "backups passing the manifest checks: {backups_ok}"
    );
    assert!(
        exports_ok >= 50,
        "exports passing the document checks: {exports_ok}"
    );
}
