//! Storage errors. No variant carries clip content, SQL parameters, SQLite's
//! free-text message or a file path (GC-06): only codes and kinds.

use darkclip_core::CoreError;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StorageError {
    /// SQLite or rusqlite failure, as a code name only (no SQL parameters, no message).
    Sqlite(String),
    /// Filesystem failure, kind only (no path).
    Io(std::io::ErrorKind),
    /// The database was written by a newer Dark Clipboard. Never downgraded.
    SchemaTooNew {
        found: u32,
        supported: u32,
    },
    NotFound,
    /// A write was attempted on a read-only store, or a read-only open found a
    /// schema that needs a migration.
    ReadOnly,
    /// The disk (or the database size limit) is full: SQLITE_FULL or an I/O `StorageFull`.
    /// The capture actor reacts with one retention run and one retry (F-02).
    StorageFull,
    /// Compaction needs free disk space of at least the database size (Task 411).
    InsufficientSpace,
    /// The clip violates a schema constraint (`Clip::validate`). Field name only.
    /// Added to the Task 007 interface: `upsert_clip` must call `Clip::validate`.
    Invalid(CoreError),
    /// The request is not allowed for this target (a note or tags on a masked clip,
    /// renaming or deleting a system collection, reordering a non-manual collection,
    /// too many tags). A fixed description only, never user input.
    InvalidArgument(&'static str),
    /// A backup or export archive was refused (Task 405). Reason code only.
    Archive(crate::ArchiveError),
}

impl std::fmt::Display for StorageError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Sqlite(code) => write!(f, "database error ({code})"),
            Self::Io(kind) => write!(f, "file error ({kind:?})"),
            Self::SchemaTooNew { found, supported } => write!(
                f,
                "database schema version {found} is newer than supported version {supported}"
            ),
            Self::StorageFull => f.write_str("storage is full"),
            Self::InsufficientSpace => f.write_str("not enough free disk space"),
            Self::NotFound => f.write_str("not found"),
            Self::ReadOnly => f.write_str("storage is read-only"),
            Self::Invalid(e) => write!(f, "invalid clip: {e}"),
            Self::InvalidArgument(what) => write!(f, "invalid argument: {what}"),
            Self::Archive(e) => write!(f, "archive refused: {e:?}"),
        }
    }
}

impl std::error::Error for StorageError {}

impl From<std::io::Error> for StorageError {
    fn from(e: std::io::Error) -> Self {
        if e.kind() == std::io::ErrorKind::StorageFull {
            return Self::StorageFull;
        }
        Self::Io(e.kind())
    }
}

impl From<CoreError> for StorageError {
    fn from(e: CoreError) -> Self {
        Self::Invalid(e)
    }
}

/// Maps a rusqlite error to a code. The message part of `SqliteFailure` and every
/// payload (parameter names, column names, inner conversion errors that may echo a
/// value) are dropped on purpose.
impl From<rusqlite::Error> for StorageError {
    fn from(e: rusqlite::Error) -> Self {
        use rusqlite::Error as E;
        let code = match e {
            E::SqliteFailure(err, _) if err.code == rusqlite::ErrorCode::DiskFull => {
                return Self::StorageFull;
            }
            E::SqliteFailure(err, _message_dropped) => {
                format!("{:?}/{}", err.code, err.extended_code)
            }
            E::QueryReturnedNoRows => return Self::NotFound,
            E::FromSqlConversionFailure(..) => "FromSqlConversionFailure".to_owned(),
            E::IntegralValueOutOfRange(..) => "IntegralValueOutOfRange".to_owned(),
            E::InvalidColumnType(..) => "InvalidColumnType".to_owned(),
            E::InvalidColumnIndex(..) => "InvalidColumnIndex".to_owned(),
            E::InvalidColumnName(..) => "InvalidColumnName".to_owned(),
            E::InvalidParameterName(..) => "InvalidParameterName".to_owned(),
            E::InvalidParameterCount(..) => "InvalidParameterCount".to_owned(),
            E::ToSqlConversionFailure(..) => "ToSqlConversionFailure".to_owned(),
            E::ExecuteReturnedResults => "ExecuteReturnedResults".to_owned(),
            E::StatementChangedRows(..) => "StatementChangedRows".to_owned(),
            E::InvalidPath(..) => "InvalidPath".to_owned(),
            E::NulError(..) => "NulError".to_owned(),
            _ => "Other".to_owned(),
        };
        Self::Sqlite(code)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn disk_full_maps_to_storage_full() {
        let sqlite = rusqlite::Error::SqliteFailure(
            rusqlite::ffi::Error::new(rusqlite::ffi::SQLITE_FULL),
            None,
        );
        assert_eq!(StorageError::from(sqlite), StorageError::StorageFull);
        let io = std::io::Error::from(std::io::ErrorKind::StorageFull);
        assert_eq!(StorageError::from(io), StorageError::StorageFull);
        let other = std::io::Error::from(std::io::ErrorKind::PermissionDenied);
        assert_eq!(
            StorageError::from(other),
            StorageError::Io(std::io::ErrorKind::PermissionDenied)
        );
    }
}
