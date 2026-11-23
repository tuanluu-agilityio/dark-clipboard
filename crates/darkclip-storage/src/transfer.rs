//! Export and import files (Task 405). Stub.

use crate::archive::ArchiveLimits;
use crate::{Storage, StorageError};
use darkclip_core::{Clip, ClipId, Collection, Timestamp};
use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ExportReport {
    pub exported: u64,
    pub excluded_sensitive: u64,
    pub missing: u64,
}

pub struct ExportArchive {
    collection: Collection,
    clips: Vec<Clip>,
}

impl std::fmt::Debug for ExportArchive {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ExportArchive").finish_non_exhaustive()
    }
}

impl ExportArchive {
    pub fn open(_path: &Path, _limits: ArchiveLimits) -> Result<Self, StorageError> {
        Err(StorageError::NotFound)
    }

    pub fn collection(&self) -> &Collection {
        &self.collection
    }

    pub fn clips(&self) -> &[Clip] {
        &self.clips
    }

    pub fn blob(&mut self, _sha: &[u8; 32]) -> Result<Vec<u8>, StorageError> {
        Err(StorageError::NotFound)
    }
}

impl Storage {
    pub fn write_export(
        &self,
        _ids: &[ClipId],
        _collection: &Collection,
        _dest: &Path,
        _app_version: &str,
    ) -> Result<ExportReport, StorageError> {
        Err(StorageError::NotFound)
    }

    pub fn import_created_at(
        &mut self,
        _id: &ClipId,
        _created_at: Timestamp,
    ) -> Result<Timestamp, StorageError> {
        Err(StorageError::NotFound)
    }
}

#[cfg(test)]
mod tests;
