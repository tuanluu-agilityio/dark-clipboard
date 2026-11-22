//! `settings.json`: load with recovery, atomic save (`docs/architecture/data-model.md` §10).
//!
//! Settings I/O lives here and not in core (GC-02). Nothing in this module logs or returns
//! file contents: a corrupt file is only renamed.

use crate::events::ApiError;
use darkclip_core::{Lang, PrivacyPolicy, RetentionPolicy};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::path::{Path, PathBuf};

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
}

impl Default for UiSettings {
    fn default() -> Self {
        Self {
            language: Lang::En,
            popup_position: PopupPosition::Cursor,
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
}

fn unix_ms() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_millis())
}

impl Settings {
    /// Never fails: the worst case is defaults. See [`LoadOutcome`].
    pub fn load(path: &Path) -> (Settings, LoadOutcome) {
        let Ok(bytes) = std::fs::read(path) else {
            return (Settings::default(), LoadOutcome::Defaulted);
        };
        let Ok(mut value) = serde_json::from_slice::<serde_json::Value>(&bytes) else {
            return Self::recover(path);
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
            // Invalid, or written by a newer version: keep the file, use defaults.
            _ => Self::recover(path),
        }
    }

    fn recover(path: &Path) -> (Settings, LoadOutcome) {
        let mut name = path.file_name().unwrap_or_default().to_os_string();
        name.push(format!(".corrupt-{}", unix_ms()));
        let backup = path.with_file_name(name);
        let _ = std::fs::rename(path, &backup);
        (
            Settings::default(),
            LoadOutcome::RecoveredFromCorrupt { backup },
        )
    }

    /// Temp file in the same directory, flushed, then renamed over the target. On any
    /// failure the temp file is removed and the old file is untouched.
    pub fn save(&self, path: &Path) -> Result<(), ApiError> {
        let json = serde_json::to_vec_pretty(self).map_err(|_| ApiError::Internal)?;
        let mut name = path.file_name().ok_or(ApiError::Internal)?.to_os_string();
        name.push(format!(".tmp-{}-{}", std::process::id(), unix_ms()));
        let tmp = path.with_file_name(name);
        let result = write_synced(&tmp, &json).and_then(|()| std::fs::rename(&tmp, path));
        if result.is_err() {
            let _ = std::fs::remove_file(&tmp);
        }
        result.map_err(|_| ApiError::Internal)
    }
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
        s.save(&p).unwrap();
        let (back, o) = Settings::load(&p);
        assert_eq!(back, s);
        assert_eq!(o, LoadOutcome::Loaded);
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
    fn settings_newer_version_is_kept_aside() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("settings.json");
        let mut v = serde_json::to_value(Settings::default()).unwrap();
        v["settings_version"] = 99.into();
        std::fs::write(&p, v.to_string()).unwrap();
        let (_, o) = Settings::load(&p);
        assert!(matches!(o, LoadOutcome::RecoveredFromCorrupt { .. }));
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
}
