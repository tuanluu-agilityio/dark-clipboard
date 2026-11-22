//! System helpers of the settings window (Task 207): the running programs for "pick running
//! app" and opening the data folder.
//!
//! Programs are listed by image name only: no window title is ever read (SR-11). The folder is
//! opened by starting the platform's file manager with the path as ONE argument (no shell, no
//! command string, nothing the user typed is part of the command).

use std::path::{Path, PathBuf};

/// Names sorted without regard to case, empty ones and case-insensitive repeats dropped.
pub fn unique_sorted(mut names: Vec<String>) -> Vec<String> {
    names.retain(|n| !n.trim().is_empty());
    names.sort_by_key(|n| n.to_lowercase());
    names.dedup_by(|a, b| a.eq_ignore_ascii_case(b));
    names
}

/// The running programs' file names (Windows only for now; empty elsewhere).
pub fn running_programs() -> Vec<String> {
    #[cfg(windows)]
    {
        unique_sorted(crate::winsys::running_image_names())
    }
    #[cfg(not(windows))]
    {
        Vec::new()
    }
}

/// The folder to open: an absolute path to an existing directory, otherwise nothing.
pub fn folder_to_open(path: &Path) -> Option<PathBuf> {
    (path.is_absolute() && path.is_dir()).then(|| path.to_path_buf())
}

/// Opens `path` in the file manager. True when the file manager was started.
pub fn open_folder(path: &Path) -> bool {
    let Some(dir) = folder_to_open(path) else {
        return false;
    };
    #[cfg(windows)]
    let program = std::env::var_os("SystemRoot")
        .map(|r| PathBuf::from(r).join("explorer.exe"))
        .unwrap_or_else(|| PathBuf::from("explorer.exe"));
    #[cfg(target_os = "macos")]
    let program = PathBuf::from("/usr/bin/open");
    #[cfg(not(any(windows, target_os = "macos")))]
    let program = PathBuf::from("xdg-open");
    std::process::Command::new(program)
        .arg(dir)
        .spawn()
        .map(|mut child| {
            // Explorer returns at once; reap it off the UI thread so it never lingers.
            let _ = std::thread::Builder::new()
                .name("darkclip-open-folder".into())
                .spawn(move || {
                    let _ = child.wait();
                });
        })
        .is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn program_names_are_sorted_and_deduplicated_ignoring_case() {
        let v = unique_sorted(
            ["b.exe", "A.exe", "a.EXE", "", "  ", "c.exe", "B.exe"]
                .map(String::from)
                .to_vec(),
        );
        assert_eq!(v, vec!["A.exe", "b.exe", "c.exe"]);
    }

    #[test]
    fn only_an_existing_absolute_directory_is_opened() {
        let d = tempfile::tempdir().unwrap();
        assert_eq!(folder_to_open(d.path()), Some(d.path().to_path_buf()));
        assert_eq!(folder_to_open(&d.path().join("missing")), None);
        let file = d.path().join("f.txt");
        std::fs::write(&file, b"x").unwrap();
        assert_eq!(folder_to_open(&file), None);
        assert_eq!(folder_to_open(Path::new("relative")), None);
        assert_eq!(folder_to_open(Path::new("")), None);
    }

    #[test]
    fn nothing_is_started_for_a_bad_path() {
        assert!(!open_folder(Path::new("no-such-folder")));
    }
}
