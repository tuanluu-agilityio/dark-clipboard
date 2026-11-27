//! The settings draft and its rules (Task 207). Pure data and logic, tested on every platform.

use crate::shortcut;
use darkclip_app::settings::{PopupPosition, Settings};
use darkclip_core::i18n::Msg;
use darkclip_core::{ClipType, ExcludedApp, Lang, Platform, RetentionPolicy, SensitiveHandling};
use darkclip_platform::Autostart;
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

/// An excluded app's name and program name for display: direction controls are replaced by
/// U+FFFD (D-051), so an entry from a hand-edited settings file cannot read differently than it
/// is. An entry without a name has an empty name.
pub fn app_display(app: &ExcludedApp) -> (String, String) {
    let name = app.name.as_deref().map(super::sys::display_name);
    (
        name.unwrap_or_default(),
        super::sys::display_name(&app.app_id),
    )
}

/// The confirmation asked before a built-in exclusion goes. Darkshot marks its own copies, so
/// removing its entry does not make them stored; the password managers' entries are what keeps
/// their copies out.
pub fn remove_builtin_msg(app: &ExcludedApp) -> Msg {
    if app.platform == Platform::Windows && app.app_id.eq_ignore_ascii_case(DARKSHOT_APP_ID) {
        Msg::SettingsConfirmRemoveDarkshot
    } else {
        Msg::SettingsConfirmRemoveBuiltin
    }
}

/// The program name of the built-in Darkshot entry (Windows, D-053).
const DARKSHOT_APP_ID: &str = "darkshot.exe";

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
    /// What the start-at-login toggle showed when the form was filled. Only a toggle the user
    /// changed in this form is written; otherwise the worker's current value stays (the welcome
    /// may have changed it while the form was open, D-056).
    pub launch_at_login_opened: bool,
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
            launch_at_login_opened: s.startup.launch_at_login,
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
        // Only a toggle the user changed here is written; otherwise the worker's value stays
        // (the welcome may have answered while the form was open, D-056).
        if self.launch_at_login != self.launch_at_login_opened {
            s.startup.launch_at_login = self.launch_at_login;
        }
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
    /// Saved, but start at login could not be changed (Task 208); the stored choice is the
    /// real state, so the toggle goes back to it.
    SavedLoginFailed,
}

/// Why start at login can or cannot be changed in this process.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LoginAvail {
    Available,
    /// The platform adapter cannot register; the choice is only stored.
    NoAdapter,
    /// Started with `--data-dir`: the login entry would start the default profile, not this
    /// one, so it cannot be changed from here.
    CustomProfile,
}

pub fn login_availability(adapter_native: bool, custom_profile: bool) -> LoginAvail {
    if custom_profile {
        LoginAvail::CustomProfile
    } else if adapter_native {
        LoginAvail::Available
    } else {
        LoginAvail::NoAdapter
    }
}

impl LoginAvail {
    /// The note shown next to the toggle.
    pub fn note(self) -> Option<Msg> {
        match self {
            LoginAvail::Available => None,
            LoginAvail::NoAdapter => Some(Msg::SettingsStartAtLoginNote),
            LoginAvail::CustomProfile => Some(Msg::SettingsStartAtLoginCustomProfile),
        }
    }

    /// Whether the toggle can be used.
    pub fn editable(self) -> bool {
        !matches!(self, LoginAvail::CustomProfile)
    }
}

/// What the toggle shows when the settings window opens: the live registry state when it could
/// be read, else the stored choice. The flag says stored and live differ.
pub fn login_view(stored: bool, live: Option<bool>) -> (bool, bool) {
    match live {
        Some(l) => (l, l != stored),
        None => (stored, false),
    }
}

/// The toggle after the live state was read (opening the window, and again after Revert):
/// `shown` is what the toggle shows now. Returns the toggle and whether stored and live differ.
/// A toggle the user already changed (it no longer shows the stored choice) is kept.
pub fn toggle_after_live_read(shown: bool, stored: bool, live: Option<bool>) -> (bool, bool) {
    let (checked, drift) = login_view(stored, live);
    if shown == stored {
        (checked, drift)
    } else {
        (shown, drift)
    }
}

/// Brings the start-at-login registration in line with `want`. `Ok(previous)` is the state it
/// had before when it was changed (`None`: nothing to do); `Err(actual)` when it failed, with
/// the state to store instead (the registry's if it can be read, else `fallback`). Without an
/// adapter nothing happens.
pub fn sync_login(
    auto: Option<&dyn Autostart>,
    want: bool,
    fallback: bool,
) -> Result<Option<bool>, bool> {
    let Some(auto) = auto else {
        return Ok(None);
    };
    let current = auto.is_enabled();
    if current == Ok(want) {
        return Ok(None);
    }
    let before = current.unwrap_or(fallback);
    match auto.set_enabled(want) {
        Ok(()) => Ok(Some(before)),
        Err(_) => Err(before),
    }
}

/// Validates and saves, with the start-at-login registration (Task 208). `update` writes the
/// settings (true = written); it is never called for a read-only file or invalid input. The
/// registration changes before the file is written; if the write then fails the previous state
/// is put back. A failed registration still saves everything else, with the choice stored as
/// the real state, and reports `SavedLoginFailed`.
pub fn run_save_with_login(
    read_only: bool,
    draft: &Draft,
    base: &Settings,
    auto: Option<&dyn Autostart>,
    update: impl FnOnce(Settings) -> bool,
) -> SaveResult {
    if read_only {
        return SaveResult::ReadOnly;
    }
    let mut new = match draft.apply_to(base) {
        Ok(s) => s,
        Err(p) => return SaveResult::Invalid(p),
    };
    let want = new.startup.launch_at_login;
    let login = sync_login(auto, want, base.startup.launch_at_login);
    if let Err(actual) = login {
        new.startup.launch_at_login = actual;
    }
    if !update(new) {
        if let (Ok(Some(before)), Some(auto)) = (login, auto) {
            let _ = auto.set_enabled(before);
        }
        return SaveResult::Failed;
    }
    match login {
        Err(_) => SaveResult::SavedLoginFailed,
        Ok(_) => SaveResult::Saved,
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
        "translate_source" => Msg::SkipTranslateSource,
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
