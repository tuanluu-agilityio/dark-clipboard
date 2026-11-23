//! Zip archives for backups and exports (Task 405). Stub: types only.

use crate::StorageError;

pub const BACKUP_FORMAT: u32 = 1;
pub const EXPORT_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArchiveError {
    NotAnArchive,
    BadEntryName,
    DuplicateEntry,
    TooManyEntries,
    TooLarge,
    ChecksumMismatch,
    BadManifest,
    NewerFormat,
    Corrupt,
    Unsupported,
}

impl From<ArchiveError> for StorageError {
    fn from(e: ArchiveError) -> Self {
        Self::Archive(e)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ArchiveLimits {
    pub max_entries: usize,
    pub max_entry_bytes: u64,
    pub max_total_bytes: u64,
    pub max_json_bytes: u64,
}

const MIB: u64 = 1024 * 1024;

impl ArchiveLimits {
    pub const BACKUP: Self = Self {
        max_entries: 200_002,
        max_entry_bytes: 4 * 1024 * MIB,
        max_total_bytes: 8 * 1024 * MIB,
        max_json_bytes: 64 * MIB,
    };
    pub const EXPORT: Self = Self {
        max_entries: 50_001,
        max_entry_bytes: 256 * MIB,
        max_total_bytes: 2 * 1024 * MIB,
        max_json_bytes: 64 * MIB,
    };
}
