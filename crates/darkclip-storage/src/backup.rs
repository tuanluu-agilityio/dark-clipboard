//! Backup and restore (Task 405). Stub.

use crate::archive::ArchiveLimits;
use crate::{Storage, StorageError};
use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct BackupReport {
    pub clips: u64,
    pub blobs: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct RestoreReport {
    pub clips: u64,
}

impl Storage {
    pub fn write_backup(
        &mut self,
        _dest: &Path,
        _app_version: &str,
    ) -> Result<BackupReport, StorageError> {
        Err(StorageError::NotFound)
    }

    pub fn restore_backup(&mut self, archive: &Path) -> Result<RestoreReport, StorageError> {
        self.restore_backup_with(archive, ArchiveLimits::BACKUP)
    }

    pub fn restore_backup_with(
        &mut self,
        _archive: &Path,
        _limits: ArchiveLimits,
    ) -> Result<RestoreReport, StorageError> {
        Err(StorageError::NotFound)
    }
}

#[cfg(test)]
mod tests;
