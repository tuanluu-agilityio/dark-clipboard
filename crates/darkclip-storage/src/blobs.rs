//! Content-addressed blob files: `<data dir>/blobs/<aa>/<sha256 hex>` (data-model §5).
//!
//! A blob is written to `<file>.tmp`, flushed, then renamed into place, always before
//! the transaction that references it commits. A crash in between leaves only an
//! unreferenced file (or a `.tmp`), which `sweep` removes at the next read-write open.
//! Unix: directories 0700, files 0600; after a rename the shard directory is fsynced
//! (and `blobs/` after a new shard), so the entry survives a power loss.
//!
//! Locking: blob files are written inside the writer's `BEGIN IMMEDIATE` transaction,
//! and both the sweep and the post-commit unlink also hold `BEGIN IMMEDIATE` while
//! they decide what is unreferenced and unlink it. So they can never remove a blob
//! that another writer has written but not yet committed.
//!
//! Cleanup (sweep, unlink of unreferenced blobs) is best-effort: a file that cannot
//! be removed is skipped and counted, and is retried at the next sweep.

use crate::StorageError;
use rusqlite::{Connection, TransactionBehavior};
use sha2::{Digest, Sha256};
use std::collections::HashSet;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

pub(crate) const BLOBS_DIR: &str = "blobs";

pub(crate) fn sha256(bytes: &[u8]) -> [u8; 32] {
    Sha256::digest(bytes).into()
}

pub(crate) fn hex(sha: &[u8; 32]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut s = String::with_capacity(64);
    for b in sha {
        s.push(char::from(DIGITS[usize::from(b >> 4)]));
        s.push(char::from(DIGITS[usize::from(b & 15)]));
    }
    s
}

fn blob_path(data_dir: &Path, sha: &[u8; 32]) -> PathBuf {
    let h = hex(sha);
    let shard = h.get(..2).unwrap_or("00").to_owned();
    data_dir.join(BLOBS_DIR).join(shard).join(h)
}

/// Creates `dir` (and missing parents) and makes `dir` itself 0700 on Unix.
pub(crate) fn ensure_private_dir(dir: &Path) -> Result<(), StorageError> {
    let mut b = fs::DirBuilder::new();
    b.recursive(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        b.mode(0o700);
    }
    b.create(dir)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(dir, fs::Permissions::from_mode(0o700))?;
    }
    Ok(())
}

/// Opens `path` for writing, created 0600 on Unix.
pub(crate) fn create_private_file(path: &Path, truncate: bool) -> Result<fs::File, StorageError> {
    let mut o = fs::OpenOptions::new();
    o.write(true).create(true).truncate(truncate);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        o.mode(0o600);
    }
    let f = o.open(path)?;
    restrict_file(path)?;
    Ok(f)
}

/// Makes an existing file 0600 on Unix; no-op elsewhere (per-user profile ACL).
pub(crate) fn restrict_file(path: &Path) -> Result<(), StorageError> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o600))?;
    }
    #[cfg(not(unix))]
    let _ = path;
    Ok(())
}

/// Flushes a directory entry table (Unix only; Windows has no directory fsync).
#[cfg(unix)]
fn sync_dir(dir: &Path) -> Result<(), StorageError> {
    fs::File::open(dir)?.sync_all()?;
    Ok(())
}

/// Whether the file at `path` is exactly the blob `sha` of length `len`.
fn is_intact(path: &Path, sha: &[u8; 32], len: usize) -> bool {
    let same_size = fs::metadata(path)
        .is_ok_and(|m| m.is_file() && u64::try_from(len).is_ok_and(|n| n == m.len()));
    same_size && fs::read(path).is_ok_and(|b| sha256(&b) == *sha)
}

/// Writes `bytes` (whose SHA-256 is `sha`) unless an intact copy already exists.
/// A same-named file with different content (corruption) is replaced.
pub(crate) fn write(data_dir: &Path, sha: &[u8; 32], bytes: &[u8]) -> Result<(), StorageError> {
    let path = blob_path(data_dir, sha);
    if is_intact(&path, sha, bytes.len()) {
        return Ok(());
    }
    let shard = path
        .parent()
        .ok_or(StorageError::Io(std::io::ErrorKind::InvalidInput))?;
    let new_shard = !shard.is_dir();
    ensure_private_dir(shard)?;
    #[cfg(unix)]
    if new_shard {
        sync_dir(&data_dir.join(BLOBS_DIR))?;
    }
    #[cfg(not(unix))]
    let _ = new_shard;
    let mut tmp = path.clone().into_os_string();
    tmp.push(".tmp");
    let tmp = PathBuf::from(tmp);
    let result = (|| {
        let mut f = create_private_file(&tmp, true)?;
        f.write_all(bytes)?;
        f.sync_all()?;
        drop(f);
        fs::rename(&tmp, &path)?;
        #[cfg(unix)]
        sync_dir(shard)?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&tmp);
    }
    result
}

/// Reads a blob and checks it still hashes to its name.
pub(crate) fn read(data_dir: &Path, sha: &[u8; 32]) -> Result<Vec<u8>, StorageError> {
    let bytes = fs::read(blob_path(data_dir, sha))?;
    if sha256(&bytes) != *sha {
        return Err(StorageError::Io(std::io::ErrorKind::InvalidData));
    }
    Ok(bytes)
}

/// Whether any representation or clip thumbnail references `sha`.
pub(crate) fn is_referenced(conn: &Connection, sha: &[u8; 32]) -> Result<bool, StorageError> {
    let referenced: bool = conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM representations WHERE blob_sha256 = ?1) \
             OR EXISTS(SELECT 1 FROM clips \
                       WHERE json_extract(meta, '$.image_thumb_sha256') = ?2)",
        rusqlite::params![sha, hex(sha)],
        |r| r.get(0),
    )?;
    Ok(referenced)
}

/// Outcome of a best-effort cleanup pass. Callers in this task ignore it; tests read
/// it, and Task 009's `sweep_orphan_blobs` reports `removed`.
#[cfg_attr(not(test), allow(dead_code))]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct Cleanup {
    pub removed: u64,
    /// Entries that could not be inspected or removed (left for the next sweep),
    /// or 1 when the pass was skipped because the write lock was unavailable.
    pub failed: u64,
}

fn remove(path: &Path, out: &mut Cleanup) {
    match fs::remove_file(path) {
        Ok(()) => out.removed += 1,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(_) => out.failed += 1,
    }
}

/// Unlinks each blob in `shas` that nothing references any more, under
/// `BEGIN IMMEDIATE`. Call after the commit that dropped the references.
pub(crate) fn remove_unreferenced(
    conn: &mut Connection,
    data_dir: &Path,
    shas: &[[u8; 32]],
) -> Cleanup {
    let mut out = Cleanup::default();
    let Ok(tx) = conn.transaction_with_behavior(TransactionBehavior::Immediate) else {
        out.failed += 1;
        return out;
    };
    for sha in shas {
        match is_referenced(&tx, sha) {
            Ok(false) => remove(&blob_path(data_dir, sha), &mut out),
            Ok(true) => {}
            Err(_) => out.failed += 1,
        }
    }
    // Nothing was written; dropping the transaction only releases the lock.
    drop(tx);
    out
}

fn referenced_set(conn: &Connection) -> Result<HashSet<String>, StorageError> {
    let mut set = HashSet::new();
    let mut st = conn.prepare(
        "SELECT DISTINCT blob_sha256 FROM representations WHERE blob_sha256 IS NOT NULL",
    )?;
    let rows = st.query_map([], |r| r.get::<_, Vec<u8>>(0))?;
    for sha in rows {
        if let Ok(arr) = <[u8; 32]>::try_from(sha?) {
            set.insert(hex(&arr));
        }
    }
    let mut st = conn.prepare(
        "SELECT json_extract(meta, '$.image_thumb_sha256') FROM clips \
         WHERE json_extract(meta, '$.image_thumb_sha256') IS NOT NULL",
    )?;
    let rows = st.query_map([], |r| r.get::<_, String>(0))?;
    for h in rows {
        set.insert(h?);
    }
    Ok(set)
}

/// Total size of the blob files that rows reference (thumbnails included), each file
/// once. Orphans, `.tmp` files and stray files do not count.
pub(crate) fn referenced_bytes(conn: &Connection, data_dir: &Path) -> Result<u64, StorageError> {
    let mut total = 0u64;
    for name in referenced_set(conn)? {
        let path = data_dir
            .join(BLOBS_DIR)
            .join(name.get(..2).unwrap_or("00"))
            .join(&name);
        if let Ok(m) = fs::metadata(path) {
            if m.is_file() {
                total = total.saturating_add(m.len());
            }
        }
    }
    Ok(total)
}

fn is_blob_name(name: &str) -> bool {
    name.len() == 64 && name.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

/// Startup orphan sweep: removes `.tmp` leftovers and blob files no row references,
/// under `BEGIN IMMEDIATE`. Only touches files named like blobs inside
/// two-hex-digit shard directories. Never fails: errors are skipped and counted.
pub(crate) fn sweep(conn: &mut Connection, data_dir: &Path) -> Cleanup {
    let mut out = Cleanup::default();
    let Ok(tx) = conn.transaction_with_behavior(TransactionBehavior::Immediate) else {
        out.failed += 1;
        return out;
    };
    let Ok(referenced) = referenced_set(&tx) else {
        out.failed += 1;
        return out;
    };
    let Ok(shards) = fs::read_dir(data_dir.join(BLOBS_DIR)) else {
        out.failed += 1;
        return out;
    };
    for shard in shards {
        let Ok(shard) = shard else {
            out.failed += 1;
            continue;
        };
        let is_shard = shard
            .file_name()
            .to_str()
            .is_some_and(|s| s.len() == 2 && s.bytes().all(|b| b.is_ascii_hexdigit()));
        if !is_shard || !shard.file_type().is_ok_and(|t| t.is_dir()) {
            continue;
        }
        let Ok(entries) = fs::read_dir(shard.path()) else {
            out.failed += 1;
            continue;
        };
        for entry in entries {
            let Ok(entry) = entry else {
                out.failed += 1;
                continue;
            };
            let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
                continue;
            };
            if !entry.file_type().is_ok_and(|t| t.is_file()) {
                continue;
            }
            let stale_tmp = name.strip_suffix(".tmp").is_some_and(is_blob_name);
            let orphan = is_blob_name(&name) && !referenced.contains(&name);
            if stale_tmp || orphan {
                remove(&entry.path(), &mut out);
            }
        }
    }
    drop(tx);
    out
}
