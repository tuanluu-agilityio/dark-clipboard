//! The settings draft and its rules (Task 207). Pure data and logic, tested on every platform.

use crate::shortcut;
use darkclip_app::settings::{PopupPosition, Settings};
use darkclip_core::i18n::Msg;
use darkclip_core::{ClipType, ExcludedApp, Lang, Platform, RetentionPolicy, SensitiveHandling};
use std::collections::BTreeMap;
use std::ops::RangeInclusive;

// The ranges of the retention schema. 0 = no limit (items and days only).
pub const ITEMS_RANGE: RangeInclusive<u32> = 0..=1_000_000;
pub const AGE_RANGE: RangeInclusive<u32> = 0..=3_650;
pub const STORAGE_RANGE: RangeInclusive<u32> = 10..=102_400;
/// The longest program name accepted (Windows file names are at most 255 characters).
pub const MAX_APP_ID: usize = 255;

/// Every kind of content, in the order the window lists them.
pub const ALL_KINDS: [ClipType; 7] = [
    ClipType::Text,
    ClipType::RichText,
    ClipType::Url,
    ClipType::Color,
    ClipType::Code,
    ClipType::Image,
    ClipType::Files,
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Field {
    Shortcut,
    MaxItems,
    MaxAgeDays,
    MaxStorageMb,
    Kinds,
}

/// One refused input: which field, and the message that explains it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Problem {
    pub field: Field,
    pub msg: Msg,
}

/// What removing an excluded app did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RemoveOutcome {
    Removed,
    /// A built-in entry: nothing changed yet, the caller must ask and then call
    /// `confirm_remove`.
    NeedsConfirmation,
    NotFound,
}

/// The excluded apps. A built-in entry (password managers) goes only after an explicit
/// confirmation: `request_remove` never removes it by itself.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct AppList {
    items: Vec<ExcludedApp>,
    /// The `app_id` of a built-in entry waiting for the confirmation.
    pending: Option<String>,
}

impl AppList {
    pub fn new(items: Vec<ExcludedApp>) -> Self {
        Self {
            items,
            pending: None,
        }
    }

    pub fn items(&self) -> &[ExcludedApp] {
        &self.items
    }

    /// The built-in entry that waits for the confirmation, if any.
    pub fn pending(&self) -> Option<&ExcludedApp> {
        let id = self.pending.as_deref()?;
        self.items.iter().find(|a| a.app_id == id)
    }

    pub fn request_remove(&mut self, index: usize) -> RemoveOutcome {
        self.pending = None;
        let Some(app) = self.items.get(index) else {
            return RemoveOutcome::NotFound;
        };
        if app.builtin {
            self.pending = Some(app.app_id.clone());
            return RemoveOutcome::NeedsConfirmation;
        }
        self.items.remove(index);
        RemoveOutcome::Removed
    }

    /// Removes the entry that `request_remove` reported as needing confirmation. Does nothing
    /// (false) when no confirmation is pending.
    pub fn confirm_remove(&mut self) -> bool {
        let Some(id) = self.pending.take() else {
            return false;
        };
        let before = self.items.len();
        self.items.retain(|a| a.app_id != id);
        self.items.len() != before
    }

    pub fn cancel_remove(&mut self) {
        self.pending = None;
    }

    /// Adds a program by its file name (never a window title). Returns the cleaned name.
    pub fn add(&mut self, input: &str, platform: Platform) -> Result<String, Msg> {
        let id = clean_app_id(input)?;
        if self
            .items
            .iter()
            .any(|a| a.app_id.to_lowercase() == id.to_lowercase())
        {
            return Err(Msg::SettingsErrAppDuplicate);
        }
        self.items.push(ExcludedApp {
            app_id: id.clone(),
            platform,
            name: None,
            builtin: false,
        });
        Ok(id)
    }
}

/// A program name as typed or picked: trimmed, no folder path, no control characters.
pub fn clean_app_id(input: &str) -> Result<String, Msg> {
    let id = input.trim();
    if id.is_empty() {
        return Err(Msg::SettingsErrAppEmpty);
    }
    if id.chars().count() > MAX_APP_ID
        || id.chars().any(|c| {
            c.is_control() || c == '/' || c == '\\' || crate::popup::detail::is_bidi_control(c)
        })
    {
        return Err(Msg::SettingsErrAppInvalid);
    }
    Ok(id.to_string())
}

/// The platform the excluded-app entries are written for.
pub fn current_platform() -> Platform {
    if cfg!(windows) {
        Platform::Windows
    } else if cfg!(target_os = "macos") {
        Platform::Macos
    } else {
        Platform::LinuxX11
    }
}

/// Everything the form edits. Numbers stay text until `apply_to` checks them.
#[derive(Debug, Clone, PartialEq)]
pub struct Draft {
    pub shortcut: String,
    pub language: Lang,
    pub launch_at_login: bool,
    pub popup_position: PopupPosition,
    pub close_on_focus_loss: bool,
    pub enter_pastes: bool,
    pub sensitive: SensitiveHandling,
    /// One flag per entry of `ALL_KINDS`.
    pub kinds: [bool; 7],
    pub apps: AppList,
    pub max_items: String,
    pub max_age_days: String,
    pub max_storage_mb: String,
}

fn parse_in(text: &str, range: &RangeInclusive<u32>, field: Field) -> Result<u32, Problem> {
    let t = text.trim();
    let bad = |msg| Problem { field, msg };
    if t.is_empty() || !t.chars().all(|c| c.is_ascii_digit()) {
        return Err(bad(Msg::SettingsErrNumber));
    }
    // Digits only, so a parse failure here is an overflow: out of range.
    let n: u32 = t.parse().map_err(|_| bad(Msg::SettingsErrRange))?;
    if range.contains(&n) {
        Ok(n)
    } else {
        Err(bad(Msg::SettingsErrRange))
    }
}

impl Draft {
    pub fn from_settings(s: &Settings) -> Draft {
        let mut kinds = [false; 7];
        for (flag, kind) in kinds.iter_mut().zip(ALL_KINDS) {
            *flag = s.privacy.capture_kinds.contains(&kind);
        }
        Draft {
            shortcut: s.shortcuts.toggle_popup.clone(),
            language: s.ui.language,
            launch_at_login: s.startup.launch_at_login,
            popup_position: s.ui.popup_position,
            close_on_focus_loss: s.ui.close_on_focus_loss,
            enter_pastes: s.ui.enter_pastes,
            sensitive: s.privacy.sensitive_handling,
            kinds,
            apps: AppList::new(s.privacy.excluded_apps.clone()),
            max_items: s.retention.max_items.to_string(),
            max_age_days: s.retention.max_age_days.to_string(),
            max_storage_mb: s.retention.max_storage_mb.to_string(),
        }
    }

    /// Checks every field and merges the draft onto `base` (what the app holds now), so what
    /// the form does not show (window rectangles, the capture mode, the sensitive categories)
    /// is kept. All problems are reported together. `respect_os_markers` is forced on: there is
    /// no input for it (GC-07).
    pub fn apply_to(&self, base: &Settings) -> Result<Settings, Vec<Problem>> {
        let mut problems = Vec::new();
        let shortcut = match shortcut::parse(&self.shortcut) {
            Ok(s) => Some(s.display()),
            Err(e) => {
                problems.push(Problem {
                    field: Field::Shortcut,
                    msg: e.msg(),
                });
                None
            }
        };
        let mut num = |text: &str, range: &RangeInclusive<u32>, field| {
            parse_in(text, range, field).unwrap_or_else(|p| {
                problems.push(p);
                0
            })
        };
        let max_items = num(&self.max_items, &ITEMS_RANGE, Field::MaxItems);
        let max_age_days = num(&self.max_age_days, &AGE_RANGE, Field::MaxAgeDays);
        let max_storage_mb = num(&self.max_storage_mb, &STORAGE_RANGE, Field::MaxStorageMb);
        let kinds: Vec<ClipType> = ALL_KINDS
            .iter()
            .zip(self.kinds)
            .filter(|(_, on)| *on)
            .map(|(k, _)| *k)
            .collect();
        if kinds.is_empty() {
            problems.push(Problem {
                field: Field::Kinds,
                msg: Msg::SettingsErrNoKinds,
            });
        }
        let Some(shortcut) = shortcut.filter(|_| problems.is_empty()) else {
            return Err(problems);
        };
        let mut s = base.clone();
        s.shortcuts.toggle_popup = shortcut;
        s.ui.language = self.language;
        s.ui.popup_position = self.popup_position;
        s.ui.close_on_focus_loss = self.close_on_focus_loss;
        s.ui.enter_pastes = self.enter_pastes;
        s.startup.launch_at_login = self.launch_at_login;
        s.privacy.sensitive_handling = self.sensitive;
        s.privacy.capture_kinds = kinds;
        s.privacy.excluded_apps = self.apps.items().to_vec();
        // GC-07: the form has no input for the markers and a save never switches them off.
        s.privacy.respect_os_markers = true;
        s.retention.max_items = max_items;
        s.retention.max_age_days = max_age_days;
        s.retention.max_storage_mb = max_storage_mb;
        Ok(s)
    }
}

/// What a save did.
#[derive(Debug, Clone, PartialEq)]
pub enum SaveResult {
    Saved,
    Invalid(Vec<Problem>),
    Failed,
    /// The settings file is read-only (D-028): nothing was written.
    ReadOnly,
}

/// Validates and saves. `update` writes the settings (true = written); it is never called for
/// a read-only file or invalid input.
pub fn run_save(
    read_only: bool,
    draft: &Draft,
    base: &Settings,
    update: impl FnOnce(Settings) -> bool,
) -> SaveResult {
    if read_only {
        return SaveResult::ReadOnly;
    }
    match draft.apply_to(base) {
        Err(p) => SaveResult::Invalid(p),
        Ok(new) => {
            if update(new) {
                SaveResult::Saved
            } else {
                SaveResult::Failed
            }
        }
    }
}

/// True when a limit goes down, so older clips may be deleted at the next cleanup. For items
/// and days 0 means "no limit": a number after 0 lowers, 0 after a number raises.
pub fn limits_lowered(old: &RetentionPolicy, new: &RetentionPolicy) -> bool {
    let lower = |old: u32, new: u32| new != 0 && (old == 0 || new < old);
    lower(old.max_items, new.max_items)
        || lower(old.max_age_days, new.max_age_days)
        || new.max_storage_mb < old.max_storage_mb
}

/// What a control in the privacy section is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Control {
    CaptureMode,
    Sensitive,
    Kinds,
    ExcludedApps,
}

/// A row of the privacy section. The markers row is `Locked`: it carries text only, no value
/// and no way to change it (GC-07).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PrivacyRow {
    Editable(Control),
    Locked { label: Msg, note: Msg },
}

pub fn privacy_rows() -> [PrivacyRow; 5] {
    [
        PrivacyRow::Editable(Control::CaptureMode),
        PrivacyRow::Locked {
            label: Msg::SettingsMarkersLocked,
            note: Msg::SettingsMarkersNote,
        },
        PrivacyRow::Editable(Control::Sensitive),
        PrivacyRow::Editable(Control::Kinds),
        PrivacyRow::Editable(Control::ExcludedApps),
    ]
}

/// The banner for the read-only state (D-028), or none.
pub fn banner(read_only: bool) -> Option<Msg> {
    read_only.then_some(Msg::SettingsReadOnlyBanner)
}

/// Saving is possible only when the settings file may be written and no save is running.
pub fn can_save(read_only: bool, saving: bool) -> bool {
    !read_only && !saving
}

/// The clear-history dialog: it asks first, and only a confirmed request deletes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ClearFlow {
    #[default]
    Idle,
    Confirm {
        keep_pinned: bool,
    },
}

impl ClearFlow {
    /// Opens the dialog; "keep pinned" starts on (the safer choice).
    pub fn ask(&mut self) {
        *self = ClearFlow::Confirm { keep_pinned: true };
    }

    pub fn set_keep_pinned(&mut self, on: bool) {
        if let ClearFlow::Confirm { keep_pinned } = self {
            *keep_pinned = on;
        }
    }

    pub fn cancel(&mut self) {
        *self = ClearFlow::Idle;
    }

    /// `Some(keep_pinned)` when the dialog was open; closes it. `None` otherwise: nothing is
    /// deleted without the dialog.
    pub fn confirm(&mut self) -> Option<bool> {
        match std::mem::take(self) {
            ClearFlow::Confirm { keep_pinned } => Some(keep_pinned),
            ClearFlow::Idle => None,
        }
    }

    pub fn is_open(&self) -> bool {
        matches!(self, ClearFlow::Confirm { .. })
    }
}

/// The message naming a skip reason (`SkipReason::name`), or none for an unknown name.
pub fn skip_msg(name: &str) -> Option<Msg> {
    Some(match name {
        "disabled" => Msg::SkipDisabled,
        "paused" => Msg::SkipPaused,
        "private" => Msg::SkipPrivate,
        "self_write" => Msg::SkipSelfWrite,
        "os_concealed" => Msg::SkipOsConcealed,
        "darkshot_excluded" => Msg::SkipDarkshotExcluded,
        "transient" => Msg::SkipTransient,
        "excluded_app" => Msg::SkipExcludedApp,
        "kind_disabled" => Msg::SkipKindDisabled,
        "too_large" => Msg::SkipTooLarge,
        "nothing_readable" => Msg::SkipNothingReadable,
        "sensitive" => Msg::SkipSensitive,
        "busy" => Msg::SkipBusy,
        _ => return None,
    })
}

/// The skip counters that are not zero, as (message, count), in a stable order.
pub fn skip_rows(skipped: &BTreeMap<String, u64>) -> Vec<(Msg, u64)> {
    skipped
        .iter()
        .filter(|(_, n)| **n > 0)
        .filter_map(|(name, n)| skip_msg(name).map(|m| (m, *n)))
        .collect()
}
