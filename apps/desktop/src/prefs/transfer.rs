//! Backup, restore, export and import in the windows (Task 405): suggested file names, the
//! restore confirmation, and the messages for results and errors. Pure logic, tested
//! everywhere; the Slint wiring is `ui/settings_host.rs` and `ui/history_host.rs`, the file
//! picker `crate::filedlg`. Paths are the user's own choice: shown nowhere, never logged.

use darkclip_app::{ApiError, ImportReport};
use darkclip_core::i18n::{t, Lang, Msg};
use darkclip_core::Timestamp;
use darkclip_storage::ExportReport;
use std::path::PathBuf;

pub const BACKUP_EXT: &str = "dcbackup";
pub const EXPORT_EXT: &str = "dcexport";

/// `dark-clipboard-YYYY-MM-DD.dcbackup` (UTC date).
pub fn backup_file_name(now: Timestamp) -> String {
    format!("dark-clipboard-{}.{BACKUP_EXT}", utc_date(now))
}

/// `dark-clipboard-export-YYYY-MM-DD.dcexport` (UTC date).
pub fn export_file_name(now: Timestamp) -> String {
    format!("dark-clipboard-export-{}.{EXPORT_EXT}", utc_date(now))
}

/// `YYYY-MM-DD` of `now` in UTC.
fn utc_date(now: Timestamp) -> String {
    let mut s = now.to_rfc3339();
    s.truncate(10);
    s
}

/// The message for a failed backup, restore, export or import.
pub fn error_msg(e: &ApiError) -> Msg {
    use darkclip_app::events::{FILE_INVALID, FILE_TOO_LARGE, IMPORT_PRIVATE};
    match e {
        ApiError::SchemaTooNew => Msg::TransferNewer,
        ApiError::InvalidArgument(w) if *w == FILE_TOO_LARGE => Msg::TransferTooLarge,
        ApiError::InvalidArgument(w) if *w == FILE_INVALID => Msg::TransferInvalid,
        ApiError::InvalidArgument(w) if *w == IMPORT_PRIVATE => Msg::TransferPrivate,
        _ => Msg::TransferFailed,
    }
}

/// "Imported 3 new clips; 1 were already in your history" plus the skipped counts, if any.
pub fn import_text(lang: Lang, r: &ImportReport) -> String {
    let mut out = t(lang, Msg::TransferImportDone)
        .replace("{n}", &r.imported.to_string())
        .replace("{m}", &r.merged.to_string());
    if r.skipped() > 0 {
        out.push_str(". ");
        out.push_str(
            &t(lang, Msg::TransferImportSkipped)
                .replace("{s}", &r.skipped_sensitive.to_string())
                .replace("{k}", &r.skipped_kind.to_string())
                .replace("{l}", &r.skipped_too_large.to_string())
                .replace("{d}", &r.skipped_invalid.to_string()),
        );
    }
    out
}

/// "Exported 3 clips" plus the masked or sensitive clips left out, if any.
pub fn export_text(lang: Lang, r: &ExportReport) -> String {
    let mut out = t(lang, Msg::TransferExportDone).replace("{n}", &r.exported.to_string());
    if r.excluded_sensitive > 0 {
        out.push_str(". ");
        out.push_str(
            &t(lang, Msg::TransferExportExcluded).replace("{n}", &r.excluded_sensitive.to_string()),
        );
    }
    out
}

/// The restore confirmation: nothing is replaced without it. Holds the chosen file.
#[derive(Default)]
pub struct RestoreFlow {
    file: Option<PathBuf>,
}

/// Never prints the path (GC-06).
impl std::fmt::Debug for RestoreFlow {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RestoreFlow")
            .field("open", &self.file.is_some())
            .finish()
    }
}

impl RestoreFlow {
    pub fn ask(&mut self, file: PathBuf) {
        self.file = Some(file);
    }

    pub fn cancel(&mut self) {
        self.file = None;
    }

    /// The file when the question was open; closes it.
    pub fn confirm(&mut self) -> Option<PathBuf> {
        self.file.take()
    }

    pub fn is_open(&self) -> bool {
        self.file.is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const DAY: Timestamp = Timestamp(1_795_392_000_000); // 2026-11-23T00:00:00Z

    #[test]
    fn suggested_names_carry_the_date_and_extension() {
        assert_eq!(
            backup_file_name(Timestamp(DAY.0 + 3_600_000)),
            "dark-clipboard-2026-11-23.dcbackup"
        );
        assert_eq!(
            export_file_name(DAY),
            "dark-clipboard-export-2026-11-23.dcexport"
        );
    }

    #[test]
    fn errors_have_their_own_messages() {
        assert_eq!(error_msg(&ApiError::SchemaTooNew), Msg::TransferNewer);
        assert_eq!(
            error_msg(&ApiError::InvalidArgument(
                darkclip_app::events::FILE_TOO_LARGE
            )),
            Msg::TransferTooLarge
        );
        assert_eq!(
            error_msg(&ApiError::InvalidArgument(
                darkclip_app::events::FILE_INVALID
            )),
            Msg::TransferInvalid
        );
        assert_eq!(
            error_msg(&ApiError::InvalidArgument(
                darkclip_app::events::IMPORT_PRIVATE
            )),
            Msg::TransferPrivate
        );
        for e in [ApiError::Storage, ApiError::Internal, ApiError::NotFound] {
            assert_eq!(error_msg(&e), Msg::TransferFailed);
        }
    }

    #[test]
    fn import_results_name_every_count() {
        let r = ImportReport {
            imported: 3,
            merged: 1,
            ..ImportReport::default()
        };
        assert_eq!(
            import_text(Lang::En, &r),
            "Imported 3 new clips; 1 were already in your history"
        );
        let r = ImportReport {
            imported: 1,
            merged: 0,
            skipped_sensitive: 2,
            skipped_kind: 3,
            skipped_too_large: 4,
            skipped_invalid: 5,
        };
        let text = import_text(Lang::En, &r);
        assert!(text.starts_with("Imported 1 new clips; 0 were"));
        assert!(text.ends_with(
            "Not imported: 2 secrets, 3 turned-off kinds or file lists, 4 too large, 5 damaged"
        ));
        assert!(import_text(Lang::Vi, &r).contains("Không nhập: 2 bí mật"));
    }

    #[test]
    fn export_results_name_what_was_left_out() {
        let r = ExportReport {
            exported: 4,
            excluded_sensitive: 0,
            missing: 0,
        };
        assert_eq!(export_text(Lang::En, &r), "Exported 4 clips");
        let r = ExportReport {
            exported: 4,
            excluded_sensitive: 2,
            missing: 1,
        };
        assert_eq!(
            export_text(Lang::En, &r),
            "Exported 4 clips. Masked or sensitive clips left out: 2"
        );
    }

    #[test]
    fn a_restore_needs_the_confirmation() {
        let mut f = RestoreFlow::default();
        assert_eq!(
            f.confirm(),
            None,
            "nothing is restored without the question"
        );
        f.ask(PathBuf::from("backup.dcbackup"));
        assert!(f.is_open());
        assert!(!format!("{f:?}").contains("backup.dcbackup"));
        f.cancel();
        assert!(!f.is_open());
        assert_eq!(f.confirm(), None);
        f.ask(PathBuf::from("b.dcbackup"));
        assert_eq!(f.confirm(), Some(PathBuf::from("b.dcbackup")));
        assert_eq!(f.confirm(), None, "one confirmation, one restore");
    }
}
