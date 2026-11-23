//! What the desktop does when the history cannot be opened at start (Task 413, F407-03):
//! a message window instead of a silent quit (data-model §7).
//!
//! * A newer database (`SchemaTooNew`) is only explained; the file is never touched.
//! * A damaged one (`SQLITE_CORRUPT`, `SQLITE_NOTADB`, a failed `quick_check`) is explained
//!   and the user may move it aside (`darkclip_storage::move_damaged_aside`, a rename, never a
//!   delete) and start with an empty history, once.
//! * Anything else is explained with the folder and the app quits.
//!
//! The decisions are pure and tested here; the window itself is `winsys::message_box`.
//! Texts come from `darkclip_core::i18n` (GC-14); the folder path is shown to the user but
//! never logged (GC-06).
#![cfg_attr(not(windows), allow(dead_code))]

use darkclip_app::ApiError;
use darkclip_core::i18n::{t, Lang, Msg};
use darkclip_storage::StorageError;
use std::path::Path;

/// Why the app could not open its history.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StartProblem {
    /// Written by a newer Dark Clipboard: explain and quit; never modify it.
    TooNew,
    /// Damaged or not a database: offer to move it aside.
    Damaged,
    /// Anything else (permissions, disk): explain and quit.
    Other,
}

impl StartProblem {
    /// Static log name.
    pub fn name(self) -> &'static str {
        match self {
            StartProblem::TooNew => "schema_too_new",
            StartProblem::Damaged => "damaged",
            StartProblem::Other => "other",
        }
    }

    /// The text shown in the message window.
    pub fn msg(self) -> Msg {
        match self {
            StartProblem::TooNew => Msg::StartDbTooNew,
            StartProblem::Damaged => Msg::StartDbDamaged,
            StartProblem::Other => Msg::StartDbFailed,
        }
    }

    /// The text printed by `--headless`, which has no window and never moves anything.
    pub fn headless_msg(self) -> Msg {
        match self {
            StartProblem::Damaged => Msg::CliDbDamaged,
            other => other.msg(),
        }
    }

    /// Only a damaged history may be moved aside.
    pub fn offers_move_aside(self) -> bool {
        self == StartProblem::Damaged
    }
}

/// Classifies a failed `App::start`. `probe` is the error of a second, read-only
/// `Storage::open` of the same folder (the API error alone does not say "damaged").
pub fn classify(start: &ApiError, probe: Option<&StorageError>) -> StartProblem {
    match (start, probe) {
        (ApiError::SchemaTooNew, _)
        | (ApiError::Storage, Some(StorageError::SchemaTooNew { .. })) => StartProblem::TooNew,
        (ApiError::Storage, Some(StorageError::Corrupt)) => StartProblem::Damaged,
        _ => StartProblem::Other,
    }
}

/// Opens the history read-only once more to learn why `App::start` failed; `None` when it
/// opens. Runs only after a failed start, on the worker thread or in headless mode.
pub fn probe(data_dir: &Path) -> Option<StorageError> {
    darkclip_storage::Storage::open(
        data_dir,
        darkclip_storage::OpenMode::ReadOnly,
        std::sync::Arc::new(crate::SystemClock),
    )
    .err()
}

/// What to do after the message window.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Step {
    /// Rename the damaged history aside and try once more.
    MoveAsideAndRetry,
    Quit,
}

/// `chose_move` is the user's "Yes"; `already_moved` stops a second move in one run.
pub fn next_step(problem: StartProblem, chose_move: bool, already_moved: bool) -> Step {
    if problem.offers_move_aside() && chose_move && !already_moved {
        Step::MoveAsideAndRetry
    } else {
        Step::Quit
    }
}

/// `msg` in `lang` with `{path}` replaced by the data folder.
pub fn text(lang: Lang, msg: Msg, data_dir: &Path) -> String {
    t(lang, msg).replace("{path}", &data_dir.display().to_string())
}

/// Shows `msg` in a message window (Windows; elsewhere on standard error). With `ask`, the
/// window has Yes/No and the answer is returned; otherwise it only informs and returns false.
pub fn show(lang: Lang, msg: Msg, data_dir: &Path, ask: bool) -> bool {
    let body = text(lang, msg, data_dir);
    #[cfg(windows)]
    {
        crate::winsys::message_box(t(lang, Msg::UiAppName), &body, ask)
    }
    #[cfg(not(windows))]
    {
        let _ = ask;
        eprintln!("{body}");
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn a_newer_database_is_too_new_whatever_the_probe_says() {
        assert_eq!(
            classify(&ApiError::SchemaTooNew, None),
            StartProblem::TooNew
        );
        assert_eq!(
            classify(&ApiError::SchemaTooNew, Some(&StorageError::Corrupt)),
            StartProblem::TooNew
        );
        let too_new = StorageError::SchemaTooNew {
            found: 9,
            supported: 3,
        };
        assert_eq!(
            classify(&ApiError::Storage, Some(&too_new)),
            StartProblem::TooNew
        );
    }

    #[test]
    fn a_corrupt_probe_means_damaged() {
        assert_eq!(
            classify(&ApiError::Storage, Some(&StorageError::Corrupt)),
            StartProblem::Damaged
        );
    }

    #[test]
    fn anything_else_is_other() {
        for probe in [
            None,
            Some(StorageError::Io(std::io::ErrorKind::PermissionDenied)),
            Some(StorageError::Sqlite("Busy/5".into())),
            Some(StorageError::NotFound),
            Some(StorageError::ReadOnly),
        ] {
            assert_eq!(
                classify(&ApiError::Storage, probe.as_ref()),
                StartProblem::Other
            );
        }
        // A start that failed for a reason other than storage is never "damaged".
        assert_eq!(
            classify(&ApiError::Internal, Some(&StorageError::Corrupt)),
            StartProblem::Other
        );
    }

    #[test]
    fn each_problem_has_its_message_and_only_damaged_offers_the_move() {
        assert_eq!(StartProblem::TooNew.msg(), Msg::StartDbTooNew);
        assert_eq!(StartProblem::Damaged.msg(), Msg::StartDbDamaged);
        assert_eq!(StartProblem::Other.msg(), Msg::StartDbFailed);
        assert!(StartProblem::Damaged.offers_move_aside());
        assert!(!StartProblem::TooNew.offers_move_aside());
        assert!(!StartProblem::Other.offers_move_aside());
        assert_eq!(StartProblem::Damaged.name(), "damaged");
        // Headless has no Yes/No: its damaged text does not ask.
        assert_eq!(StartProblem::Damaged.headless_msg(), Msg::CliDbDamaged);
        assert_eq!(StartProblem::TooNew.headless_msg(), Msg::StartDbTooNew);
        assert_eq!(StartProblem::Other.headless_msg(), Msg::StartDbFailed);
    }

    #[test]
    fn the_move_happens_only_for_damaged_on_yes_and_only_once() {
        use StartProblem::*;
        assert_eq!(next_step(Damaged, true, false), Step::MoveAsideAndRetry);
        assert_eq!(next_step(Damaged, false, false), Step::Quit);
        assert_eq!(next_step(Damaged, true, true), Step::Quit);
        for p in [TooNew, Other] {
            for (yes, moved) in [(true, false), (false, false), (true, true)] {
                assert_eq!(next_step(p, yes, moved), Step::Quit, "{p:?}");
            }
        }
    }

    #[test]
    fn texts_name_the_folder_in_both_languages() {
        let dir = PathBuf::from("C:\\Users\\someone\\AppData\\Local\\DarkClipboard");
        for lang in [Lang::En, Lang::Vi] {
            for m in [
                Msg::StartDbDamaged,
                Msg::StartDbTooNew,
                Msg::StartDbFailed,
                Msg::CliDbDamaged,
                Msg::StartMoveAsideFailed,
            ] {
                let s = text(lang, m, &dir);
                assert!(
                    s.contains("AppData\\Local\\DarkClipboard"),
                    "{m:?} {lang:?}"
                );
                assert!(!s.contains("{path}"), "{m:?} {lang:?}");
            }
            // The pipe message has no placeholder and is returned as is.
            assert_eq!(
                text(lang, Msg::StartPipeInUse, &dir),
                t(lang, Msg::StartPipeInUse)
            );
        }
    }
}
