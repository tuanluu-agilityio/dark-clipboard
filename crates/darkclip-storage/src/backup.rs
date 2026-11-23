//! Backup and restore (Task 405, data-model §8).
//!
//! **Backup** (`.dcbackup`, a zip): `history.db` + the blob files it references +
//! `manifest.json` `{ format, schema_version, app_version, created_at, clip_count, files }`.
//! The database is a `VACUUM INTO` snapshot (a consistent online copy that carries no free
//! pages), from which masked and expired clips are then deleted and the snapshot vacuumed
//! again: a backup never holds what the history would have dropped within minutes. The zip is
//! written to `<dest>.partial`, flushed, then renamed.
//!
//! **Restore** validates everything before the live data is touched: entry names, count and
//! sizes ([`crate::archive`]), the manifest (a newer `format` or `schema_version` is refused),
//! every file's size and SHA-256. The files are extracted to a private staging folder in the
//! data folder, the database there is opened with [`Storage::open`] (migrations run), then
//! checked: `integrity_check`, `foreign_key_check`, its schema must equal the one the
//! migrations create (no extra trigger, view or table can come in), the Favorites row, and
//! every clip must decode and validate. Only then:
//! 1. a safety copy of the live database is written as `history.db.pre-restore-<ms>` (kept
//!    until the next successful open, which deletes it; masked clips are purged from it);
//! 2. the staged blobs are copied into the live blob folder (content-addressed: existing
//!    files are never changed);
//! 3. the staged database is copied over the live one with SQLite's online backup API, on
//!    the store's own connection (the single writer, D-028). The destination write
//!    transaction is held across steps and rolled back if the copy does not finish, so a
//!    failure leaves the live database as it was.
//!
//! Blobs the old history used stay on disk (the safety copy references them) until the
//! orphan sweep of the next open. Nothing here logs or returns content or paths (GC-06).

use crate::archive::{
    self, ArchiveError, ArchiveLimits, AtomicZip, CheckedZip, EntryKind, BACKUP_FORMAT, HISTORY_DB,
    MANIFEST,
};
use crate::{blobs, db, migrate, OpenMode, Storage, StorageError};
use darkclip_core::{ClipId, Clock, Timestamp, FAVORITES_COLLECTION_ID};
use rusqlite::{Connection, OpenFlags};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

/// What a backup wrote.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct BackupReport {
    pub clips: u64,
    pub blobs: u64,
}

/// What a restore brought back.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct RestoreReport {
    pub clips: u64,
}

/// Prefix of the safety copy kept after a restore.
pub(crate) const PRE_RESTORE_PREFIX: &str = "history.db.pre-restore-";
/// Prefixes of staging folders (removed by the next open if a crash left one).
pub(crate) const STAGING_PREFIXES: [&str; 2] = ["backup-staging-", "restore-staging-"];

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Manifest {
    format: u32,
    schema_version: u32,
    app_version: String,
    created_at: Timestamp,
    clip_count: u64,
    files: Vec<ManifestFile>,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ManifestFile {
    path: String,
    size: u64,
    sha256: String,
}

/// Pages copied per backup step. Small in tests so a fault can land mid-copy.
const SWAP_STEP_PAGES: i32 = if cfg!(test) { 4 } else { 1024 };
/// How long the swap waits for another connection's lock before giving up.
const SWAP_BUSY_RETRIES: u32 = 500;

fn hex_len(n: usize) -> u64 {
    u64::try_from(n).unwrap_or(u64::MAX)
}

/// A consistent copy of the database at `target` without masked or expired clips and
/// without free pages (so nothing deleted lingers in it), as a single rollback-journal file.
fn private_snapshot(conn: &Connection, target: &Path, now: Timestamp) -> Result<(), StorageError> {
    db::backup(conn, target)?;
    let result = (|| {
        let c = Connection::open_with_flags(
            target,
            OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )?;
        c.pragma_update(None, "foreign_keys", true)?;
        c.pragma_update(None, "secure_delete", true)?;
        let _: String = c.pragma_update_and_check(None, "journal_mode", "DELETE", |r| r.get(0))?;
        let gone = c.execute(
            "DELETE FROM clips WHERE sensitivity_level = 'masked' \
             OR (expires_at IS NOT NULL AND expires_at <= ?1)",
            [now.0],
        )?;
        if gone > 0 {
            c.execute(
                "DELETE FROM tags WHERE id NOT IN (SELECT tag_id FROM clip_tags)",
                [],
            )?;
            crate::maintenance::fts_optimize(&c);
            c.execute_batch("VACUUM")?;
        }
        Ok(())
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(target);
    }
    result
}

/// `<data>/history.db.pre-restore-<ms>`, made unique if a copy with that name exists.
fn pre_restore_path(dir: &Path, now: Timestamp) -> PathBuf {
    let base = format!("{PRE_RESTORE_PREFIX}{}", now.0);
    let mut path = dir.join(&base);
    let mut n = 1u32;
    while path.exists() && n < 1000 {
        path = dir.join(format!("{base}-{n}"));
        n += 1;
    }
    path
}

/// Parses the manifest; a newer `format` or `schema_version` is reported as such before
/// the strict parse (so a newer file gets a clear message, not "malformed").
fn parse_manifest(raw: &[u8], supported_schema: u32) -> Result<Manifest, StorageError> {
    let v: serde_json::Value =
        serde_json::from_slice(raw).map_err(|_| ArchiveError::BadManifest)?;
    let number = |key: &str| v.get(key).and_then(serde_json::Value::as_u64);
    if number("format").is_some_and(|f| f > u64::from(BACKUP_FORMAT)) {
        return Err(ArchiveError::NewerFormat.into());
    }
    if let Some(found) = number("schema_version").filter(|s| *s > u64::from(supported_schema)) {
        return Err(StorageError::SchemaTooNew {
            found: u32::try_from(found).unwrap_or(u32::MAX),
            supported: supported_schema,
        });
    }
    let m: Manifest = serde_json::from_value(v).map_err(|_| ArchiveError::BadManifest)?;
    if m.format != BACKUP_FORMAT || m.schema_version == 0 {
        return Err(ArchiveError::BadManifest.into());
    }
    Ok(m)
}

/// The manifest's files by entry kind, checked against the archive's entries: the same set,
/// a database, blob names that match their hashes, sizes within the limits.
fn expected_files(
    m: &Manifest,
    z: &CheckedZip,
) -> Result<HashMap<EntryKind, (u64, [u8; 32])>, StorageError> {
    let mut out = HashMap::new();
    for f in &m.files {
        let kind = archive::classify_entry(&f.path).ok_or(ArchiveError::BadManifest)?;
        let sha = archive::parse_hex32(&f.sha256).ok_or(ArchiveError::BadManifest)?;
        match kind {
            EntryKind::Manifest | EntryKind::ExportJson => {
                return Err(ArchiveError::BadManifest.into())
            }
            EntryKind::Blob(name) if name != sha => return Err(ArchiveError::BadManifest.into()),
            _ => {}
        }
        if f.size > z.limits.max_entry_bytes {
            return Err(ArchiveError::TooLarge.into());
        }
        if out.insert(kind, (f.size, sha)).is_some() {
            return Err(ArchiveError::BadManifest.into());
        }
    }
    if !out.contains_key(&EntryKind::HistoryDb) {
        return Err(ArchiveError::BadManifest.into());
    }
    let in_zip = z
        .entries
        .iter()
        .filter(|(k, _)| *k != EntryKind::Manifest)
        .count();
    let all_listed = z
        .entries
        .iter()
        .all(|(k, _)| *k == EntryKind::Manifest || out.contains_key(k));
    if in_zip != out.len() || !all_listed {
        return Err(ArchiveError::BadManifest.into());
    }
    Ok(out)
}

/// One `sqlite_master` row: type, name, table, SQL.
type SchemaRow = (String, String, String, Option<String>);

/// The schema objects (type, name, table, SQL) a store must have: those the migrations
/// create, read from an in-memory database. SQLite's own objects are left out.
fn schema_of(conn: &Connection) -> Result<Vec<SchemaRow>, StorageError> {
    let mut st = conn.prepare(
        "SELECT type, name, tbl_name, sql FROM sqlite_master \
         WHERE name NOT LIKE 'sqlite\\_%' ESCAPE '\\' ORDER BY type, name",
    )?;
    let rows = st
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

fn expected_schema() -> Result<Vec<SchemaRow>, StorageError> {
    let mut mem = Connection::open_in_memory()?;
    migrate::run(&mut mem, migrate::MIGRATIONS)?;
    schema_of(&mem)
}

/// Any failure to read or check the staged database means the file is not a valid backup,
/// except a newer schema (its own message) and a full or failing disk.
fn staged_err(e: StorageError) -> StorageError {
    match e {
        StorageError::SchemaTooNew { .. }
        | StorageError::StorageFull
        | StorageError::Io(_)
        | StorageError::Archive(_) => e,
        _ => ArchiveError::Corrupt.into(),
    }
}

/// Opens the extracted database (migrating it), checks it, matches its page size to the
/// live one; returns its clip count. The store is closed again on return.
fn check_staged(
    staging: &Path,
    clock: Arc<dyn Clock>,
    live_page_size: i64,
) -> Result<u64, StorageError> {
    let st = Storage::open(staging, OpenMode::ReadWrite, clock).map_err(staged_err)?;
    let corrupt = || StorageError::Archive(ArchiveError::Corrupt);
    let check = || -> Result<u64, StorageError> {
        let c = &st.conn;
        let ok: String = c.query_row("PRAGMA integrity_check(1)", [], |r| r.get(0))?;
        if ok != "ok" {
            return Err(corrupt());
        }
        let fk_problem = c
            .prepare("PRAGMA foreign_key_check")?
            .query([])?
            .next()?
            .is_some();
        if fk_problem {
            return Err(corrupt());
        }
        if schema_of(c)? != expected_schema()? {
            return Err(corrupt());
        }
        let systems: i64 = c.query_row(
            "SELECT count(*) FROM collections WHERE kind = 'system'",
            [],
            |r| r.get(0),
        )?;
        let favorites: i64 = c.query_row(
            "SELECT count(*) FROM collections WHERE id = ?1 AND system_key = 'favorites'",
            [FAVORITES_COLLECTION_ID],
            |r| r.get(0),
        )?;
        if systems != 1 || favorites != 1 {
            return Err(corrupt());
        }
        let ids: Vec<String> = c
            .prepare("SELECT id FROM clips")?
            .query_map([], |r| r.get(0))?
            .collect::<Result<_, _>>()?;
        for id in &ids {
            let id = ClipId::parse(id).map_err(|_| corrupt())?;
            let clip = st.get_clip(&id)?.ok_or_else(corrupt)?;
            clip.validate().map_err(|_| corrupt())?;
        }
        let page: i64 = c.pragma_query_value(None, "page_size", |r| r.get(0))?;
        if page != live_page_size {
            // The online backup into a WAL database needs equal page sizes.
            let _: String =
                c.pragma_update_and_check(None, "journal_mode", "DELETE", |r| r.get(0))?;
            c.pragma_update(None, "page_size", live_page_size)?;
            c.execute_batch("VACUUM")?;
            let now: i64 = c.pragma_query_value(None, "page_size", |r| r.get(0))?;
            if now != live_page_size {
                return Err(corrupt());
            }
        }
        Ok(hex_len(ids.len()))
    };
    let r = check().map_err(staged_err);
    drop(st);
    r
}

impl Storage {
    /// Writes a backup of the store to `dest` (atomically: temp file, then rename).
    pub fn write_backup(
        &mut self,
        dest: &Path,
        app_version: &str,
    ) -> Result<BackupReport, StorageError> {
        self.ensure_writable()?;
        let now = self.clock.now();
        let staging = archive::fresh_dir(&self.dir, STAGING_PREFIXES[0], now.0)?;
        let result = self.backup_in(dest, app_version, &staging, now);
        archive::remove_dir(&staging);
        result
    }

    fn backup_in(
        &self,
        dest: &Path,
        app_version: &str,
        staging: &Path,
        now: Timestamp,
    ) -> Result<BackupReport, StorageError> {
        let snap = staging.join(HISTORY_DB);
        private_snapshot(&self.conn, &snap, now)?;
        let (clips, schema_version, referenced) = {
            let c = Connection::open_with_flags(
                &snap,
                OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
            )?;
            let n: i64 = c.query_row("SELECT count(*) FROM clips", [], |r| r.get(0))?;
            let mut referenced: Vec<String> = blobs::referenced_set(&c)?.into_iter().collect();
            referenced.sort();
            (
                u64::try_from(n).unwrap_or(0),
                migrate::user_version(&c)?,
                referenced,
            )
        };
        let mut z = AtomicZip::create(dest)?;
        let mut files = Vec::new();
        let (size, sha) = z.add_file(HISTORY_DB, &snap)?;
        files.push(ManifestFile {
            path: HISTORY_DB.into(),
            size,
            sha256: blobs::hex(&sha),
        });
        let mut blob_count = 0u64;
        for name in referenced {
            let Some(sha) = archive::parse_hex32(&name) else {
                continue;
            };
            let bytes = match blobs::read(&self.dir, &sha) {
                Ok(b) => b,
                // Already missing or damaged in the live store: the clip is broken there
                // too; the backup carries what exists.
                Err(StorageError::Io(
                    std::io::ErrorKind::NotFound | std::io::ErrorKind::InvalidData,
                )) => continue,
                Err(e) => return Err(e),
            };
            let entry = archive::blob_entry_name(&sha);
            z.add_bytes(&entry, &bytes)?;
            files.push(ManifestFile {
                path: entry,
                size: hex_len(bytes.len()),
                sha256: blobs::hex(&sha),
            });
            blob_count += 1;
        }
        let manifest = Manifest {
            format: BACKUP_FORMAT,
            schema_version,
            app_version: app_version.to_string(),
            created_at: now,
            clip_count: clips,
            files,
        };
        let json = serde_json::to_vec_pretty(&manifest)
            .map_err(|_| StorageError::Sqlite("ColumnEncode".into()))?;
        z.add_bytes(MANIFEST, &json)?;
        z.commit()?;
        Ok(BackupReport {
            clips,
            blobs: blob_count,
        })
    }

    /// Replaces the history with the backup at `archive`, after validating all of it.
    /// Any failure leaves the live data as it was.
    pub fn restore_backup(&mut self, archive: &Path) -> Result<RestoreReport, StorageError> {
        self.restore_backup_with(archive, ArchiveLimits::BACKUP)
    }

    /// `restore_backup` with explicit limits (tests use small ones).
    pub fn restore_backup_with(
        &mut self,
        archive: &Path,
        limits: ArchiveLimits,
    ) -> Result<RestoreReport, StorageError> {
        self.ensure_writable()?;
        let now = self.clock.now();
        let staging = archive::fresh_dir(&self.dir, STAGING_PREFIXES[1], now.0)?;
        let result = self.restore_in(archive, limits, &staging, now);
        archive::remove_dir(&staging);
        result
    }

    fn restore_in(
        &mut self,
        path: &Path,
        limits: ArchiveLimits,
        staging: &Path,
        now: Timestamp,
    ) -> Result<RestoreReport, StorageError> {
        // 1. Validate and extract, without touching the live data.
        let mut z = CheckedZip::open(path, limits, &[EntryKind::Manifest, EntryKind::HistoryDb])?;
        if z.index_of(EntryKind::Manifest).is_none() {
            return Err(ArchiveError::NotAnArchive.into());
        }
        let supported = migrate::target_version(migrate::MIGRATIONS);
        let manifest = parse_manifest(&z.read_json(EntryKind::Manifest)?, supported)?;
        let expected = expected_files(&manifest, &z)?;
        let staged_blobs = staging.join(blobs::BLOBS_DIR);
        blobs::ensure_private_dir(&staged_blobs)?;
        let mut blob_files = Vec::new();
        for (kind, idx) in z.entries.clone() {
            let Some((size, sha)) = expected.get(&kind).copied() else {
                continue;
            };
            match kind {
                EntryKind::HistoryDb => {
                    z.extract_verified(idx, size, &sha, &staging.join(HISTORY_DB))?;
                }
                EntryKind::Blob(_) => {
                    let h = blobs::hex(&sha);
                    let shard = staged_blobs.join(h.get(..2).unwrap_or("00"));
                    blobs::ensure_private_dir(&shard)?;
                    let target = shard.join(&h);
                    z.extract_verified(idx, size, &sha, &target)?;
                    blob_files.push((sha, target));
                }
                EntryKind::Manifest | EntryKind::ExportJson => {}
            }
        }
        drop(z);

        // 2. Open the staged copy (migrations run) and check it.
        let live_page: i64 = self
            .conn
            .pragma_query_value(None, "page_size", |r| r.get(0))?;
        let clips = check_staged(staging, Arc::clone(&self.clock), live_page)?;

        // 3. Safety copy of the live data.
        let pre = pre_restore_path(&self.dir, now);
        private_snapshot(&self.conn, &pre, now)?;
        let result = (|| {
            // 4. Blobs first: they must exist before rows that name them commit.
            for (sha, file) in &blob_files {
                // The staged open swept blobs its database does not reference.
                let Ok(bytes) = std::fs::read(file) else {
                    continue;
                };
                blobs::write(&self.dir, sha, &bytes)?;
            }
            // 5. The swap.
            self.swap_from(&staging.join(HISTORY_DB))
        })();
        if result.is_err() {
            let _ = std::fs::remove_file(&pre);
            return result.map(|()| RestoreReport::default());
        }
        crate::retention::checkpoint(&self.conn);
        self.fts_dirty = 0;
        Ok(RestoreReport { clips })
    }

    /// Copies the database at `src` over the live one with the online backup API. The
    /// destination write transaction spans all steps; if the copy stops early it is rolled
    /// back when the backup object is dropped.
    fn swap_from(&mut self, src: &Path) -> Result<(), StorageError> {
        use rusqlite::backup::{Backup, StepResult};
        let from = Connection::open_with_flags(
            src,
            OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )
        .map_err(staged_err_sqlite)?;
        #[cfg(test)]
        let fail = self.fail_restore_swap;
        let backup = Backup::new(&from, &mut self.conn)?;
        let mut busy = 0u32;
        loop {
            match backup.step(SWAP_STEP_PAGES)? {
                StepResult::Done => break,
                StepResult::More => {}
                StepResult::Busy | StepResult::Locked => {
                    busy += 1;
                    if busy > SWAP_BUSY_RETRIES {
                        return Err(StorageError::Sqlite("Busy".into()));
                    }
                    std::thread::sleep(Duration::from_millis(10));
                }
                _ => return Err(StorageError::Sqlite("BackupStep".into())),
            }
            #[cfg(test)]
            if fail {
                return Err(StorageError::Io(std::io::ErrorKind::Interrupted));
            }
        }
        Ok(())
    }
}

fn staged_err_sqlite(e: rusqlite::Error) -> StorageError {
    staged_err(e.into())
}

/// Removes the safety copies of earlier restores and staging folders a crash left behind.
/// Called by a successful read-write open. Best-effort.
pub(crate) fn cleanup_after_open(dir: &Path) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for e in entries.flatten() {
        let name = e.file_name();
        let Some(name) = name.to_str() else {
            continue;
        };
        let Ok(ft) = e.file_type() else {
            continue;
        };
        if ft.is_file() && name.starts_with(PRE_RESTORE_PREFIX) {
            let _ = std::fs::remove_file(e.path());
        } else if ft.is_dir() && STAGING_PREFIXES.iter().any(|p| name.starts_with(p)) {
            let _ = std::fs::remove_dir_all(e.path());
        }
    }
}

#[cfg(test)]
mod tests;
