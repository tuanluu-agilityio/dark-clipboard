//! Backup, restore, export and import for the UI (Task 405). Stub.

use super::*;

/// What to export: a collection, or clips the user selected.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExportSelection {
    Collection(CollectionId),
    Clips(Vec<ClipId>),
}

/// What an import did, as counts per outcome (never content).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ImportReport {
    /// New clips stored.
    pub imported: u64,
    /// Clips already in history (same content): bumped, no second row.
    pub merged: u64,
    /// Secret-shaped text dropped under the "drop" setting.
    pub skipped_sensitive: u64,
    /// Kinds switched off in Privacy settings.
    pub skipped_kind: u64,
    /// Over the size limits of History settings.
    pub skipped_too_large: u64,
    /// Missing or damaged data in the file.
    pub skipped_invalid: u64,
}

impl ImportReport {
    pub fn skipped(&self) -> u64 {
        self.skipped_sensitive
            .saturating_add(self.skipped_kind)
            .saturating_add(self.skipped_too_large)
            .saturating_add(self.skipped_invalid)
    }
}

impl App {
    pub fn ui_backup(
        &self,
        c: &Caller,
        _dest: &Path,
    ) -> Result<darkclip_storage::BackupReport, ApiError> {
        ui_only(c)?;
        Err(ApiError::Internal)
    }

    pub fn ui_restore(
        &self,
        c: &Caller,
        _archive: &Path,
    ) -> Result<darkclip_storage::RestoreReport, ApiError> {
        ui_only(c)?;
        Err(ApiError::Internal)
    }

    pub fn ui_export(
        &self,
        c: &Caller,
        _what: ExportSelection,
        _dest: &Path,
    ) -> Result<darkclip_storage::ExportReport, ApiError> {
        ui_only(c)?;
        Err(ApiError::Internal)
    }

    pub fn ui_import(&self, c: &Caller, _archive: &Path) -> Result<ImportReport, ApiError> {
        ui_only(c)?;
        Err(ApiError::Internal)
    }
}
