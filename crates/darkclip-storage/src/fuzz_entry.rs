//! Fuzz entry points (Task 406). Built only for tests and with the `fuzzing` feature (the
//! `fuzz/` cargo-fuzz targets enable it); never part of a normal build.
//!
//! Each function takes arbitrary bytes, drives the backup/import validation of Task 405 and
//! checks its invariants with `assert!`; the messages never include the input (GC-06). The
//! archive targets read the bytes from memory (`CheckedZip::from_reader`, the same checks
//! `open` runs on a file) and run the same steps as a restore or an import up to, not
//! including, the database checks and the live swap; the database extraction writes a
//! scratch file in the system temp folder. The `#[ignore]` stable harness in `harness` calls
//! the same functions.

use crate::archive::{
    blob_entry_name, classify_entry, parse_hex32, ArchiveLimits, CheckedZip, EntryKind,
};
use crate::backup::{expected_files, parse_manifest};
use crate::transfer::ExportArchive;
use crate::{migrate, StorageError};
use std::io::Cursor;
use std::path::PathBuf;

/// Small limits, so the size, count and budget checks trip on small inputs.
const SMALL: ArchiveLimits = ArchiveLimits {
    max_entries: 16,
    max_entry_bytes: 64 * 1024,
    max_total_bytes: 256 * 1024,
    max_json_bytes: 16 * 1024,
};

fn limits(sel: u8, real: ArchiveLimits) -> ArchiveLimits {
    if sel & 0x0F == 0 {
        real
    } else {
        SMALL
    }
}

fn split(data: &[u8]) -> (u8, &[u8]) {
    match data.split_first() {
        Some((&sel, rest)) => (sel, rest),
        None => (0, &[]),
    }
}

/// A scratch path unique to this process and thread.
fn scratch(suffix: &str) -> PathBuf {
    let thread = format!("{:?}", std::thread::current().id());
    let thread: String = thread.chars().filter(char::is_ascii_digit).collect();
    std::env::temp_dir().join(format!(
        "darkclip-fuzz-{}-{thread}.{suffix}",
        std::process::id()
    ))
}

/// Removes this thread's scratch files.
pub fn remove_scratch() {
    let _ = std::fs::remove_file(scratch("db"));
}

/// Entry-name allow-list and hex parsing.
pub fn archive_names(data: &[u8]) {
    let name = String::from_utf8_lossy(data);
    if let Some(sha) = parse_hex32(&name) {
        assert_eq!(crate::blobs::hex(&sha), name, "hex round trip");
    }
    match classify_entry(&name) {
        Some(EntryKind::Blob(sha)) => {
            assert_eq!(blob_entry_name(&sha), name, "blob name is canonical");
        }
        Some(EntryKind::Manifest | EntryKind::HistoryDb | EntryKind::ExportJson) => {
            assert!(
                matches!(&*name, "manifest.json" | "history.db" | "export.json"),
                "fixed names only"
            );
        }
        None => {}
    }
    if classify_entry(&name).is_some() {
        assert!(
            !name.contains("..") && !name.contains('\\') && !name.starts_with('/'),
            "no path tricks"
        );
        assert!(
            !name.contains(':') && !name.ends_with('/'),
            "no drive or directory"
        );
    }
}

/// `manifest.json` parsing on its own (no archive).
pub fn backup_manifest(data: &[u8]) {
    let supported = migrate::target_version(migrate::MIGRATIONS);
    match parse_manifest(data, supported) {
        Ok(_) | Err(StorageError::Archive(_) | StorageError::SchemaTooNew { .. }) => {}
        Err(_) => assert_unexpected_error(),
    }
}

#[allow(clippy::panic)] // Fuzz-only: a finding must fail.
fn assert_unexpected_error() {
    panic!("unexpected error kind")
}

/// A `.dcbackup` as a restore reads it: open and check the zip, parse the manifest, match
/// it against the entries, then read every listed file with its size and SHA-256 checked.
/// First byte: low nibble 0 selects the real limits (else small ones); bit 4 also extracts
/// the database to a scratch file the way restore stages it.
pub fn backup_archive(data: &[u8]) {
    let (sel, zip_bytes) = split(data);
    let limits = limits(sel, ArchiveLimits::BACKUP);
    let allowed = [EntryKind::Manifest, EntryKind::HistoryDb];
    let len = zip_bytes.len() as u64;
    let Ok(mut z) = CheckedZip::from_reader(Cursor::new(zip_bytes), len, limits, &allowed) else {
        return;
    };
    assert!(z.entries.len() <= limits.max_entries, "entry count");
    let Ok(raw) = z.read_json(EntryKind::Manifest) else {
        return;
    };
    assert!(raw.len() as u64 <= limits.max_json_bytes, "json cap");
    let supported = migrate::target_version(migrate::MIGRATIONS);
    let Ok(manifest) = parse_manifest(&raw, supported) else {
        return;
    };
    let Ok(expected) = expected_files(&manifest, &z) else {
        return;
    };
    let mut read_total: u64 = 0;
    for (kind, idx) in z.entries.clone() {
        let Some((size, sha)) = expected.get(&kind).copied() else {
            assert_eq!(kind, EntryKind::Manifest, "every other entry is listed");
            continue;
        };
        if kind == EntryKind::HistoryDb && sel & 0x10 != 0 {
            let db = scratch("db");
            let _ = std::fs::remove_file(&db);
            let r = z.extract_verified(idx, size, &sha, &db);
            assert_eq!(r.is_ok(), db.exists(), "a failed extraction leaves no file");
            if r.is_ok() {
                read_total += size;
            }
            let _ = std::fs::remove_file(&db);
            continue;
        }
        if let Ok(bytes) = z.read_verified(idx, size, &sha, true) {
            assert_eq!(bytes.len() as u64, size, "verified size");
            read_total += size;
        }
    }
    assert!(read_total <= limits.max_total_bytes, "total read");
}

/// A `.dcexport` as an import reads it: open (names, sizes, the JSON document, the blob
/// list), then read every blob, some twice (the re-read budget).
pub fn export_archive(data: &[u8]) {
    let (sel, zip_bytes) = split(data);
    let limits = limits(sel, ArchiveLimits::EXPORT);
    let len = zip_bytes.len() as u64;
    let Ok(zip) = CheckedZip::from_reader(
        Cursor::new(zip_bytes),
        len,
        limits,
        &[EntryKind::ExportJson],
    ) else {
        return;
    };
    let Ok(mut export) = ExportArchive::from_zip(zip, limits) else {
        return;
    };
    assert!(export.clips().len() <= limits.max_entries, "clip count");
    let _ = export.collection();
    let shas = export.blob_shas();
    for (i, sha) in shas.iter().enumerate() {
        let size = export.blob_size(sha);
        assert!(
            size.is_some_and(|s| s <= limits.max_entry_bytes),
            "declared size"
        );
        let reads = if i % 2 == 0 { 2 } else { 1 };
        for _ in 0..reads {
            if let Ok(bytes) = export.blob(sha) {
                assert_eq!(Some(bytes.len() as u64), size, "verified size");
            }
        }
    }
}

#[cfg(test)]
mod harness;
