//! `settings.json`: load with recovery, atomic save (`docs/architecture/data-model.md` §10).
//!
//! Settings I/O lives here and not in core (GC-02). Nothing in this module logs or returns
//! file contents: a corrupt file is only renamed.

use crate::events::ApiError;
use darkclip_core::{Lang, PrivacyPolicy, RetentionPolicy};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

pub const SETTINGS_VERSION: u32 = 1;
const OS_MARKERS_PATH: &str = "privacy.respect_os_markers";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Shortcuts {
    /// Global shortcut that opens the popup.
    pub toggle_popup: String,
}

impl Default for Shortcuts {
    fn default() -> Self {
        Self {
            toggle_popup: "Ctrl+Shift+V".into(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PopupPosition {
    Cursor,
    Center,
}

fn default_true() -> bool {
    true
}

fn ser_lang<S: Serializer>(l: &Lang, s: S) -> Result<S::Ok, S::Error> {
    s.serialize_str(match l {
        Lang::En => "en",
        Lang::Vi => "vi",
    })
}

fn de_lang<'de, D: Deserializer<'de>>(d: D) -> Result<Lang, D::Error> {
    match String::deserialize(d)?.as_str() {
        "en" => Ok(Lang::En),
        "vi" => Ok(Lang::Vi),
        _ => Err(serde::de::Error::custom("unknown language")),
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UiSettings {
    #[serde(serialize_with = "ser_lang", deserialize_with = "de_lang")]
    pub language: Lang,
    pub popup_position: PopupPosition,
    /// "Enter also pastes" (Task 205): Enter restores the clip and pastes it into the previous
    /// app, like Ctrl+Enter. Off by default (Enter only restores, D-044). Missing in files
    /// written before Task 205.
    #[serde(default)]
    pub enter_pastes: bool,
    /// The popup closes when it loses the keyboard focus (Task 207). On by default; missing in
    /// files written before Task 207.
    #[serde(default = "default_true")]
    pub close_on_focus_loss: bool,
    /// Where the history window was last, per monitor (Task 211): the key names the monitor
    /// (its geometry, never a device name). Missing in older files.
    #[serde(default)]
    pub history_windows: BTreeMap<String, WindowRect>,
}

/// A saved window rectangle: the position is in physical pixels of the virtual screen, the
/// size in logical pixels (so it survives a DPI change of the same monitor).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct WindowRect {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

/// Monitors remembered at most (the oldest key by name is dropped beyond this).
pub const MAX_REMEMBERED_MONITORS: usize = 16;

impl Default for UiSettings {
    fn default() -> Self {
        Self {
            language: Lang::En,
            popup_position: PopupPosition::Cursor,
            enter_pastes: false,
            close_on_focus_loss: true,
            history_windows: BTreeMap::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct StartupSettings {
    pub launch_at_login: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Settings {
    pub settings_version: u32,
    #[serde(default)]
    pub privacy: PrivacyPolicy,
    #[serde(default)]
    pub retention: RetentionPolicy,
    #[serde(default)]
    pub shortcuts: Shortcuts,
    #[serde(default)]
    pub ui: UiSettings,
    #[serde(default)]
    pub startup: StartupSettings,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            settings_version: SETTINGS_VERSION,
            privacy: PrivacyPolicy::default(),
            retention: RetentionPolicy::default(),
            shortcuts: Shortcuts::default(),
            ui: UiSettings::default(),
            startup: StartupSettings::default(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LoadOutcome {
    Loaded,
    /// No file (first run) or an unreadable one; defaults in use, nothing renamed.
    Defaulted,
    /// The file was unusable and was renamed to `backup`; defaults in use.
    RecoveredFromCorrupt {
        backup: PathBuf,
    },
    /// Loaded, with these field paths forced back to their only allowed value.
    LoadedWithCorrections(Vec<&'static str>),
    /// Defaults are in use and the file must NOT be written: it exists but could not be read,
    /// is from a newer version, or could not be moved aside. The reason is static text.
    DefaultedReadOnly(&'static str),
}

/// Makes temp file names unique within the process (saves from several threads).
static SAVE_COUNTER: AtomicU64 = AtomicU64::new(0);

fn unix_ms() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_millis())
}

impl Settings {
    /// Never fails: the worst case is defaults. See [`LoadOutcome`].
    pub fn load(path: &Path) -> (Settings, LoadOutcome) {
        let bytes = match std::fs::read(path) {
            Ok(b) => b,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                return (Settings::default(), LoadOutcome::Defaulted)
            }
            Err(_) => {
                return (
                    Settings::default(),
                    LoadOutcome::DefaultedReadOnly("settings file unreadable"),
                )
            }
        };
        let Ok(mut value) = serde_json::from_slice::<serde_json::Value>(&bytes) else {
            return Self::recover(path, |from, to| std::fs::rename(from, to));
        };
        // Core rejects `respect_os_markers: false`; the app corrects it instead of
        // discarding the whole file (GC-07: the marker list cannot be switched off).
        let mut corrections: Vec<&'static str> = Vec::new();
        if let Some(flag) = value
            .get_mut("privacy")
            .and_then(|p| p.get_mut("respect_os_markers"))
        {
            if *flag == serde_json::Value::Bool(false) {
                *flag = serde_json::Value::Bool(true);
                corrections.push(OS_MARKERS_PATH);
            }
        }
        match serde_json::from_value::<Settings>(value) {
            Ok(s) if s.settings_version <= SETTINGS_VERSION => {
                let outcome = if corrections.is_empty() {
                    LoadOutcome::Loaded
                } else {
                    LoadOutcome::LoadedWithCorrections(corrections)
                };
                (s, outcome)
            }
            // A newer version wrote it: leave the file alone, never overwrite it.
            Ok(_) => (
                Settings::default(),
                LoadOutcome::DefaultedReadOnly("settings file is from a newer version"),
            ),
            Err(_) => Self::recover(path, |from, to| std::fs::rename(from, to)),
        }
    }

    fn recover(
        path: &Path,
        rename: impl FnOnce(&Path, &Path) -> std::io::Result<()>,
    ) -> (Settings, LoadOutcome) {
        let mut name = path.file_name().unwrap_or_default().to_os_string();
        name.push(format!(".corrupt-{}", unix_ms()));
        let backup = path.with_file_name(name);
        if rename(path, &backup).is_err() {
            // Still in place: saving would destroy the only copy.
            return (
                Settings::default(),
                LoadOutcome::DefaultedReadOnly("corrupt settings file could not be moved"),
            );
        }
        (
            Settings::default(),
            LoadOutcome::RecoveredFromCorrupt { backup },
        )
    }

    /// Temp file in the same directory, flushed, then renamed over the target. On any
    /// failure the temp file is removed and the old file is untouched.
    pub fn save(&self, path: &Path) -> Result<(), ApiError> {
        let json = serde_json::to_vec_pretty(self).map_err(|_| ApiError::Internal)?;
        let tmp = temp_path(path).ok_or(ApiError::Internal)?;
        let result = write_synced(&tmp, &json).and_then(|()| std::fs::rename(&tmp, path));
        if result.is_err() {
            let _ = std::fs::remove_file(&tmp);
        }
        result.map_err(|_| ApiError::Internal)
    }
}

/// A temp file name next to `path`, unique within the process (pid + counter, no clock).
fn temp_path(path: &Path) -> Option<PathBuf> {
    let mut name = path.file_name()?.to_os_string();
    let n = SAVE_COUNTER.fetch_add(1, Ordering::Relaxed);
    name.push(format!(".tmp-{}-{n}", std::process::id()));
    Some(path.with_file_name(name))
}

fn write_synced(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    use std::io::Write;
    let mut f = std::fs::File::create(path)?;
    f.write_all(bytes)?;
    f.sync_all()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn files(dir: &Path) -> Vec<String> {
        let mut v: Vec<String> = std::fs::read_dir(dir)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        v.sort();
        v
    }

    #[test]
    fn settings_missing_file_defaults() {
        let d = tempfile::tempdir().unwrap();
        let (s, o) = Settings::load(&d.path().join("settings.json"));
        assert_eq!(s, Settings::default());
        assert_eq!(o, LoadOutcome::Defaulted);
    }

    #[test]
    fn settings_round_trip() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("settings.json");
        let mut s = Settings::default();
        s.retention.max_items = 77;
        s.ui.language = Lang::Vi;
        s.ui.popup_position = PopupPosition::Center;
        s.startup.launch_at_login = true;
        s.ui.enter_pastes = true;
        s.save(&p).unwrap();
        let (back, o) = Settings::load(&p);
        assert_eq!(back, s);
        assert_eq!(o, LoadOutcome::Loaded);
    }

    #[test]
    fn enter_pastes_is_off_by_default_and_optional_in_older_files() {
        assert!(!Settings::default().ui.enter_pastes);
        // A file written before Task 205 has no `enter_pastes`: it loads, with the default.
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("settings.json");
        std::fs::write(
            &p,
            br#"{"settings_version":1,"ui":{"language":"vi","popup_position":"center"}}"#,
        )
        .unwrap();
        let (s, o) = Settings::load(&p);
        assert_eq!(o, LoadOutcome::Loaded);
        assert_eq!(s.ui.language, Lang::Vi);
        assert!(!s.ui.enter_pastes);
    }

    #[test]
    fn close_on_focus_loss_defaults_on_and_is_optional_in_older_files() {
        assert!(Settings::default().ui.close_on_focus_loss);
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("settings.json");
        std::fs::write(
            &p,
            br#"{"settings_version":1,"ui":{"language":"en","popup_position":"cursor"}}"#,
        )
        .unwrap();
        let (s, o) = Settings::load(&p);
        assert_eq!(o, LoadOutcome::Loaded);
        assert!(s.ui.close_on_focus_loss);
    }

    #[test]
    fn history_window_rects_are_optional_in_older_files_and_round_trip() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("settings.json");
        // A file written before Task 211 has no `history_windows`.
        std::fs::write(
            &p,
            br#"{"settings_version":1,"ui":{"language":"en","popup_position":"cursor","enter_pastes":true}}"#,
        )
        .unwrap();
        let (mut s, o) = Settings::load(&p);
        assert_eq!(o, LoadOutcome::Loaded);
        assert!(s.ui.history_windows.is_empty());
        assert!(s.ui.enter_pastes);
        s.ui.history_windows.insert(
            "0,0,1920x1080".into(),
            WindowRect {
                x: -10,
                y: 20,
                width: 980,
                height: 640,
            },
        );
        s.save(&p).unwrap();
        let (back, o) = Settings::load(&p);
        assert_eq!(o, LoadOutcome::Loaded);
        assert_eq!(back, s);
    }

    #[test]
    fn settings_atomic_save() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("settings.json");
        let mut s = Settings::default();
        s.save(&p).unwrap();
        // Overwrite: the temp file is renamed away, only the target remains.
        s.retention.max_items = 5;
        s.save(&p).unwrap();
        assert_eq!(files(d.path()), vec!["settings.json".to_string()]);
        assert_eq!(Settings::load(&p).0.retention.max_items, 5);
        // Simulated failure: the rename target is a directory. Its content is intact and
        // no temp file is left behind.
        let blocked = d.path().join("blocked");
        std::fs::create_dir(&blocked).unwrap();
        std::fs::write(blocked.join("keep"), b"x").unwrap();
        assert!(s.save(&blocked).is_err());
        assert_eq!(std::fs::read(blocked.join("keep")).unwrap(), b"x");
        assert_eq!(
            files(d.path()),
            vec!["blocked".to_string(), "settings.json".to_string()]
        );
        // Missing parent directory: error, nothing created.
        assert!(s
            .save(&d.path().join("nope").join("settings.json"))
            .is_err());
        assert!(!d.path().join("nope").exists());
    }

    #[test]
    fn settings_corrupt_recovers() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("settings.json");
        std::fs::write(&p, b"\x00 not json {{{").unwrap();
        let (s, o) = Settings::load(&p);
        assert_eq!(s, Settings::default());
        let LoadOutcome::RecoveredFromCorrupt { backup } = o else {
            panic!("expected recovery, got {o:?}");
        };
        let name = backup.file_name().unwrap().to_string_lossy().into_owned();
        let ts = name.strip_prefix("settings.json.corrupt-").unwrap();
        assert!(
            !ts.is_empty() && ts.chars().all(|c| c.is_ascii_digit()),
            "{name}"
        );
        assert_eq!(std::fs::read(&backup).unwrap(), b"\x00 not json {{{");
        assert!(!p.exists());
    }

    #[test]
    fn settings_valid_json_wrong_shape_recovers() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("settings.json");
        std::fs::write(
            &p,
            br#"{"settings_version":1,"retention":{"max_items":"many"}}"#,
        )
        .unwrap();
        let (s, o) = Settings::load(&p);
        assert_eq!(s, Settings::default());
        assert!(matches!(o, LoadOutcome::RecoveredFromCorrupt { .. }));
    }

    #[test]
    fn settings_newer_version_is_left_untouched() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("settings.json");
        let mut v = serde_json::to_value(Settings::default()).unwrap();
        v["settings_version"] = 99.into();
        std::fs::write(&p, v.to_string()).unwrap();
        let (s, o) = Settings::load(&p);
        assert_eq!(s, Settings::default());
        assert!(matches!(o, LoadOutcome::DefaultedReadOnly(_)), "{o:?}");
        assert_eq!(files(d.path()), vec!["settings.json".to_string()]);
        assert_eq!(std::fs::read_to_string(&p).unwrap(), v.to_string());
    }

    #[test]
    fn settings_unreadable_is_read_only_defaults() {
        let d = tempfile::tempdir().unwrap();
        // A directory in place of the file: reading fails with something other than NotFound.
        let p = d.path().join("settings.json");
        std::fs::create_dir(&p).unwrap();
        let (s, o) = Settings::load(&p);
        assert_eq!(s, Settings::default());
        assert!(matches!(o, LoadOutcome::DefaultedReadOnly(_)), "{o:?}");
        assert!(p.is_dir());
    }

    #[test]
    fn settings_corrupt_that_cannot_be_moved_is_read_only() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("settings.json");
        std::fs::write(&p, b"garbage").unwrap();
        let (s, o) = Settings::recover(&p, |_, _| {
            Err(std::io::Error::from(std::io::ErrorKind::PermissionDenied))
        });
        assert_eq!(s, Settings::default());
        assert!(matches!(o, LoadOutcome::DefaultedReadOnly(_)), "{o:?}");
        assert_eq!(std::fs::read(&p).unwrap(), b"garbage");
    }

    #[test]
    fn temp_names_are_unique_within_one_millisecond() {
        let p = Path::new("dir").join("settings.json");
        let names: Vec<PathBuf> = (0..1000).map(|_| temp_path(&p).unwrap()).collect();
        let distinct: std::collections::HashSet<&PathBuf> = names.iter().collect();
        assert_eq!(distinct.len(), names.len());
        assert_eq!(names[0].parent(), p.parent());
    }

    #[test]
    fn settings_concurrent_saves_leave_one_valid_file() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("settings.json");
        std::thread::scope(|sc| {
            for i in 0..8u32 {
                let p = p.clone();
                sc.spawn(move || {
                    let mut s = Settings::default();
                    s.retention.max_items = 100 + i;
                    for _ in 0..10 {
                        let _ = s.save(&p);
                    }
                });
            }
        });
        let (s, o) = Settings::load(&p);
        assert_eq!(o, LoadOutcome::Loaded);
        assert!((100..108).contains(&s.retention.max_items));
        assert_eq!(files(d.path()), vec!["settings.json".to_string()]);
    }

    #[test]
    fn settings_os_markers_not_configurable() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("settings.json");
        let mut v = serde_json::to_value(Settings::default()).unwrap();
        v["privacy"]["respect_os_markers"] = false.into();
        v["retention"]["max_items"] = 50.into();
        std::fs::write(&p, v.to_string()).unwrap();
        let (s, o) = Settings::load(&p);
        assert!(s.privacy.respect_os_markers);
        assert_eq!(s.retention.max_items, 50);
        assert_eq!(
            o,
            LoadOutcome::LoadedWithCorrections(vec!["privacy.respect_os_markers"])
        );
        // The file was not discarded.
        assert_eq!(files(d.path()), vec!["settings.json".to_string()]);
    }

    #[test]
    fn indefinite_pause_survives_save_and_load() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("settings.json");
        let mut s = Settings::default();
        s.privacy.mode = darkclip_core::CaptureMode::Paused {
            until: darkclip_core::FAR_FUTURE,
        };
        s.save(&p).unwrap();
        let (loaded, outcome) = Settings::load(&p);
        assert_eq!(outcome, LoadOutcome::Loaded);
        assert_eq!(loaded.privacy.mode, s.privacy.mode);
    }
}
