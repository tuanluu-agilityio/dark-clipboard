//! Embedded SQL migrations guarded by `PRAGMA user_version` (data-model §7).
//! No migration framework. Each migration runs in one transaction; the file sets
//! `user_version` itself, so a failed migration leaves the old version in place.

use crate::StorageError;
use rusqlite::Connection;

pub(crate) struct Migration {
    pub version: u32,
    pub sql: &'static str,
}

pub(crate) const MIGRATIONS: &[Migration] = &[
    Migration {
        version: 1,
        sql: include_str!("../migrations/0001_init.sql"),
    },
    Migration {
        version: 2,
        sql: include_str!("../migrations/0002_organization.sql"),
    },
    Migration {
        version: 3,
        sql: include_str!("../migrations/0003_no_export.sql"),
    },
];

/// The highest version `migrations` brings a database to.
pub(crate) fn target_version(migrations: &[Migration]) -> u32 {
    migrations.iter().map(|m| m.version).max().unwrap_or(0)
}

pub(crate) fn user_version(conn: &Connection) -> Result<u32, StorageError> {
    let v: i64 = conn.pragma_query_value(None, "user_version", |r| r.get(0))?;
    // A negative header value can only come from another writer.
    u32::try_from(v).map_err(|_| StorageError::Sqlite("UserVersionOutOfRange".into()))
}

/// Applies every migration newer than the current `user_version`, in order.
pub(crate) fn run(conn: &mut Connection, migrations: &[Migration]) -> Result<(), StorageError> {
    for m in migrations {
        if m.version <= user_version(conn)? {
            continue;
        }
        let tx = conn.transaction()?;
        tx.execute_batch(m.sql)?;
        // Every migration file must set its own version; refuse to commit otherwise.
        let after: i64 = tx.pragma_query_value(None, "user_version", |r| r.get(0))?;
        if after != i64::from(m.version) {
            return Err(StorageError::Sqlite("MigrationVersionNotSet".into()));
        }
        tx.commit()?;
    }
    Ok(())
}
