//! A history that cannot be opened (Task 413, F407-03).
//!
//! `move_damaged_aside` renames a damaged database out of the way so the app can start with
//! an empty one. It never deletes anything: the database with its `-wal` and `-shm`, the
//! `blobs/` folder (which the next open's orphan sweep would otherwise empty), and the
//! pre-migration and pre-restore copies (which the next successful open would otherwise
//! remove) all get the same `damaged-<ms>` mark. Names only; no content is read.

use crate::backup::PRE_RESTORE_PREFIX;
use crate::blobs::BLOBS_DIR;
use crate::db::DB_FILE;
use crate::StorageError;
use darkclip_core::Timestamp;
use std::path::{Path, PathBuf};

/// Prefix of a pre-migration copy (`history.db.pre-vN.bak`, see `db::backup_path`).
const PRE_MIGRATION_PREFIX: &str = "history.db.pre-v";

/// What gets moved, as `(from, to)` pairs in rename order. The `-wal` and `-shm` files go
/// before the database, so a failure can never leave a stale WAL next to a new database.
fn plan(data_dir: &Path, now: Timestamp) -> Result<Vec<(PathBuf, PathBuf)>, StorageError> {
    let db = data_dir.join(DB_FILE);
    if !db.is_file() {
        return Err(StorageError::NotFound);
    }
    let mark = format!("{DB_FILE}.damaged-{}", now.0);
    let mut out = Vec::new();
    for side in ["-wal", "-shm"] {
        let from = data_dir.join(format!("{DB_FILE}{side}"));
        if exists(&from) {
            out.push((from, data_dir.join(format!("{mark}{side}"))));
        }
    }
    out.push((db, data_dir.join(&mark)));
    // Copies the next successful open would remove (pre-migration, pre-restore).
    let mut copies: Vec<String> = std::fs::read_dir(data_dir)?
        .flatten()
        .filter(|e| e.file_type().is_ok_and(|t| t.is_file()))
        .filter_map(|e| e.file_name().to_str().map(str::to_owned))
        .filter(|n| n.starts_with(PRE_MIGRATION_PREFIX) || n.starts_with(PRE_RESTORE_PREFIX))
        .collect();
    copies.sort();
    for name in copies {
        // `history.db.pre-...` -> `history.db.damaged-<ms>.pre-...`
        let rest = name.get(DB_FILE.len()..).unwrap_or_default();
        out.push((data_dir.join(&name), data_dir.join(format!("{mark}{rest}"))));
    }
    let blobs = data_dir.join(BLOBS_DIR);
    if exists(&blobs) {
        out.push((
            blobs,
            data_dir.join(format!("{BLOBS_DIR}.damaged-{}", now.0)),
        ));
    }
    if out.iter().any(|(_, to)| exists(to)) {
        return Err(StorageError::Io(std::io::ErrorKind::AlreadyExists));
    }
    Ok(out)
}

/// True when something (file, folder or link) has this name.
fn exists(p: &Path) -> bool {
    std::fs::symlink_metadata(p).is_ok()
}

/// Renames the damaged history in `data_dir` aside with the mark `damaged-<now ms>`:
/// `history.db` -> `history.db.damaged-<ms>` (its `-wal`/`-shm` follow as
/// `history.db.damaged-<ms>-wal`/`-shm`, so SQLite still pairs them), `blobs/` ->
/// `blobs.damaged-<ms>/`, and `history.db.pre-...` copies -> `history.db.damaged-<ms>.pre-...`.
///
/// `NotFound` when there is no `history.db`; `Io(AlreadyExists)` when a target name is taken
/// (nothing is moved then). If a rename fails half way, the ones already done are undone
/// (best-effort) and the error is returned. Must be called while no connection is open.
pub fn move_damaged_aside(data_dir: &Path, now: Timestamp) -> Result<(), StorageError> {
    let moves = plan(data_dir, now)?;
    for (i, (from, to)) in moves.iter().enumerate() {
        if let Err(e) = std::fs::rename(from, to) {
            // Undo in reverse order so the history is left as it was found.
            for (back_from, back_to) in moves.iter().take(i).rev() {
                let _ = std::fs::rename(back_to, back_from);
            }
            return Err(e.into());
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{OpenMode, Storage};
    use darkclip_core::Clock;
    use std::sync::Arc;

    struct FixedClock;
    impl Clock for FixedClock {
        fn now(&self) -> Timestamp {
            Timestamp(1_795_320_000_000)
        }
    }

    fn clock() -> Arc<dyn Clock> {
        Arc::new(FixedClock)
    }

    fn open(dir: &Path, mode: OpenMode) -> Result<Storage, StorageError> {
        Storage::open(dir, mode, clock())
    }

    /// A valid store with a filler table big enough that a page in the middle is a b-tree
    /// page (synthetic bytes only).
    fn store_with_pages(dir: &Path) {
        drop(open(dir, OpenMode::ReadWrite).unwrap());
        let c = rusqlite::Connection::open(dir.join(DB_FILE)).unwrap();
        c.execute_batch("CREATE TABLE filler(x BLOB)").unwrap();
        let row = vec![0x5Au8; 100];
        c.execute_batch("BEGIN").unwrap();
        for _ in 0..8000 {
            c.execute("INSERT INTO filler(x) VALUES (?1)", [&row])
                .unwrap();
        }
        c.execute_batch("COMMIT").unwrap();
        // Closing the last connection checkpoints the WAL into the database file.
        drop(c);
        assert!(!dir.join(format!("{DB_FILE}-wal")).exists());
    }

    fn page_count(dir: &Path) -> u64 {
        std::fs::metadata(dir.join(DB_FILE)).unwrap().len() / 4096
    }

    #[test]
    fn a_file_that_is_not_a_database_is_corrupt_in_both_modes() {
        let tmp = tempfile::tempdir().unwrap();
        let db = tmp.path().join(DB_FILE);
        let junk = b"this is not a database, just synthetic text for the test file.";
        std::fs::write(&db, junk).unwrap();
        assert_eq!(
            open(tmp.path(), OpenMode::ReadWrite).err(),
            Some(StorageError::Corrupt)
        );
        assert_eq!(
            open(tmp.path(), OpenMode::ReadOnly).err(),
            Some(StorageError::Corrupt)
        );
        // Never modified.
        assert_eq!(std::fs::read(&db).unwrap(), junk);
    }

    #[test]
    fn a_damaged_page_fails_the_quick_check_in_both_modes() {
        let tmp = tempfile::tempdir().unwrap();
        store_with_pages(tmp.path());
        let pages = page_count(tmp.path());
        assert!(pages > 100, "{pages} pages");
        let db = tmp.path().join(DB_FILE);
        let mut bytes = std::fs::read(&db).unwrap();
        let at = usize::try_from(pages / 2 * 4096).unwrap();
        bytes[at..at + 4096].fill(0xA5);
        std::fs::write(&db, &bytes).unwrap();
        assert_eq!(
            open(tmp.path(), OpenMode::ReadWrite).err(),
            Some(StorageError::Corrupt)
        );
        assert_eq!(
            open(tmp.path(), OpenMode::ReadOnly).err(),
            Some(StorageError::Corrupt)
        );
    }

    #[test]
    fn a_healthy_store_passes_the_quick_check() {
        let tmp = tempfile::tempdir().unwrap();
        store_with_pages(tmp.path());
        assert!(open(tmp.path(), OpenMode::ReadWrite).is_ok());
        assert!(open(tmp.path(), OpenMode::ReadOnly).is_ok());
    }

    #[test]
    fn a_newer_database_is_still_schema_too_new_and_untouched() {
        let tmp = tempfile::tempdir().unwrap();
        drop(open(tmp.path(), OpenMode::ReadWrite).unwrap());
        let db = tmp.path().join(DB_FILE);
        {
            let c = rusqlite::Connection::open(&db).unwrap();
            c.pragma_update(None, "user_version", 999).unwrap();
        }
        let before = std::fs::read(&db).unwrap();
        for mode in [OpenMode::ReadWrite, OpenMode::ReadOnly] {
            assert!(matches!(
                open(tmp.path(), mode).err(),
                Some(StorageError::SchemaTooNew { found: 999, .. })
            ));
        }
        assert_eq!(std::fs::read(&db).unwrap(), before);
    }

    #[test]
    fn corrupt_and_not_a_database_codes_map_to_corrupt() {
        for code in [rusqlite::ffi::SQLITE_CORRUPT, rusqlite::ffi::SQLITE_NOTADB] {
            let e = rusqlite::Error::SqliteFailure(rusqlite::ffi::Error::new(code), None);
            assert_eq!(StorageError::from(e), StorageError::Corrupt);
        }
        let other = rusqlite::Error::SqliteFailure(
            rusqlite::ffi::Error::new(rusqlite::ffi::SQLITE_BUSY),
            None,
        );
        assert!(matches!(StorageError::from(other), StorageError::Sqlite(_)));
        assert!(!StorageError::Corrupt.to_string().is_empty());
    }

    fn write(p: &Path, bytes: &[u8]) {
        if let Some(d) = p.parent() {
            std::fs::create_dir_all(d).unwrap();
        }
        std::fs::write(p, bytes).unwrap();
    }

    #[test]
    fn moving_aside_renames_every_part_and_keeps_the_bytes() {
        let tmp = tempfile::tempdir().unwrap();
        let d = tmp.path();
        write(&d.join("history.db"), b"db");
        write(&d.join("history.db-wal"), b"wal");
        write(&d.join("history.db-shm"), b"shm");
        write(&d.join("blobs").join("ab").join("abcd"), b"blob");
        write(&d.join("history.db.pre-v3.bak"), b"premig");
        write(&d.join("history.db.pre-restore-77"), b"prerest");
        write(&d.join("settings.json"), b"{}");
        write(&d.join("dark-clipboard.log"), b"log");

        move_damaged_aside(d, Timestamp(1234)).unwrap();

        for gone in [
            "history.db",
            "history.db-wal",
            "history.db-shm",
            "blobs",
            "history.db.pre-v3.bak",
            "history.db.pre-restore-77",
        ] {
            assert!(!d.join(gone).exists(), "{gone} still there");
        }
        let read = |n: &str| std::fs::read(d.join(n)).unwrap();
        assert_eq!(read("history.db.damaged-1234"), b"db");
        assert_eq!(read("history.db.damaged-1234-wal"), b"wal");
        assert_eq!(read("history.db.damaged-1234-shm"), b"shm");
        assert_eq!(
            std::fs::read(d.join("blobs.damaged-1234").join("ab").join("abcd")).unwrap(),
            b"blob"
        );
        assert_eq!(read("history.db.damaged-1234.pre-v3.bak"), b"premig");
        assert_eq!(read("history.db.damaged-1234.pre-restore-77"), b"prerest");
        // Unrelated files stay.
        assert_eq!(read("settings.json"), b"{}");
        assert_eq!(read("dark-clipboard.log"), b"log");
    }

    #[test]
    fn moving_aside_only_the_database_works_when_nothing_else_exists() {
        let tmp = tempfile::tempdir().unwrap();
        write(&tmp.path().join("history.db"), b"db");
        move_damaged_aside(tmp.path(), Timestamp(5)).unwrap();
        assert!(!tmp.path().join("history.db").exists());
        assert!(tmp.path().join("history.db.damaged-5").is_file());
    }

    #[test]
    fn moving_aside_without_a_database_is_not_found() {
        let tmp = tempfile::tempdir().unwrap();
        write(&tmp.path().join("blobs").join("ab").join("x"), b"blob");
        assert_eq!(
            move_damaged_aside(tmp.path(), Timestamp(5)),
            Err(StorageError::NotFound)
        );
        assert!(tmp.path().join("blobs").join("ab").join("x").is_file());
    }

    #[test]
    fn moving_aside_never_overwrites_and_then_moves_nothing() {
        let tmp = tempfile::tempdir().unwrap();
        let d = tmp.path();
        write(&d.join("history.db"), b"db");
        write(&d.join("history.db-wal"), b"wal");
        write(&d.join("blobs").join("ab").join("x"), b"blob");
        write(&d.join("blobs.damaged-9").join("keep"), b"older");
        assert_eq!(
            move_damaged_aside(d, Timestamp(9)),
            Err(StorageError::Io(std::io::ErrorKind::AlreadyExists))
        );
        assert!(d.join("history.db").is_file());
        assert!(d.join("history.db-wal").is_file());
        assert!(d.join("blobs").join("ab").join("x").is_file());
        assert!(!d.join("history.db.damaged-9").exists());
        assert_eq!(
            std::fs::read(d.join("blobs.damaged-9").join("keep")).unwrap(),
            b"older"
        );
    }

    #[test]
    fn after_moving_aside_the_store_starts_empty_and_the_damaged_copy_stays() {
        let tmp = tempfile::tempdir().unwrap();
        let d = tmp.path();
        write(
            &d.join("history.db"),
            b"not a database at all, synthetic test bytes",
        );
        write(&d.join("blobs").join("ab").join("abcdef"), b"blob");
        assert_eq!(
            open(d, OpenMode::ReadWrite).err(),
            Some(StorageError::Corrupt)
        );
        move_damaged_aside(d, Timestamp(42)).unwrap();
        let st = open(d, OpenMode::ReadWrite).unwrap();
        assert_eq!(st.stats().unwrap().clips, 0);
        drop(st);
        assert!(d.join("history.db.damaged-42").is_file());
        // The sweep of the new store did not touch the moved blobs.
        assert!(d
            .join("blobs.damaged-42")
            .join("ab")
            .join("abcdef")
            .is_file());
    }
}
