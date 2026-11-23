//! Backup, restore, export and import for the UI (Task 405). Each runs as one command on
//! the actor thread (the only writer, D-028); this side only resolves what to export and
//! waits. UI only: other callers get `PermissionDenied` (the P2 CLI and MCP get
//! `export_collection` through its grant, never a path of their choosing; T-11).

use super::*;
use crate::capture::TransferOp;
use darkclip_core::{CollectionKind, CollectionSort, Msg, SchemaVersion};
use darkclip_storage::{BackupReport, ExportReport, RestoreReport};

pub use crate::capture::{ImportReport, TransferReply};

/// What to export: a collection, or clips the user selected.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExportSelection {
    Collection(CollectionId),
    Clips(Vec<ClipId>),
}

/// A transfer started with `App::ui_start`.
pub enum UiTransfer {
    Backup(PathBuf),
    /// `free_bytes`: free space of the data folder, measured by the caller.
    Restore {
        archive: PathBuf,
        free_bytes: Option<u64>,
    },
    Export {
        what: ExportSelection,
        dest: PathBuf,
    },
    Import(PathBuf),
}

/// Prints the operation only, never a path (GC-06).
impl std::fmt::Debug for UiTransfer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Backup(_) => "Backup",
            Self::Restore { .. } => "Restore",
            Self::Export { .. } => "Export",
            Self::Import(_) => "Import",
        })
    }
}

/// The pending result of a transfer.
pub struct TransferWait(mpsc::Receiver<Result<TransferReply, ApiError>>);

impl TransferWait {
    /// Blocks until the actor answers. `AppNotRunning` if it stopped without answering.
    pub fn wait(self) -> Result<TransferReply, ApiError> {
        self.0.recv().map_err(|_| ApiError::AppNotRunning)?
    }
}

/// How long a queued transfer may wait for the actor before it is dropped unexecuted.
const TRANSFER_DEADLINE: Duration = Duration::from_secs(24 * 60 * 60);
/// Folder of `export_collection` files: `<data>/exports/` (T-11: never a caller's path).
pub const EXPORTS_DIR: &str = "exports";

impl App {
    /// Writes a backup of the whole history (masked clips excluded) to `dest`, which the
    /// user chose. The file appears only when complete.
    pub fn ui_backup(&self, c: &Caller, dest: &Path) -> Result<BackupReport, ApiError> {
        ui_only(c)?;
        match self
            .ui_start(c, UiTransfer::Backup(dest.to_path_buf()))?
            .wait()?
        {
            TransferReply::Backup(r) => Ok(r),
            _ => Err(ApiError::Internal),
        }
    }

    /// Replaces the history with the backup at `archive` after validating all of it. A
    /// newer backup is `SchemaTooNew`; an invalid one `InvalidArgument`. Any failure leaves
    /// the history as it was.
    pub fn ui_restore(
        &self,
        c: &Caller,
        archive: &Path,
        free_bytes: Option<u64>,
    ) -> Result<RestoreReport, ApiError> {
        ui_only(c)?;
        let start = UiTransfer::Restore {
            archive: archive.to_path_buf(),
            free_bytes,
        };
        match self.ui_start(c, start)?.wait()? {
            TransferReply::Restore(r) => Ok(r),
            _ => Err(ApiError::Internal),
        }
    }

    /// Exports a collection or the selected clips to `dest`. Masked and sensitive clips are
    /// left out and counted.
    pub fn ui_export(
        &self,
        c: &Caller,
        what: ExportSelection,
        dest: &Path,
    ) -> Result<ExportReport, ApiError> {
        ui_only(c)?;
        let start = UiTransfer::Export {
            what,
            dest: dest.to_path_buf(),
        };
        match self.ui_start(c, start)?.wait()? {
            TransferReply::Export(r) => Ok(r),
            _ => Err(ApiError::Internal),
        }
    }

    /// Imports an export file; reports counts per outcome.
    pub fn ui_import(&self, c: &Caller, archive: &Path) -> Result<ImportReport, ApiError> {
        ui_only(c)?;
        match self
            .ui_start(c, UiTransfer::Import(archive.to_path_buf()))?
            .wait()?
        {
            TransferReply::Import(r) => Ok(r),
            _ => Err(ApiError::Internal),
        }
    }

    /// Starts a transfer and returns at once; the caller waits for the result on a thread of
    /// its own (`TransferWait::wait`), so its worker stays free for mode changes and searches.
    /// The actor runs it as one command: a capture-mode change sent after this call is
    /// applied before any clipboard change that arrives after it (one FIFO queue).
    pub fn ui_start(&self, c: &Caller, what: UiTransfer) -> Result<TransferWait, ApiError> {
        ui_only(c)?;
        let op = match what {
            UiTransfer::Backup(p) => TransferOp::Backup(p),
            UiTransfer::Restore {
                archive,
                free_bytes,
            } => TransferOp::Restore(archive, free_bytes),
            UiTransfer::Export { what, dest } => {
                let (ids, collection) = self.export_target(what)?;
                TransferOp::Export {
                    ids,
                    collection,
                    dest,
                }
            }
            UiTransfer::Import(p) => TransferOp::Import(p),
        };
        let (tx, rx) = mpsc::channel();
        // The actor skips a command whose deadline has passed; a transfer queued behind
        // another long one must still run, so the deadline is far away.
        let now = Instant::now();
        let deadline = now.checked_add(TRANSFER_DEADLINE).unwrap_or(now);
        self.cap.send(ActorCommand::Transfer {
            op,
            deadline,
            reply: tx,
        })?;
        Ok(TransferWait(rx))
    }

    /// The clips and the collection document of an export.
    fn export_target(
        &self,
        what: ExportSelection,
    ) -> Result<(Vec<ClipId>, darkclip_core::Collection), ApiError> {
        match what {
            ExportSelection::Collection(id) => {
                let st = self.reader();
                let col = st
                    .list_collections()
                    .map_err(|e| map_storage(&e))?
                    .into_iter()
                    .find(|c| c.id == id)
                    .ok_or(ApiError::NotFound)?;
                let ids = st.collection_clip_ids(&id).map_err(|e| map_storage(&e))?;
                Ok((ids, col))
            }
            ExportSelection::Clips(ids) => {
                let now = self.clock.now();
                let col = darkclip_core::Collection {
                    schema_version: SchemaVersion,
                    id: CollectionId::new_v7(now),
                    name: darkclip_core::t(self.lang(), Msg::ExportSelectedName).to_string(),
                    kind: CollectionKind::User,
                    system_key: None,
                    color: None,
                    created_at: now,
                    updated_at: now,
                    clip_count: 0,
                    sort: CollectionSort::Manual,
                };
                Ok((ids, col))
            }
        }
    }

    /// `export_collection` for the UI: writes `<data>/exports/<random id>.dcexport`.
    pub(super) fn export_collection_ui(&self, col: CollectionId) -> Result<ExportResult, ApiError> {
        let (ids, collection) = self.export_target(ExportSelection::Collection(col))?;
        let dir = self.data_dir.join(EXPORTS_DIR);
        std::fs::create_dir_all(&dir).map_err(|_| ApiError::Storage)?;
        let dest = dir.join(format!("{}.dcexport", ClipId::new_v7(self.clock.now())));
        let (tx, rx) = mpsc::channel();
        let now = Instant::now();
        self.cap.send(ActorCommand::Transfer {
            op: TransferOp::Export {
                ids,
                collection,
                dest: dest.clone(),
            },
            deadline: now.checked_add(TRANSFER_DEADLINE).unwrap_or(now),
            reply: tx,
        })?;
        let r = match TransferWait(rx).wait()? {
            TransferReply::Export(r) => r,
            _ => return Err(ApiError::Internal),
        };
        Ok(ExportResult {
            path: dest.display().to_string(),
            clip_count: r.exported,
            excluded_sensitive: r.excluded_sensitive,
        })
    }
}
