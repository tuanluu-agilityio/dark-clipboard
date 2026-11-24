//! Opening the store: directory and file permissions, connection pragmas
//! (ADR-0003), schema version checks, pre-migration backup (data-model §7) and the
//! startup orphan-blob sweep.

use crate::blobs;
use crate::migrate::{self, Migration, MIGRATIONS};
use crate::{OpenMode, Storage, StorageError};
use darkclip_core::Clock;
use rusqlite::{Connection, OpenFlags};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

pub(crate) const DB_FILE: &str = "history.db";
const BUSY_TIMEOUT: Duration = Duration::from_millis(2000);

fn backup_path(data_dir: &Path, version: u32) -> PathBuf {
    data_dir.join(format!("{DB_FILE}.pre-v{version}.bak"))
}

/// Consistent copy of the open database (including WAL content) via `VACUUM INTO`,
/// into a fresh 0600 file.
pub(crate) fn backup(conn: &Connection, target: &Path) -> Result<(), StorageError> {
    match std::fs::remove_file(target) {
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => return Err(e.into()),
        _ => {}
    }
    // VACUUM INTO accepts an empty existing file; creating it first fixes its mode.
    drop(blobs::create_private_file(target, true)?);
    let path = target
        .to_str()
        .ok_or(StorageError::Io(std::io::ErrorKind::InvalidInput))?;
    let result = conn.execute("VACUUM INTO ?1", [path]).map(drop);
    if result.is_err() {
        let _ = std::fs::remove_file(target);
    }
    Ok(result?)
}

/// `PRAGMA quick_check(1)`: O(N) structural check of every b-tree page (no index-content or
/// UNIQUE check). Anything but a single `ok` is `Corrupt`, and so is a `SQLITE_CORRUPT` or
/// `SQLITE_NOTADB` while checking (the error mapping); other errors (busy, I/O) stay what they
/// are, so a locked database is never offered for moving aside. The text of a failure (page
/// numbers) is dropped.
fn quick_check(conn: &Connection) -> Result<(), StorageError> {
    let first: String = conn.query_row("PRAGMA quick_check(1)", [], |r| r.get(0))?;
    if first == "ok" {
        Ok(())
    } else {
        Err(StorageError::Corrupt)
    }
}

pub(crate) fn open_with(
    data_dir: &Path,
    mode: OpenMode,
    clock: Arc<dyn Clock>,
    migrations: &[Migration],
) -> Result<Storage, StorageError> {
    let db_path = data_dir.join(DB_FILE);
    let supported = migrate::target_version(migrations);

    let mut conn = match mode {
        OpenMode::ReadOnly => {
            // Never create anything on a read-only open.
            if !db_path.is_file() {
                return Err(StorageError::NotFound);
            }
            Connection::open_with_flags(
                &db_path,
                OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
            )?
        }
        OpenMode::ReadWrite => {
            blobs::ensure_private_dir(data_dir)?;
            blobs::ensure_private_dir(&data_dir.join(blobs::BLOBS_DIR))?;
            // Pre-create the file 0600 so SQLite's -wal/-shm inherit that mode.
            drop(blobs::create_private_file(&db_path, false)?);
            Connection::open_with_flags(
                &db_path,
                OpenFlags::SQLITE_OPEN_READ_WRITE
                    | OpenFlags::SQLITE_OPEN_CREATE
                    | OpenFlags::SQLITE_OPEN_NO_MUTEX,
            )?
        }
    };
    conn.busy_timeout(BUSY_TIMEOUT)?;

    // Check the version before changing anything: a newer database is never touched.
    let found = migrate::user_version(&conn)?;
    if found > supported {
        return Err(StorageError::SchemaTooNew { found, supported });
    }
    // A damaged file is refused in both modes before anything is written or read from it
    // (Task 413, F407-03); the desktop then offers to move it aside.
    quick_check(&conn)?;
    conn.pragma_update(None, "foreign_keys", true)?;

    if mode == OpenMode::ReadOnly {
        if found < supported {
            // Migrating is a write; the read-write owner (the app) must do it first.
            return Err(StorageError::ReadOnly);
        }
        return Ok(Storage::from_parts(conn, data_dir, mode, clock));
    }

    let journal: String =
        conn.pragma_update_and_check(None, "journal_mode", "WAL", |r| r.get(0))?;
    if !journal.eq_ignore_ascii_case("wal") {
        return Err(StorageError::Sqlite("JournalModeNotWal".into()));
    }
    conn.pragma_update(None, "secure_delete", true)?;

    if found < supported {
        // Back up an existing database (not a brand-new one) before migrating.
        let has_schema: bool =
            conn.query_row("SELECT EXISTS(SELECT 1 FROM sqlite_master)", [], |r| {
                r.get(0)
            })?;
        if found > 0 || has_schema {
            backup(&conn, &backup_path(data_dir, supported))?;
        }
        migrate::run(&mut conn, migrations)?;
    } else {
        // This is a successful run at `supported` after the migrating one:
        // the pre-migration backup has served its purpose.
        // Best-effort: if it cannot be removed now, the next open retries.
        let _ = std::fs::remove_file(backup_path(data_dir, supported));
    }

    // A successful start ends the life of an earlier restore's safety copy and of staging
    // folders a crash left behind (Task 405). Best-effort, like the sweep.
    crate::backup::cleanup_after_open(data_dir);
    // Best-effort; never fails the open. Leftovers are retried at the next open.
    let _ = blobs::sweep(&mut conn, data_dir);
    Ok(Storage::from_parts(conn, data_dir, mode, clock))
}

impl Storage {
    fn from_parts(conn: Connection, dir: &Path, mode: OpenMode, clock: Arc<dyn Clock>) -> Self {
        Self {
            conn,
            dir: dir.to_path_buf(),
            mode,
            clock,
            fts_dirty: 0,
            #[cfg(test)]
            optimize_runs: 0,
            #[cfg(test)]
            fail_before_commit: false,
            #[cfg(test)]
            fail_restore_swap: false,
        }
    }

    /// Opens (and for `ReadWrite`, creates and migrates) the store in `data_dir`.
    /// A database with a newer `user_version` yields `SchemaTooNew` in both modes.
    pub fn open(
        data_dir: &Path,
        mode: OpenMode,
        clock: Arc<dyn Clock>,
    ) -> Result<Self, StorageError> {
        open_with(data_dir, mode, clock, MIGRATIONS)
    }

    pub fn schema_version(&self) -> Result<u32, StorageError> {
        migrate::user_version(&self.conn)
    }

    pub(crate) fn ensure_writable(&self) -> Result<(), StorageError> {
        match self.mode {
            OpenMode::ReadWrite => Ok(()),
            OpenMode::ReadOnly => Err(StorageError::ReadOnly),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::retention::test_support::*;

    /// A word that is unique in the file and far from secret-shaped (GC-09).
    const TOKEN: &str = "qzvkjxwpremigrate";

    fn temp_store(c: &Connection) -> i64 {
        c.pragma_query_value(None, "temp_store", |r| r.get(0))
            .unwrap()
    }

    fn contains(hay: &[u8], needle: &str) -> bool {
        hay.windows(needle.len()).any(|w| w == needle.as_bytes())
    }

    /// The shipped migrations plus one more at the next version whose SQL is `body`
    /// (it must not set `user_version`; that is appended here).
    fn with_extra(body: &str) -> (Vec<Migration>, u32) {
        let next = migrate::target_version(MIGRATIONS) + 1;
        let sql: &'static str =
            Box::leak(format!("{body}\nPRAGMA user_version = {next};").into_boxed_str());
        let mut list: Vec<Migration> = MIGRATIONS
            .iter()
            .map(|m| Migration {
                version: m.version,
                sql: m.sql,
            })
            .collect();
        list.push(Migration { version: next, sql });
        (list, next)
    }

    #[test]
    fn writer_and_reader_connections_keep_temporary_data_in_memory() {
        let tmp = tempfile::tempdir().unwrap();
        let w = open(tmp.path(), clock());
        assert_eq!(temp_store(&w.conn), 2, "read-write connection");
        let r = Storage::open(tmp.path(), OpenMode::ReadOnly, clock()).unwrap();
        assert_eq!(temp_store(&r.conn), 2, "read-only connection");
    }

    #[test]
    fn migrations_already_run_with_temporary_data_in_memory() {
        // A test-only migration records the connection's temp_store while it runs.
        let (list, _) = with_extra(
            "CREATE TABLE temp_store_probe AS SELECT temp_store AS ts FROM pragma_temp_store;",
        );
        let tmp = tempfile::tempdir().unwrap();
        let s = open_with(tmp.path(), OpenMode::ReadWrite, clock(), &list).unwrap();
        let ts: i64 = s
            .conn
            .query_row("SELECT ts FROM temp_store_probe", [], |r| r.get(0))
            .unwrap();
        assert_eq!(ts, 2);
    }

    /// Building a database at an older shipped version with pending index deletions is not
    /// practical (the current code only writes the current schema), so the store is made at
    /// the current version and opened with one extra no-op migration: the same migrating
    /// branch of `open`, with the same pre-migration copy.
    #[test]
    fn migration_backup_has_no_pending_fts_tokens() {
        let tmp = tempfile::tempdir().unwrap();
        {
            let mut s = open(tmp.path(), clock());
            let id = ins(&mut s, text_clip("a clip to annotate"));
            s.set_note(&id, Some(&format!("note {TOKEN}"))).unwrap();
            // Replaced: the old words wait for the batched merge, which never runs here
            // (as when a session ends without its shutdown merge).
            s.set_note(&id, Some("a different note")).unwrap();
            assert!(s.fts_pending() > 0);
        }
        let mut bytes = std::fs::read(tmp.path().join(DB_FILE)).unwrap();
        if let Ok(wal) = std::fs::read(tmp.path().join(format!("{DB_FILE}-wal"))) {
            bytes.extend(wal);
        }
        assert!(contains(&bytes, TOKEN), "precondition: merge pending");

        let (list, next) = with_extra("SELECT 1;");
        let s = open_with(tmp.path(), OpenMode::ReadWrite, clock(), &list).unwrap();
        assert_eq!(s.schema_version().unwrap(), next);
        let bak = backup_path(tmp.path(), next);
        assert!(bak.is_file(), "the copy is still made");
        assert!(!contains(&std::fs::read(&bak).unwrap(), TOKEN));
    }
}
