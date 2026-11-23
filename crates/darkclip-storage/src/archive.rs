//! Zip archives for backups (`.dcbackup`) and exports (`.dcexport`), Task 405.
//!
//! Every archive that is read is untrusted input. Reading enforces, before anything is
//! written anywhere:
//! - the archive file size, the entry count, every entry's size and the total uncompressed
//!   size (sizes are counted while decompressing, never taken from the headers alone);
//! - entry names from a fixed allow-list (`manifest.json`, `history.db`, `export.json`,
//!   `blobs/<aa>/<64 hex>` with `aa` the first two digits): no directories, no absolute paths,
//!   no `..`, no drive letters, no backslashes, no duplicates, no symlinks, no encryption;
//! - the SHA-256 and size of every file against its manifest entry.
//!
//! A name from an archive is never used as a path: files are written under paths this module
//! builds from the validated SHA-256 only.
//!
//! Errors carry a reason code only, never a name, a path or content (GC-06).

use crate::blobs;
use crate::StorageError;
use sha2::{Digest, Sha256};
use std::fs::File;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

/// Backup format written and accepted (`manifest.json` `format`).
pub const BACKUP_FORMAT: u32 = 1;
/// `export.json` `schema_version` written and accepted (`export.schema.json`).
pub const EXPORT_SCHEMA_VERSION: u32 = 1;

pub(crate) const MANIFEST: &str = "manifest.json";
pub(crate) const HISTORY_DB: &str = "history.db";
pub(crate) const EXPORT_JSON: &str = "export.json";

/// Why an archive was refused. Codes only (GC-06).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArchiveError {
    /// Not a readable zip file, or a required entry is missing.
    NotAnArchive,
    /// An entry name outside the allow-list (absolute, `..`, drive letter, directory, ...).
    BadEntryName,
    DuplicateEntry,
    TooManyEntries,
    /// The archive, an entry or the total uncompressed size is over its limit.
    TooLarge,
    /// A file does not match the size or SHA-256 its manifest names.
    ChecksumMismatch,
    /// The manifest or `export.json` is malformed, or does not match the archive's files.
    BadManifest,
    /// Written by a newer Dark Clipboard (`format` or `schema_version`).
    NewerFormat,
    /// The database in a backup fails its integrity, schema or content checks.
    Corrupt,
    /// The archive uses a feature that is never accepted (encryption, symlinks, other compression).
    Unsupported,
}

impl std::fmt::Display for ArchiveError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::NotAnArchive => "not a valid archive",
            Self::BadEntryName => "archive entry name not allowed",
            Self::DuplicateEntry => "duplicate archive entry",
            Self::TooManyEntries => "too many archive entries",
            Self::TooLarge => "archive too large",
            Self::ChecksumMismatch => "archive file checksum mismatch",
            Self::BadManifest => "archive manifest invalid",
            Self::NewerFormat => "archive written by a newer version",
            Self::Corrupt => "archive database invalid",
            Self::Unsupported => "archive feature not supported",
        })
    }
}

impl From<ArchiveError> for StorageError {
    fn from(e: ArchiveError) -> Self {
        Self::Archive(e)
    }
}

/// Bounds for reading an archive. `BACKUP` and `EXPORT` are the real ones; tests pass
/// smaller values.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ArchiveLimits {
    /// Entries in the zip, manifest included.
    pub max_entries: usize,
    /// Uncompressed size of any one entry.
    pub max_entry_bytes: u64,
    /// Uncompressed size of all entries together (also bounds the archive file itself).
    pub max_total_bytes: u64,
    /// Size of `manifest.json` or `export.json`.
    pub max_json_bytes: u64,
}

const MIB: u64 = 1024 * 1024;

impl ArchiveLimits {
    /// A backup: the database (at most 4 GiB) plus up to 200,000 blobs (at most 256 MiB each,
    /// 8 GiB together).
    pub const BACKUP: Self = Self {
        max_entries: 200_002,
        max_entry_bytes: 4 * 1024 * MIB,
        max_total_bytes: 8 * 1024 * MIB,
        max_json_bytes: 64 * MIB,
    };
    /// An export: up to 50,000 blobs (at most 256 MiB each, 2 GiB together).
    pub const EXPORT: Self = Self {
        max_entries: 50_001,
        max_entry_bytes: 256 * MIB,
        max_total_bytes: 2 * 1024 * MIB,
        max_json_bytes: 64 * MIB,
    };
}

/// What an allowed entry name stands for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum EntryKind {
    Manifest,
    HistoryDb,
    ExportJson,
    Blob([u8; 32]),
}

pub(crate) fn parse_hex32(s: &str) -> Option<[u8; 32]> {
    if s.len() != 64 || !s.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f')) {
        return None;
    }
    let mut out = [0u8; 32];
    for (slot, pair) in out.iter_mut().zip(s.as_bytes().chunks(2)) {
        let pair = std::str::from_utf8(pair).ok()?;
        *slot = u8::from_str_radix(pair, 16).ok()?;
    }
    Some(out)
}

/// The archive name of a blob: `blobs/<aa>/<64 hex>`.
pub(crate) fn blob_entry_name(sha: &[u8; 32]) -> String {
    let h = blobs::hex(sha);
    format!("blobs/{}/{h}", h.get(..2).unwrap_or("00"))
}

/// Classifies an entry name against the allow-list. Anything else is `None`. The check is
/// exact (byte for byte), so `..`, absolute paths, drive letters, backslashes, `./` and
/// trailing slashes are all refused.
pub(crate) fn classify_entry(name: &str) -> Option<EntryKind> {
    match name {
        MANIFEST => return Some(EntryKind::Manifest),
        HISTORY_DB => return Some(EntryKind::HistoryDb),
        EXPORT_JSON => return Some(EntryKind::ExportJson),
        _ => {}
    }
    let rest = name.strip_prefix("blobs/")?;
    let (shard, file) = rest.split_once('/')?;
    let sha = parse_hex32(file)?;
    (shard.len() == 2 && file.get(..2) == Some(shard)).then_some(EntryKind::Blob(sha))
}

/// An opened archive whose entry names, count and file size passed the checks.
pub(crate) struct CheckedZip {
    zip: zip::ZipArchive<File>,
    /// Allowed entries, by kind, with their index in the zip.
    pub entries: Vec<(EntryKind, usize)>,
    pub limits: ArchiveLimits,
    /// Uncompressed bytes read so far (bounded by `limits.max_total_bytes`).
    read_total: u64,
}

impl CheckedZip {
    /// Opens `path` and checks every entry name, the count and the file size. `allowed`
    /// says which non-blob kinds this archive type may contain; blobs are always allowed.
    pub fn open(
        path: &Path,
        limits: ArchiveLimits,
        allowed: &[EntryKind],
    ) -> Result<Self, StorageError> {
        let file = File::open(path)?;
        let len = file.metadata()?.len();
        // The compressed archive cannot usefully be larger than everything it may hold.
        if len > limits.max_total_bytes.saturating_add(limits.max_json_bytes) {
            return Err(ArchiveError::TooLarge.into());
        }
        let mut zip = zip::ZipArchive::new(file).map_err(|_| ArchiveError::NotAnArchive)?;
        if zip.len() > limits.max_entries {
            return Err(ArchiveError::TooManyEntries.into());
        }
        let mut entries = Vec::with_capacity(zip.len());
        let mut seen = std::collections::HashSet::new();
        for i in 0..zip.len() {
            let f = zip
                .by_index_raw(i)
                .map_err(|_| ArchiveError::NotAnArchive)?;
            let name = f.name().map_err(|_| ArchiveError::BadEntryName)?;
            let kind = classify_entry(&name).ok_or(ArchiveError::BadEntryName)?;
            let blob = matches!(kind, EntryKind::Blob(_));
            if !blob && !allowed.contains(&kind) {
                return Err(ArchiveError::BadEntryName.into());
            }
            if !seen.insert(kind) {
                return Err(ArchiveError::DuplicateEntry.into());
            }
            if f.encrypted() || f.is_symlink() || f.is_dir() {
                return Err(ArchiveError::Unsupported.into());
            }
            if !matches!(
                f.compression(),
                zip::CompressionMethod::Stored | zip::CompressionMethod::Deflated
            ) {
                return Err(ArchiveError::Unsupported.into());
            }
            let cap = if blob || kind == EntryKind::HistoryDb {
                limits.max_entry_bytes
            } else {
                limits.max_json_bytes
            };
            // A header that already claims too much is refused before any decompression.
            if f.size() > cap {
                return Err(ArchiveError::TooLarge.into());
            }
            drop(f);
            entries.push((kind, i));
        }
        Ok(Self {
            zip,
            entries,
            limits,
            read_total: 0,
        })
    }

    pub fn index_of(&self, kind: EntryKind) -> Option<usize> {
        self.entries
            .iter()
            .find(|(k, _)| *k == kind)
            .map(|(_, i)| *i)
    }

    /// Reads a JSON entry (at most `max_json_bytes`) into memory.
    pub fn read_json(&mut self, kind: EntryKind) -> Result<Vec<u8>, StorageError> {
        let idx = self.index_of(kind).ok_or(ArchiveError::NotAnArchive)?;
        let cap = self.limits.max_json_bytes;
        let mut out = Vec::new();
        self.copy_entry(idx, cap, &mut out)?;
        Ok(out)
    }

    /// Reads entry `idx` into memory after checking its size and SHA-256.
    pub fn read_verified(
        &mut self,
        idx: usize,
        size: u64,
        sha: &[u8; 32],
    ) -> Result<Vec<u8>, StorageError> {
        let mut out = Vec::new();
        let (n, got) = self.copy_entry(idx, size.min(self.limits.max_entry_bytes), &mut out)?;
        if n != size || got != *sha {
            return Err(ArchiveError::ChecksumMismatch.into());
        }
        Ok(out)
    }

    /// Streams entry `idx` into `target` (a new private file) after checking its size and
    /// SHA-256 on the way. A mismatch removes the file.
    pub fn extract_verified(
        &mut self,
        idx: usize,
        size: u64,
        sha: &[u8; 32],
        target: &Path,
    ) -> Result<(), StorageError> {
        let result = (|| {
            let mut f = blobs::create_private_file(target, true)?;
            let (n, got) = self.copy_entry(idx, size.min(self.limits.max_entry_bytes), &mut f)?;
            if n != size || got != *sha {
                return Err(ArchiveError::ChecksumMismatch.into());
            }
            f.sync_all()?;
            Ok(())
        })();
        if result.is_err() {
            let _ = std::fs::remove_file(target);
        }
        result
    }

    /// Copies at most `cap` bytes of entry `idx` to `out`; one byte more is an error. Returns
    /// the byte count and the SHA-256 of what was copied. Counts toward the total limit.
    fn copy_entry(
        &mut self,
        idx: usize,
        cap: u64,
        out: &mut dyn Write,
    ) -> Result<(u64, [u8; 32]), StorageError> {
        let total_left = self.limits.max_total_bytes.saturating_sub(self.read_total);
        let entry = self
            .zip
            .by_index(idx)
            .map_err(|_| ArchiveError::NotAnArchive)?;
        let mut limited = entry.take(cap.saturating_add(1));
        let mut hasher = Sha256::new();
        let mut buf = vec![0u8; 64 * 1024];
        let mut n: u64 = 0;
        loop {
            let k = match limited.read(&mut buf) {
                Ok(0) => break,
                Ok(k) => k,
                Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
                // A corrupt stream (bad deflate data, CRC failure) is the archive's fault.
                Err(_) => return Err(ArchiveError::NotAnArchive.into()),
            };
            n = n.saturating_add(k as u64);
            if n > cap {
                return Err(ArchiveError::TooLarge.into());
            }
            if n > total_left {
                return Err(ArchiveError::TooLarge.into());
            }
            let chunk = buf.get(..k).unwrap_or_default();
            hasher.update(chunk);
            out.write_all(chunk)?;
        }
        self.read_total = self.read_total.saturating_add(n);
        Ok((n, hasher.finalize().into()))
    }
}

/// Writes a zip to `<dest>.partial` and renames it over `dest` once complete and flushed.
/// A failure removes the partial file; `dest` is never left half-written.
pub(crate) struct AtomicZip {
    zip: Option<zip::ZipWriter<File>>,
    partial: PathBuf,
    dest: PathBuf,
}

impl AtomicZip {
    pub fn create(dest: &Path) -> Result<Self, StorageError> {
        let mut partial = dest.as_os_str().to_owned();
        partial.push(".partial");
        let partial = PathBuf::from(partial);
        let file = blobs::create_private_file(&partial, true)?;
        Ok(Self {
            zip: Some(zip::ZipWriter::new(file)),
            partial,
            dest: dest.to_path_buf(),
        })
    }

    fn writer(&mut self) -> Result<&mut zip::ZipWriter<File>, StorageError> {
        self.zip
            .as_mut()
            .ok_or(StorageError::Io(std::io::ErrorKind::Other))
    }

    fn start(&mut self, name: &str, len: u64) -> Result<(), StorageError> {
        let opts = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated)
            .large_file(len >= u64::from(u32::MAX));
        self.writer()?
            .start_file(name, opts)
            .map_err(|_| StorageError::Io(std::io::ErrorKind::Other))
    }

    /// Adds `bytes` as `name`.
    pub fn add_bytes(&mut self, name: &str, bytes: &[u8]) -> Result<(), StorageError> {
        self.start(name, bytes.len() as u64)?;
        self.writer()?.write_all(bytes)?;
        Ok(())
    }

    /// Adds the file at `path` as `name`; returns its size and SHA-256.
    pub fn add_file(&mut self, name: &str, path: &Path) -> Result<(u64, [u8; 32]), StorageError> {
        let mut f = File::open(path)?;
        let len = f.metadata()?.len();
        self.start(name, len)?;
        let w = self.writer()?;
        let mut hasher = Sha256::new();
        let mut buf = vec![0u8; 64 * 1024];
        let mut n = 0u64;
        loop {
            let k = match f.read(&mut buf) {
                Ok(0) => break,
                Ok(k) => k,
                Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
                Err(e) => return Err(e.into()),
            };
            let chunk = buf.get(..k).unwrap_or_default();
            hasher.update(chunk);
            w.write_all(chunk)?;
            n = n.saturating_add(k as u64);
        }
        Ok((n, hasher.finalize().into()))
    }

    /// Finishes the zip, flushes it and renames it into place.
    pub fn commit(mut self) -> Result<(), StorageError> {
        let zip = self
            .zip
            .take()
            .ok_or(StorageError::Io(std::io::ErrorKind::Other))?;
        let file = zip
            .finish()
            .map_err(|_| StorageError::Io(std::io::ErrorKind::Other))?;
        file.sync_all()?;
        drop(file);
        std::fs::rename(&self.partial, &self.dest)?;
        Ok(())
    }
}

impl Drop for AtomicZip {
    /// Not committed: the partial file is removed (best-effort).
    fn drop(&mut self) {
        if let Some(zip) = self.zip.take() {
            drop(zip);
            let _ = std::fs::remove_file(&self.partial);
        }
    }
}

/// A fresh private directory `<parent>/<prefix><n>` that did not exist before.
pub(crate) fn fresh_dir(parent: &Path, prefix: &str, seed: i64) -> Result<PathBuf, StorageError> {
    for n in 0..1000i64 {
        let dir = parent.join(format!("{prefix}{}", seed.saturating_add(n)));
        match std::fs::create_dir(&dir) {
            Ok(()) => {
                blobs::ensure_private_dir(&dir)?;
                return Ok(dir);
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(e.into()),
        }
    }
    Err(StorageError::Io(std::io::ErrorKind::AlreadyExists))
}

/// Removes a staging directory and everything in it (best-effort).
pub(crate) fn remove_dir(dir: &Path) {
    let _ = std::fs::remove_dir_all(dir);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn entry_names_follow_the_allow_list() {
        let sha = "ab".repeat(32);
        assert_eq!(classify_entry("manifest.json"), Some(EntryKind::Manifest));
        assert_eq!(classify_entry("history.db"), Some(EntryKind::HistoryDb));
        assert_eq!(classify_entry("export.json"), Some(EntryKind::ExportJson));
        assert!(matches!(
            classify_entry(&format!("blobs/ab/{sha}")),
            Some(EntryKind::Blob(_))
        ));
        for bad in [
            format!("blobs/cd/{sha}"),
            format!("blobs/ab/{}", "AB".repeat(32)),
            format!("blobs/ab/{sha}/"),
            format!("blobs/ab/../{sha}"),
            format!("/blobs/ab/{sha}"),
            format!("blobs\\ab\\{sha}"),
            "./manifest.json".to_string(),
            "../history.db".to_string(),
            "C:/history.db".to_string(),
            "C:\\x".to_string(),
            "/etc/passwd".to_string(),
            "blobs/".to_string(),
            "history.db/".to_string(),
            "Manifest.json".to_string(),
            String::new(),
        ] {
            assert_eq!(classify_entry(&bad), None, "{bad}");
        }
    }
}
