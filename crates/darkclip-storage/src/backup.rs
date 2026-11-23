//! Backup and restore (Task 405, data-model §8).
//!
//! **Backup** (`.dcbackup`, a zip): `history.db` + the blob files it references +
//! `manifest.json` `{ format, schema_version, app_version, created_at, clip_count, files }`.
//! The database is a `VACUUM INTO` snapshot (a consistent online copy that carries no free
//! pages), from which masked clips and clips already expired are then deleted and the
//! snapshot vacuumed again. Clips flagged no-export (NoCloud) are kept: a backup is the
//! user's own full copy. The zip is written to `<dest>.dark-clipboard-partial`, flushed,
//! checked against the limits its own restore enforces (`TooLargeToWrite` otherwise), then
//! renamed. A crash can leave the partial file behind; it is safe to delete.
//!
//! **Restore** validates everything before the live data is touched:
//! 1. the archive ([`crate::archive`]: names, counts, declared sizes and their ratio to the
//!    file, header consistency), the manifest (a newer `format` or `schema_version` is
//!    refused; it must list exactly the archive's files), and free disk space for the
//!    staging copy, the safety copy and the blobs (`InsufficientSpace`, also when the
//!    caller could not measure it);
//! 2. every file is extracted to a private staging folder with its size and SHA-256 checked;
//! 3. before any migration, the staged database is opened on a raw, defensive connection
//!    (`SQLITE_DBCONFIG_DEFENSIVE`, `trusted_schema=OFF`, `cell_size_check=ON`, no mmap):
//!    the whole `sqlite_master` (unfiltered: `sqlite_*` names included) must equal a schema
//!    this app ever created (versions 1 to current) plus at most SQLite's own
//!    `sqlite_sequence` / `sqlite_stat1` / `sqlite_stat4` tables, so no extra trigger, view,
//!    table or index can reach a migration or the live store, whatever its name; then text
//!    encoding UTF-8, `user_version` equal to the manifest's, and only then `integrity_check`;
//! 4. then it is opened with [`Storage::open`] (migrations run) and checked again:
//!    `integrity_check`, `foreign_key_check`, the current schema exactly, the Favorites row,
//!    every clip decodes and validates;
//! 5. header fields the online backup API copies with page 1 are normalised to the live
//!    database's: page size and auto-vacuum (converted with a `VACUUM`), `application_id`,
//!    WAL journal mode; the encoding is never converted (UTF-16 is refused in step 3) and
//!    `user_version` is the current schema version.
//!
//! Only then: a safety copy of the live database is written as
//! `history.db.pre-restore-<ms>` (kept until the next successful open, which deletes it;
//! masked clips are purged from it); the staged blobs are copied into the live blob folder
//! (content-addressed: existing files are never changed); and the staged database is copied
//! over the live one with SQLite's online backup API, on the store's own connection (the
//! single writer, D-028). The destination write transaction is held across steps and rolled
//! back if the copy does not finish, so a failure leaves the live database as it was.
//!
//! A restored database is not scanned again for sensitive content: it holds what the user's
//! own history held when the backup was made (masked clips were left out then).
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

/// Every `sqlite_master` row (type, name, table, SQL), unfiltered: objects named `sqlite_*`
/// included, since `writable_schema` can plant a trigger or table under such a name.
fn schema_of(conn: &Connection) -> Result<Vec<SchemaRow>, StorageError> {
    let mut st =
        conn.prepare("SELECT type, name, tbl_name, sql FROM sqlite_master ORDER BY type, name")?;
    let rows = st
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

/// The schema migrations 1..=`version` create (from an in-memory database).
fn known_schema(version: u32) -> Result<Vec<SchemaRow>, StorageError> {
    let mut mem = Connection::open_in_memory()?;
    let upto: Vec<migrate::Migration> = migrate::MIGRATIONS
        .iter()
        .filter(|m| m.version <= version)
        .map(|m| migrate::Migration {
            version: m.version,
            sql: m.sql,
        })
        .collect();
    migrate::run(&mut mem, &upto)?;
    schema_of(&mem)
}

/// SQLite's own tables that may appear in a store besides the migrations' objects, with the
/// exact SQL SQLite gives them: `sqlite_sequence` (AUTOINCREMENT) and the `ANALYZE` statistics
/// (`sqlite_stat1`, and `sqlite_stat4` when compiled in). Taken from a scratch in-memory
/// database, so the text is SQLite's own.
fn allowed_sqlite_tables() -> Result<Vec<SchemaRow>, StorageError> {
    let mem = Connection::open_in_memory()?;
    mem.execute_batch(
        "CREATE TABLE t(x INTEGER PRIMARY KEY AUTOINCREMENT, y); CREATE INDEX ti ON t(y); \
         INSERT INTO t(y) VALUES (1), (2); ANALYZE;",
    )?;
    Ok(schema_of(&mem)?
        .into_iter()
        .filter(|(kind, name, _, _)| {
            kind == "table"
                && matches!(
                    name.as_str(),
                    "sqlite_sequence" | "sqlite_stat1" | "sqlite_stat4"
                )
        })
        .collect())
}

/// Whether `staged` is exactly the `known` schema, plus at most SQLite's own allowed tables.
/// Any other object (a trigger, view, table or index, whatever its name, `sqlite_*` included,
/// or an `sqlite_autoindex_*` of an unexpected table) fails, as does a missing one.
fn schema_matches(staged: &[SchemaRow], known: &[SchemaRow]) -> Result<bool, StorageError> {
    let allowed = allowed_sqlite_tables()?;
    let all_known_present = known.iter().all(|k| staged.contains(k));
    let nothing_else = staged
        .iter()
        .all(|row| known.contains(row) || allowed.contains(row));
    let no_repeats = {
        let mut names: Vec<(&String, &String)> = staged.iter().map(|(t, n, _, _)| (t, n)).collect();
        names.sort();
        names.windows(2).all(|w| w.first() != w.get(1))
    };
    Ok(all_known_present && nothing_else && no_repeats)
}

/// The header fields the online backup API carries with page 1 that this module controls.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Header {
    page_size: i64,
    auto_vacuum: i64,
    application_id: i64,
}

fn header_of(c: &Connection) -> Result<Header, StorageError> {
    let get = |name: &str| -> Result<i64, StorageError> {
        Ok(c.pragma_query_value(None, name, |r| r.get(0))?)
    };
    Ok(Header {
        page_size: get("page_size")?,
        auto_vacuum: get("auto_vacuum")?,
        application_id: get("application_id")?,
    })
}

fn text_pragma(c: &Connection, name: &str) -> Result<String, StorageError> {
    Ok(c.pragma_query_value(None, name, |r| r.get(0))?)
}

/// Step 3 of a restore: the staged file on a raw, defensive connection, before any migration
/// runs on it. Returns its schema version.
fn precheck_raw(db: &Path, manifest_version: u32, supported: u32) -> Result<u32, StorageError> {
    let corrupt = || StorageError::Archive(ArchiveError::Corrupt);
    let c = Connection::open_with_flags(
        db,
        OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .map_err(|_| corrupt())?;
    let check = || -> Result<u32, StorageError> {
        c.set_db_config(rusqlite::config::DbConfig::SQLITE_DBCONFIG_DEFENSIVE, true)?;
        c.pragma_update(None, "trusted_schema", false)?;
        c.pragma_update(None, "cell_size_check", true)?;
        let _: i64 = c.pragma_update_and_check(None, "mmap_size", 0, |r| r.get(0))?;
        // The whole schema is read first: a file that is not a database fails here, before its
        // header is believed.
        let rows = schema_of(&c).map_err(|_| corrupt())?;
        if text_pragma(&c, "encoding")? != "UTF-8" {
            return Err(corrupt());
        }
        let found = migrate::user_version(&c)?;
        if found > supported {
            return Err(StorageError::SchemaTooNew { found, supported });
        }
        if found != manifest_version {
            return Err(ArchiveError::BadManifest.into());
        }
        // The unfiltered schema is compared before `integrity_check`, so the check only ever
        // walks the app's own objects.
        if found == 0 || !schema_matches(&rows, &known_schema(found)?)? {
            return Err(corrupt());
        }
        if text_pragma(&c, "integrity_check")? != "ok" {
            return Err(corrupt());
        }
        Ok(found)
    };
    let r = check().map_err(staged_err);
    drop(c);
    r
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

/// Steps 4 and 5 of a restore: opens the extracted database (migrating it), checks it and
/// normalises its header to `live`; returns its clip count. Closed again on return.
fn check_staged(staging: &Path, clock: Arc<dyn Clock>, live: Header) -> Result<u64, StorageError> {
    let st = Storage::open(staging, OpenMode::ReadWrite, clock).map_err(staged_err)?;
    let corrupt = || StorageError::Archive(ArchiveError::Corrupt);
    let supported = migrate::target_version(migrate::MIGRATIONS);
    let check = || -> Result<u64, StorageError> {
        let c = &st.conn;
        if text_pragma(c, "integrity_check")? != "ok" {
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
        if migrate::user_version(c)? != supported
            || !schema_matches(&schema_of(c)?, &known_schema(supported)?)?
        {
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
        // Header fields the backup API copies with page 1 (step 5).
        let mut h = header_of(c)?;
        if h.page_size != live.page_size || h.auto_vacuum != live.auto_vacuum {
            // Both need a rebuild; page size cannot change in WAL mode.
            let _: String =
                c.pragma_update_and_check(None, "journal_mode", "DELETE", |r| r.get(0))?;
            c.pragma_update(None, "page_size", live.page_size)?;
            c.pragma_update(None, "auto_vacuum", live.auto_vacuum)?;
            c.execute_batch("VACUUM")?;
        }
        if h.application_id != live.application_id {
            c.pragma_update(None, "application_id", live.application_id)?;
        }
        let jm: String = c.pragma_update_and_check(None, "journal_mode", "WAL", |r| r.get(0))?;
        h = header_of(c)?;
        if h != live
            || !jm.eq_ignore_ascii_case("wal")
            || text_pragma(c, "encoding")? != "UTF-8"
            || migrate::user_version(c)? != supported
        {
            return Err(corrupt());
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
        self.write_backup_with(dest, app_version, ArchiveLimits::BACKUP)
    }

    /// `write_backup` refusing anything `limits` would refuse on restore.
    pub fn write_backup_with(
        &mut self,
        dest: &Path,
        app_version: &str,
        limits: ArchiveLimits,
    ) -> Result<BackupReport, StorageError> {
        self.ensure_writable()?;
        let now = self.clock.now();
        let staging = archive::fresh_dir(&self.dir, STAGING_PREFIXES[0], now.0)?;
        let result = self.backup_in(dest, app_version, &staging, now, limits);
        archive::remove_dir(&staging);
        result
    }

    fn backup_in(
        &self,
        dest: &Path,
        app_version: &str,
        staging: &Path,
        now: Timestamp,
        limits: ArchiveLimits,
    ) -> Result<BackupReport, StorageError> {
        let too_big = || StorageError::Archive(ArchiveError::TooLargeToWrite);
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
        // Refuse what the restore would refuse, before writing anything.
        let db_len = std::fs::metadata(&snap)?.len();
        if referenced.len().saturating_add(2) > limits.max_entries
            || db_len > limits.max_entry_bytes
            || db_len > limits.max_total_bytes
        {
            return Err(too_big());
        }
        let mut total = db_len;
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
            let len = hex_len(bytes.len());
            total = total.saturating_add(len);
            if len > limits.max_entry_bytes || total > limits.max_total_bytes {
                return Err(too_big());
            }
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
        let json_len = hex_len(json.len());
        total = total.saturating_add(json_len);
        if json_len > limits.max_json_bytes || total > limits.max_total_bytes {
            return Err(too_big());
        }
        z.add_bytes(MANIFEST, &json)?;
        z.commit_checked(total, limits)?;
        Ok(BackupReport {
            clips,
            blobs: blob_count,
        })
    }

    /// Replaces the history with the backup at `archive`, after validating all of it.
    /// Any failure leaves the live data as it was.
    /// `free_bytes`: free disk space of the data folder as the caller measured it (this crate
    /// has no OS access); unknown or too little is refused with `InsufficientSpace`.
    pub fn restore_backup(
        &mut self,
        archive: &Path,
        free_bytes: Option<u64>,
    ) -> Result<RestoreReport, StorageError> {
        self.restore_backup_with(archive, ArchiveLimits::BACKUP, free_bytes)
    }

    /// `restore_backup` with explicit limits (tests use small ones).
    pub fn restore_backup_with(
        &mut self,
        archive: &Path,
        limits: ArchiveLimits,
        free_bytes: Option<u64>,
    ) -> Result<RestoreReport, StorageError> {
        self.ensure_writable()?;
        let now = self.clock.now();
        let staging = archive::fresh_dir(&self.dir, STAGING_PREFIXES[1], now.0)?;
        let result = self.restore_in(archive, limits, free_bytes, &staging, now);
        archive::remove_dir(&staging);
        result
    }

    fn restore_in(
        &mut self,
        path: &Path,
        limits: ArchiveLimits,
        free_bytes: Option<u64>,
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
        // Room for the staged files, the safety copy of the live database and the blobs
        // copied into the live folder (unknown free space is refused, like compaction).
        let declared: u64 = expected.values().map(|(size, _)| *size).sum();
        let live_db = std::fs::metadata(self.dir.join(db::DB_FILE)).map_or(0, |m| m.len());
        let needed = declared.saturating_mul(2).saturating_add(live_db);
        if free_bytes.is_none_or(|free| free < needed) {
            return Err(StorageError::InsufficientSpace);
        }
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

        // 2. Check the staged copy raw, then open it (migrations run) and check it again.
        precheck_raw(
            &staging.join(HISTORY_DB),
            manifest.schema_version,
            supported,
        )?;
        let live = header_of(&self.conn)?;
        let clips = check_staged(staging, Arc::clone(&self.clock), live)?;

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
