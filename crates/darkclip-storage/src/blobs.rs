//! Content-addressed blob files: `<data dir>/blobs/<aa>/<sha256 hex>` (data-model §5).
//!
//! A blob is written to `<file>.tmp`, flushed, then renamed into place, always before
//! the transaction that references it commits. A crash in between leaves only an
//! unreferenced file (or a `.tmp`), which `sweep` removes at the next read-write open.
//! Unix: directories 0700, files 0600.

use crate::StorageError;
use rusqlite::Connection;
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

/// Writes `bytes` (whose SHA-256 is `sha`) unless an identical blob already exists.
pub(crate) fn write(data_dir: &Path, sha: &[u8; 32], bytes: &[u8]) -> Result<(), StorageError> {
    let path = blob_path(data_dir, sha);
    if let Ok(m) = fs::metadata(&path) {
        if m.is_file() && u64::try_from(bytes.len()).is_ok_and(|n| n == m.len()) {
            return Ok(());
        }
    }
    if let Some(parent) = path.parent() {
        ensure_private_dir(parent)?;
    }
    let mut tmp = path.clone().into_os_string();
    tmp.push(".tmp");
    let tmp = PathBuf::from(tmp);
    let result = (|| {
        let mut f = create_private_file(&tmp, true)?;
        f.write_all(bytes)?;
        f.sync_all()?;
        drop(f);
        fs::rename(&tmp, &path)?;
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

/// Unlinks each blob in `shas` that nothing references any more. Call after commit.
pub(crate) fn remove_unreferenced(
    conn: &Connection,
    data_dir: &Path,
    shas: &[[u8; 32]],
) -> Result<(), StorageError> {
    for sha in shas {
        if !is_referenced(conn, sha)? {
            match fs::remove_file(blob_path(data_dir, sha)) {
                Err(e) if e.kind() != std::io::ErrorKind::NotFound => return Err(e.into()),
                _ => {}
            }
        }
    }
    Ok(())
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

fn is_blob_name(name: &str) -> bool {
    name.len() == 64 && name.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

/// Startup orphan sweep: removes `.tmp` leftovers and blob files no row references.
/// Only touches files named like blobs inside two-hex-digit shard directories.
/// Returns the number of files removed.
pub(crate) fn sweep(conn: &Connection, data_dir: &Path) -> Result<u64, StorageError> {
    let root = data_dir.join(BLOBS_DIR);
    let referenced = referenced_set(conn)?;
    let mut removed = 0u64;
    for shard in fs::read_dir(&root)? {
        let shard = shard?;
        let shard_name = shard.file_name();
        let is_shard = shard_name
            .to_str()
            .is_some_and(|s| s.len() == 2 && s.bytes().all(|b| b.is_ascii_hexdigit()));
        if !is_shard || !shard.file_type()?.is_dir() {
            continue;
        }
        for entry in fs::read_dir(shard.path())? {
            let entry = entry?;
            let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
                continue;
            };
            if !entry.file_type()?.is_file() {
                continue;
            }
            let stale_tmp = name.strip_suffix(".tmp").is_some_and(is_blob_name);
            let orphan = is_blob_name(&name) && !referenced.contains(&name);
            if stale_tmp || orphan {
                fs::remove_file(entry.path())?;
                removed += 1;
            }
        }
    }
    Ok(removed)
}
