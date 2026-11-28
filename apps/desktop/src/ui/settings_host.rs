//! Wires the settings window to the view-model (`crate::prefs`) and the worker (Task 207).
//!
//! Everything here runs on the Slint event loop thread; the state lives in a thread-local. The
//! window is created when it is first opened. `App` calls (saving, clearing the history,
//! the capture mode, the storage statistics) are queued on the worker with `Poster`; answers
//! return with `invoke_from_event_loop`. Listing the running programs runs on its own short
//! thread. The UI thread never waits for any of them.
//!
//! Saves go through `App::update_settings` (validated, atomic). The worker checks the
//! read-only state itself before writing (`model::run_save_with_login`) and reports it as such, never as
//! "saved". Lowering a retention limit asks for a confirmation first. The form is merged onto the settings the worker holds at that moment, so
//! values the form does not show (window rectangles, the capture mode) are never overwritten.
//! The OS and Darkshot privacy markers have no control anywhere in this window (GC-07).
//!
//! Nothing is logged here: settings values, program names and paths are never formatted into
//! log lines (GC-06).
//!
//! Settings > Translate (Task 606): the switch, shortcut, direction and model are part of the
//! draft and save with the save bar; turning the switch on without consent opens the consent
//! dialog first. The API key is not in the draft: its Save stores it at once in the OS
//! credential store (`prefs::translate`), the field is cleared and only the mask is shown back;
//! the key is never kept in a property or in `Host`. The Test button translates the fixed
//! sample on its own thread; the answer is shown once, never stored or logged.

use super::settings_keys::{self, EscapeLayer, KeyCtx, SettingsKey};
use super::{status_msg, Poster, SText, SettingsWindow, TrText};
use super::{AppRow, KindItem};
use crate::popup::keys::Mods;
use crate::popup::row::fill;
use crate::prefs::about::{self, CapValue};
use crate::prefs::data::{self, CleanupAction, CleanupFlow, ClearCounts};
use crate::prefs::model::{
    self, can_save, limits_lowered, login_availability, run_save_with_login, ClearFlow, Draft,
    Field, Problem, RemoveOutcome, SaveResult, ALL_KINDS,
};
use crate::prefs::sys;
use crate::prefs::transfer::{self, RestoreFlow};
use crate::prefs::translate::{self as tr, SwitchStep};
use crate::update::{self, Outcome, ReleaseUrl, UreqFetch};
use darkclip_app::api::{AppStatus, Caller, CaptureStatus, StorageStatsView};
use darkclip_app::settings::{PopupPosition, Settings, UpdateSettings};
use darkclip_app::{ApiError, App, ClipApi, TransferReply, TransferWait, UiTransfer};
use darkclip_core::i18n::{t, Lang, Msg};
use darkclip_core::{CaptureMode, ClipType, Clock, SensitiveHandling, Timestamp, FAR_FUTURE};
use darkclip_platform::Capabilities;
use slint::{ComponentHandle, ModelRc, SharedString, VecModel};
use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;

/// What the window needs when it is first opened.
struct Init {
    poster: Poster,
    data_dir: PathBuf,
}

/// What the last status refresh found (the form is filled from it when the window opens).
struct Snapshot {
    settings: Settings,
    read_only: bool,
    capture: CaptureStatus,
    caps: Option<Capabilities>,
    shortcut_warning: Option<Msg>,
    /// Why the translate shortcut is not registered, if it is not (Task 606).
    translate_warning: Option<Msg>,
    lang: Lang,
}

struct Host {
    win: SettingsWindow,
    poster: Poster,
    data_dir: PathBuf,
    lang: Lang,
    apps: model::AppList,
    clear: ClearFlow,
    clear_counts: ClearCounts,
    cleanup: CleanupFlow,
    /// The restore confirmation, with the chosen backup (Task 405).
    restore: RestoreFlow,
    saving: bool,
    app_rows: Rc<VecModel<AppRow>>,
    kind_rows: Rc<VecModel<KindItem>>,
    picker_rows: Rc<VecModel<SharedString>>,
    /// The real program names behind the picker rows (the rows show `sys::display_name`).
    picker_names: Vec<String>,
    /// Bumped by every picker request: a late answer for an older one is dropped.
    picker_gen: u64,
    /// What the start-at-login toggle showed when the form was filled (or last saved): only a
    /// change from this is written (D-056).
    login_opened: bool,
    /// Counts every edit of the form; a save clears `dirty` only if no edit came after the form
    /// was read for it.
    edits: u64,
    /// `edits` when the running save read the form.
    edits_at_save: u64,
    /// The translate consent of the form: the stored one, or 1 after the dialog (Task 606).
    ai_consent: u32,
    /// The mask of the stored API key, read once when the form is filled (never the key).
    key_mask: Option<String>,
    /// The key row's Credential Manager calls, run off the UI thread (Task 611).
    key_calls: tr::KeyCalls,
    /// Bumped by every Test press: a late answer for an older one is dropped.
    test_gen: u64,
}

thread_local! {
    static INIT: RefCell<Option<Init>> = const { RefCell::new(None) };
    static HOST: RefCell<Option<Host>> = const { RefCell::new(None) };
    static SNAP: RefCell<Option<Snapshot>> = const { RefCell::new(None) };
    /// The live start-at-login state differs from the stored choice (set when the window opens).
    static LOGIN_DRIFT: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    /// What the About section shows for the update check (kept while the window is closed).
    static UPDATE_VIEW: RefCell<UpdateView> = const { RefCell::new(UpdateView::Idle) };
}

/// One update check at a time, whether from the button or the weekly path.
static IN_FLIGHT: update::InFlight = update::InFlight::new();

/// The update check as About shows it (Task 412).
#[derive(Debug, Clone, PartialEq, Eq)]
enum UpdateView {
    Idle,
    Checking,
    Done(Outcome),
}

/// The result line and whether the "open the release page" button shows.
fn update_texts(lang: Lang, view: &UpdateView) -> (String, bool) {
    match view {
        UpdateView::Idle => (String::new(), false),
        UpdateView::Checking => (t(lang, Msg::SettingsUpdateChecking).to_string(), false),
        UpdateView::Done(Outcome::UpToDate) => {
            (t(lang, Msg::SettingsUpdateUpToDate).to_string(), false)
        }
        UpdateView::Done(Outcome::Available { shown, .. }) => (
            format!("{} {shown}", t(lang, Msg::SettingsUpdateAvailable)),
            true,
        ),
        UpdateView::Done(Outcome::Failed) => {
            (t(lang, Msg::SettingsUpdateFailed).to_string(), false)
        }
    }
}

fn show_update_view(w: &SettingsWindow, lang: Lang) {
    let view = UPDATE_VIEW.with(|v| v.borrow().clone());
    let (text, found) = update_texts(lang, &view);
    w.set_update_status(text.into());
    w.set_update_found(found);
    w.set_update_busy(view == UpdateView::Checking);
}

/// Runs one check on its own thread (the network never blocks the UI or the worker) and
/// returns the outcome on the event loop. The thread holds no clip data. `gate` is the weekly
/// path: the settings and time it must be due under; `None` is the button.
fn spawn_check(gate: Option<(UpdateSettings, Timestamp)>) {
    let started = std::thread::Builder::new()
        .name("darkclip-update-check".into())
        .spawn(move || {
            let current = about::version();
            let outcome = match gate {
                None => Some(update::check_now(&UreqFetch, current)),
                Some((u, now)) => update::check_if_due(&u, now, &UreqFetch, current),
            };
            let view = outcome.map_or(UpdateView::Idle, UpdateView::Done);
            IN_FLIGHT.end();
            let _ = slint::invoke_from_event_loop(move || set_update_view(view));
        });
    if started.is_err() {
        IN_FLIGHT.end();
        let _ =
            slint::invoke_from_event_loop(|| set_update_view(UpdateView::Done(Outcome::Failed)));
    }
}

fn set_update_view(view: UpdateView) {
    UPDATE_VIEW.with(|v| *v.borrow_mut() = view);
    with_host(|h| show_update_view(&h.win, h.lang));
}

/// "Check for updates" pressed: the only way to a request besides the weekly toggle.
fn check_updates() {
    if !IN_FLIGHT.try_begin() {
        return;
    }
    set_update_view(UpdateView::Checking);
    spawn_check(None);
}

/// The weekly check on the worker: asks nothing unless the toggle is on and a week has passed.
/// The attempt time is saved before the request, so a failing network cannot cause a retry
/// loop; when it cannot be saved there is no request at all.
fn run_due_check(poster: &Poster) {
    poster.post_quiet(|app: &App| {
        let settings = app.settings();
        let now = crate::SystemClock.now();
        if !update::auto_due(&settings.updates, now) {
            return;
        }
        if !IN_FLIGHT.try_begin() {
            return;
        }
        let due = (settings.updates.clone(), now);
        let mut next = settings;
        next.updates.last_check_ms = Some(now.0);
        if app.update_settings(next).is_err() {
            IN_FLIGHT.end();
            return;
        }
        let _ = slint::invoke_from_event_loop(|| set_update_view(UpdateView::Checking));
        spawn_check(Some(due));
    });
}

/// How often the app looks again whether the weekly check is due.
const AUTO_LOOK_EVERY: std::time::Duration = std::time::Duration::from_secs(6 * 60 * 60);

/// Starts the weekly check: once now and then every few hours, each time only when due.
/// With the toggle off (the default) this never makes a request.
pub fn start_auto_check(poster: Poster) {
    let _ = std::thread::Builder::new()
        .name("darkclip-update-timer".into())
        .spawn(move || loop {
            run_due_check(&poster);
            std::thread::sleep(AUTO_LOOK_EVERY);
        });
}

/// The weekly toggle changed: saved at once (About has no Save bar); turning it on checks now
/// when a check is due.
fn auto_update_toggled(on: bool) {
    let Some(poster) = with_host(|h| h.poster.clone()) else {
        return;
    };
    let again = poster.clone();
    poster.post(move |app: &App| {
        let mut s = app.settings();
        s.updates.auto_check = on;
        let saved = app.update_settings(s).is_ok();
        let live = app.settings().updates.auto_check;
        let _ = slint::invoke_from_event_loop(move || {
            with_host(|h| {
                h.win.set_auto_update(live);
                let msg = (!saved).then(|| (t(h.lang, Msg::SettingsSaveFailed).to_string(), true));
                h.set_status(msg);
            });
        });
        if saved && on {
            run_due_check(&again);
        }
    });
}

/// Opens the release page of the last result. Only an address that passed the allow check is
/// ever kept in the result, so nothing else can be opened from here.
fn open_release() {
    let url: Option<ReleaseUrl> = UPDATE_VIEW.with(|v| match &*v.borrow() {
        UpdateView::Done(Outcome::Available { url, .. }) => Some(url.clone()),
        _ => None,
    });
    if let Some(url) = url {
        if !update::open_release(&url) {
            with_host(|h| {
                h.set_status(Some((
                    t(h.lang, Msg::SettingsUpdateOpenFailed).to_string(),
                    true,
                )))
            });
        }
    }
}

/// Runs `f` on the host; does nothing when it is missing or already borrowed (re-entry).
fn with_host<R>(f: impl FnOnce(&mut Host) -> R) -> Option<R> {
    HOST.with(|cell| {
        let mut guard = cell.try_borrow_mut().ok()?;
        guard.as_mut().map(f)
    })
}

/// Stores what the window needs to be created later. Call once, on the main thread.
pub fn install(poster: Poster, data_dir: PathBuf) {
    INIT.with(|i| *i.borrow_mut() = Some(Init { poster, data_dir }));
}

fn kind_msg(k: ClipType) -> Msg {
    match k {
        ClipType::Text => Msg::ClipTypeText,
        ClipType::RichText => Msg::ClipTypeRichText,
        ClipType::Url => Msg::ClipTypeUrl,
        ClipType::Color => Msg::ClipTypeColor,
        ClipType::Code => Msg::ClipTypeCode,
        ClipType::Image => Msg::ClipTypeImage,
        ClipType::Files => Msg::ClipTypeFiles,
    }
}

fn platform_msg(p: darkclip_core::Platform) -> Msg {
    use darkclip_core::Platform;
    match p {
        Platform::Windows => Msg::PlatformWindows,
        Platform::Macos => Msg::PlatformMacos,
        Platform::LinuxX11 | Platform::LinuxWayland => Msg::PlatformLinux,
    }
}

fn mode_index(c: CaptureStatus) -> i32 {
    match c {
        CaptureStatus::Private => 1,
        CaptureStatus::Paused => 2,
        CaptureStatus::Active | CaptureStatus::Unavailable(_) => 0,
    }
}

/// The capability report as "label: value" lines.
fn caps_text(caps: Option<&Capabilities>, lang: Lang) -> String {
    let Some(c) = caps else {
        return String::new();
    };
    about::capability_rows(c)
        .into_iter()
        .map(|(label, v)| {
            let v = match v {
                CapValue::Msg(m) => t(lang, m),
                CapValue::Raw(s) => s,
            };
            format!("{}: {}", t(lang, label), v)
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Sets every label (creation, language change).
/// The Settings > Translate texts (Task 606).
fn set_translate_strings(w: &SettingsWindow, lang: Lang) {
    let s = w.global::<TrText>();
    let m = |msg: Msg| SharedString::from(t(lang, msg));
    s.set_section(m(Msg::SettingsSectionTranslate));
    s.set_section_desc(m(Msg::SettingsDescTranslate));
    s.set_enable(m(Msg::SettingsTrEnable));
    s.set_enable_note(m(Msg::SettingsTrEnableNote));
    s.set_unavailable(m(Msg::SettingsTrUnavailable));
    s.set_how(m(Msg::SettingsTrHow));
    s.set_shortcut(m(Msg::SettingsTrShortcut));
    s.set_shortcut_note(m(Msg::SettingsTrShortcutNote));
    s.set_shortcut_hint(m(Msg::SettingsShortcutHint));
    s.set_direction(m(Msg::SettingsTrDirection));
    s.set_direction_note(m(Msg::SettingsTrDirectionNote));
    let [auto, en, vi] = tr::direction_labels(lang);
    s.set_dir_auto(auto.into());
    s.set_dir_en(en.into());
    s.set_dir_vi(vi.into());
    s.set_model(m(Msg::SettingsTrModel));
    s.set_model_note(m(Msg::SettingsTrModelNote));
    let [opus, haiku] = tr::model_labels(lang);
    s.set_model_opus(opus.into());
    s.set_model_haiku(haiku.into());
    s.set_key_card(m(Msg::SettingsTrKeyCard));
    s.set_key(m(Msg::SettingsTrKey));
    s.set_key_placeholder(m(Msg::SettingsTrKeyPlaceholder));
    s.set_key_save(m(Msg::SettingsTrKeySave));
    s.set_key_replace(m(Msg::SettingsTrKeyReplace));
    s.set_key_remove(m(Msg::SettingsTrKeyRemove));
    s.set_test(m(Msg::SettingsTrTest));
    s.set_test_note(m(Msg::SettingsTrTestNote));
    s.set_test_button(m(Msg::SettingsTrTestButton));
    s.set_never_card(m(Msg::SettingsTrNeverCard));
    s.set_never1(m(Msg::SettingsTrNever1));
    s.set_never1_note(m(Msg::SettingsTrNever1Note));
    s.set_never2(m(Msg::SettingsTrNever2));
    s.set_never2_note(m(Msg::SettingsTrNever2Note));
    s.set_never3(m(Msg::SettingsTrNever3));
    s.set_never3_note(m(Msg::SettingsTrNever3Note));
    s.set_never4(m(Msg::SettingsTrNever4));
    s.set_never4_note(m(Msg::SettingsTrNever4Note));
    s.set_consent_title(m(Msg::SettingsTrConsentTitle));
    s.set_consent_on(m(Msg::SettingsTrConsentOn));
    s.set_cancel(m(Msg::UiCancel));
    w.set_tr_available(crate::translate::host::available());
}

fn set_strings(w: &SettingsWindow, lang: Lang, snap: Option<&Snapshot>) {
    let s = w.global::<SText>();
    let m = |msg: Msg| SharedString::from(t(lang, msg));
    s.set_title(m(Msg::SettingsTitle));
    s.set_general(m(Msg::SettingsSectionGeneral));
    s.set_privacy(m(Msg::SettingsSectionPrivacy));
    s.set_history(m(Msg::SettingsSectionHistory));
    s.set_about(m(Msg::SettingsSectionAbout));
    s.set_save(m(Msg::SettingsSave));
    s.set_revert(m(Msg::SettingsRevert));
    s.set_cancel(m(Msg::UiCancel));
    s.set_loading(m(Msg::UiLoading));
    s.set_notices(m(Msg::SettingsNotices));
    s.set_limits_confirm(m(Msg::SettingsLimitsConfirm));
    s.set_close(m(Msg::UiClose));
    s.set_shortcut_label(m(Msg::SettingsShortcut));
    s.set_shortcut_hint(m(Msg::SettingsShortcutHint));
    s.set_lang_label(m(Msg::SettingsLanguage));
    s.set_lang_en(m(Msg::SettingsLangEn));
    s.set_lang_vi(m(Msg::SettingsLangVi));
    s.set_login(m(Msg::SettingsStartAtLogin));
    s.set_pos_label(m(Msg::SettingsPopupPosition));
    s.set_pos_cursor(m(Msg::SettingsPosCursor));
    s.set_pos_center(m(Msg::SettingsPosCenter));
    s.set_close_focus(m(Msg::SettingsCloseOnFocusLoss));
    s.set_enter_pastes(m(Msg::SettingsEnterPastes));
    s.set_capture_label(m(Msg::SettingsCaptureMode));
    s.set_mode_normal(m(Msg::SettingsModeNormal));
    s.set_mode_private(m(Msg::SettingsModePrivate));
    s.set_mode_paused(m(Msg::SettingsModePaused));
    // The markers row comes from the view-model as locked text: the window has no control for
    // it (GC-07).
    for row in model::privacy_rows() {
        if let model::PrivacyRow::Locked { label, note } = row {
            s.set_markers_label(m(label));
            s.set_markers_note(m(note));
        }
    }
    s.set_sensitive_label(m(Msg::SettingsSensitive));
    s.set_sens_drop(m(Msg::SettingsSensDrop));
    s.set_sens_masked(m(Msg::SettingsSensMasked));
    s.set_sens_store(m(Msg::SettingsSensStore));
    s.set_sens_warn(m(Msg::SettingsSensStoreWarn));
    s.set_kinds_label(m(Msg::SettingsKinds));
    s.set_excluded_label(m(Msg::SettingsExcluded));
    s.set_excluded_note(m(Msg::SettingsExcludedNote));
    s.set_builtin_badge(m(Msg::SettingsBuiltinBadge));
    s.set_remove(m(Msg::SettingsRemove));
    s.set_add(m(Msg::SettingsAddApp));
    s.set_app_placeholder(m(Msg::SettingsAppPlaceholder));
    s.set_pick_running(m(Msg::SettingsPickRunning));
    s.set_pick_none(m(Msg::SettingsPickNone));
    s.set_pick_note(m(Msg::SettingsPickNote));
    s.set_confirm_builtin(m(Msg::SettingsConfirmRemoveBuiltin));
    s.set_max_items_label(m(Msg::SettingsMaxItems));
    s.set_max_age_label(m(Msg::SettingsMaxAge));
    s.set_max_storage_label(m(Msg::SettingsMaxStorage));
    s.set_retention_note(m(Msg::SettingsRetentionNote));
    s.set_stats_label(m(Msg::SettingsStats));
    s.set_skipped_label(m(Msg::SettingsSkipped));
    s.set_clear(m(Msg::SettingsClear));
    s.set_clear_confirm(m(Msg::SettingsClearConfirm));
    s.set_keep_pinned(m(Msg::SettingsKeepPinned));
    s.set_clear_do(m(Msg::SettingsClearDo));
    s.set_cleanup_heading(m(Msg::SettingsCleanupHeading));
    s.set_older_label(m(Msg::SettingsOlderLabel));
    s.set_older_do(m(Msg::SettingsOlderDo));
    s.set_orphans_do(m(Msg::SettingsOrphansDo));
    s.set_compact_do(m(Msg::SettingsCompactDo));
    s.set_version(m(Msg::SettingsVersion));
    s.set_caps_label(m(Msg::SettingsCapabilities));
    s.set_licences_label(m(Msg::SettingsLicences));
    s.set_licence_text(m(Msg::SettingsLicenceText));
    s.set_third_party(about::third_party_text().into());
    s.set_folder_label(m(Msg::SettingsDataFolder));
    s.set_open_folder(m(Msg::SettingsOpenFolder));
    s.set_show_welcome(m(Msg::SettingsShowWelcome));
    s.set_transfer_heading(m(Msg::SettingsTransferHeading));
    s.set_transfer_note(m(Msg::SettingsTransferNote));
    s.set_backup_do(m(Msg::SettingsBackupDo));
    s.set_restore_do(m(Msg::SettingsRestoreDo));
    s.set_import_do(m(Msg::SettingsImportDo));
    s.set_restore_yes(m(Msg::SettingsRestoreYes));
    s.set_updates_heading(m(Msg::SettingsUpdatesHeading));
    s.set_update_check(m(Msg::SettingsUpdateCheck));
    s.set_update_auto(m(Msg::SettingsUpdateAuto));
    s.set_update_note(m(Msg::SettingsUpdateNote));
    s.set_update_open(m(Msg::SettingsUpdateOpen));
    // Redesign (Task 705).
    s.set_nav_capture(m(Msg::SettingsSectionCapture));
    s.set_nav_storage(m(Msg::SettingsSectionStorage));
    s.set_nav_data(m(Msg::SettingsSectionData));
    s.set_desc_general(m(Msg::SettingsDescGeneral));
    s.set_desc_capture(m(Msg::SettingsDescCapture));
    s.set_desc_storage(m(Msg::SettingsDescStorage));
    s.set_desc_data(m(Msg::SettingsDescData));
    s.set_desc_about(m(Msg::SettingsDescAbout));
    s.set_card_popup(m(Msg::SettingsCardPopup));
    s.set_card_app(m(Msg::SettingsCardApp));
    s.set_card_sensitive(m(Msg::SettingsCardSensitive));
    s.set_card_limits(m(Msg::SettingsCardLimits));
    s.set_enter_pastes_note(m(Msg::SettingsEnterPastesNote));
    s.set_card_recording(m(Msg::SettingsCardRecording));
    s.set_card_private(m(Msg::SettingsCardPrivate));
    s.set_card_paused(m(Msg::SettingsCardPaused));
    s.set_card_recording_line(m(Msg::SettingsCardRecordingLine));
    s.set_card_private_line(m(Msg::SettingsCardPrivateLine));
    s.set_card_paused_line(m(Msg::SettingsCardPausedLine));
    s.set_always_protected(m(Msg::SettingsAlwaysProtected));
    s.set_protected_os(m(Msg::SettingsProtectedOs));
    s.set_protected_darkshot(m(Msg::SettingsProtectedDarkshot));
    s.set_unsaved(m(Msg::SettingsUnsaved));
    s.set_key_ctrl_s(m(Msg::SettingsKeyCtrlS));
    s.set_dlg_remove(m(Msg::SettingsDlgRemove));
    s.set_dlg_limits(m(Msg::SettingsDlgLimits));
    s.set_dlg_clear(m(Msg::SettingsDlgClear));
    s.set_dlg_restore(m(Msg::SettingsDlgRestore));
    s.set_clear_row(m(Msg::SettingsClearRow));
    set_translate_strings(w, lang);
    show_update_view(w, lang);
    w.set_version_text(format!("{} {}", t(lang, Msg::SettingsVersion), about::version()).into());
    // Parts that follow the latest status refresh.
    let (read_only, caps, warning, capture) = snap
        .map_or((false, None, None, CaptureStatus::Active), |s| {
            (s.read_only, s.caps, s.shortcut_warning, s.capture)
        });
    let tr_warning = snap.and_then(|s| s.translate_warning);
    w.global::<TrText>()
        .set_shortcut_warning(tr_warning.map_or("", |b| t(lang, b)).into());
    s.set_banner(model::banner(read_only).map_or("", |b| t(lang, b)).into());
    s.set_shortcut_warning(warning.map_or("", |b| t(lang, b)).into());
    // Honest about start at login: whether the adapter can register, and whether this process
    // may (a custom data folder never changes the login entry).
    let can_register =
        caps.is_some_and(|c| matches!(c.autostart, darkclip_platform::Support::Native));
    let avail = login_availability(can_register, crate::custom_profile());
    s.set_login_note(avail.note().map_or("", |m| t(lang, m)).into());
    w.set_login_enabled(avail.editable());
    s.set_login_drift(
        if LOGIN_DRIFT.get() {
            t(lang, Msg::SettingsStartAtLoginDrift)
        } else {
            ""
        }
        .into(),
    );
    s.set_capture_off_text(m(Msg::SettingsCaptureOff));
    s.set_start_capturing(m(Msg::TrayStartCapturing));
    w.set_capture_off(snap.is_some_and(|s| !s.settings.privacy.capture_enabled));
    s.set_mode_current(t(lang, status_msg(capture)).into());
    w.set_caps_text(caps_text(caps.as_ref(), lang).into());
    w.set_can_save(can_save(read_only, false));
    // Read-only settings cannot keep the choice or the last check time: no toggle then.
    w.set_auto_update_enabled(!read_only);
    w.set_mode(mode_index(capture));
}

impl Host {
    /// The form changed: the save bar shows.
    fn clear_dirty_if_unchanged(&self) {
        if dirty_after_save(self.edits_at_save, self.edits) {
            self.win.set_dirty(false);
        }
    }

    fn mark_dirty(&mut self) {
        self.edits = self.edits.wrapping_add(1);
        self.win.set_dirty(true);
    }

    fn refresh_apps(&self) {
        let rows: Vec<AppRow> = self
            .apps
            .items()
            .iter()
            .map(|a| {
                // Direction controls from a hand-edited file are shown as U+FFFD (F-18).
                let (name, id) = model::app_display(a);
                AppRow {
                    name: name.into(),
                    id: id.into(),
                    builtin: a.builtin,
                    platform: t(self.lang, platform_msg(a.platform)).into(),
                }
            })
            .collect();
        self.app_rows.set_vec(rows);
        let pending = self.apps.pending().map(|a| {
            let (name, id) = model::app_display(a);
            let name = if name.is_empty() { id } else { name };
            format!("{name}: {}", t(self.lang, model::remove_builtin_msg(a)))
        });
        self.win
            .set_confirm_remove_text(pending.unwrap_or_default().into());
    }

    fn clear_errors(&self) {
        let w = &self.win;
        for set in [
            SettingsWindow::set_err_shortcut,
            SettingsWindow::set_err_items,
            SettingsWindow::set_err_age,
            SettingsWindow::set_err_storage,
            SettingsWindow::set_err_kinds,
            SettingsWindow::set_err_app,
            SettingsWindow::set_err_tr_shortcut,
        ] {
            set(w, SharedString::new());
        }
    }

    fn set_status(&self, msg: Option<(String, bool)>) {
        let (text, err) = msg.unwrap_or_default();
        self.win.set_status(text.into());
        self.win.set_status_error(err);
    }

    /// Fills the form from `settings` (opening the window, Revert).
    fn fill_form(&mut self, settings: &Settings) {
        let d = Draft::from_settings(settings);
        let w = &self.win;
        w.set_shortcut(d.shortcut.clone().into());
        w.set_lang_index(i32::from(d.language == Lang::Vi));
        w.set_launch_at_login(d.launch_at_login);
        self.login_opened = d.launch_at_login;
        w.set_position_index(i32::from(d.popup_position == PopupPosition::Center));
        w.set_close_on_focus_loss(d.close_on_focus_loss);
        w.set_enter_pastes(d.enter_pastes);
        w.set_auto_update(settings.updates.auto_check);
        w.set_sensitive_index(match d.sensitive {
            SensitiveHandling::Drop => 0,
            SensitiveHandling::StoreMasked => 1,
            SensitiveHandling::Store => 2,
        });
        let kinds: Vec<KindItem> = ALL_KINDS
            .iter()
            .zip(d.kinds)
            .map(|(k, on)| KindItem {
                label: t(self.lang, kind_msg(*k)).into(),
                on,
            })
            .collect();
        self.kind_rows.set_vec(kinds);
        w.set_max_items(d.max_items.clone().into());
        w.set_max_age(d.max_age_days.clone().into());
        w.set_max_storage(d.max_storage_mb.clone().into());
        w.set_new_app(SharedString::new());
        w.set_tr_enabled(d.ai.enabled);
        w.set_tr_shortcut(d.ai.shortcut.clone().into());
        w.set_tr_direction_index(tr::direction_index(d.ai.direction));
        w.set_tr_model_index(tr::model_index(&d.ai.model));
        self.ai_consent = d.ai.consent_version;
        self.fill_key(settings.ai.consent_version);
        self.apps = d.apps;
        self.clear_errors();
        self.refresh_apps();
        // The form shows what is stored: nothing to save.
        self.win.set_dirty(false);
    }

    /// The key row, read once from the credential store on a short worker thread (Task 611);
    /// the last known mask shows until the answer arrives. A user who never accepted the consent
    /// has no key (the row is disabled until then), so the store is not read for them.
    fn fill_key(&mut self, consent: u32) {
        if consent >= tr::CONSENT_VERSION && secret_store().is_some() {
            let gen = self.key_calls.start();
            if !spawn_key_call(gen, |st| KeyAnswer::Read(tr::stored_mask(st))) {
                self.key_calls.finish(gen);
                self.key_mask = None;
            }
        } else {
            self.key_calls.cancel();
            self.key_mask = None;
        }
        self.win.set_tr_key_busy(self.key_calls.busy());
        self.show_key();
        self.win.set_tr_key_input(SharedString::new());
        self.win.set_tr_key_error(SharedString::new());
        self.win.set_tr_test_text(SharedString::new());
        self.win.set_tr_test_busy(false);
    }

    /// The key row's status and buttons from `key_mask`.
    fn show_key(&self) {
        self.win
            .set_tr_key_status(tr::key_status(self.lang, self.key_mask.as_deref()).into());
        self.win.set_tr_key_editing(self.key_mask.is_none());
    }

    /// The form as a draft.
    fn collect(&self) -> Draft {
        use slint::Model;
        let w = &self.win;
        let mut kinds = [false; 7];
        for (i, flag) in kinds.iter_mut().enumerate() {
            *flag = self.kind_rows.row_data(i).is_some_and(|k| k.on);
        }
        Draft {
            shortcut: w.get_shortcut().to_string(),
            language: if w.get_lang_index() == 1 {
                Lang::Vi
            } else {
                Lang::En
            },
            launch_at_login: w.get_launch_at_login(),
            launch_at_login_opened: self.login_opened,
            popup_position: if w.get_position_index() == 1 {
                PopupPosition::Center
            } else {
                PopupPosition::Cursor
            },
            close_on_focus_loss: w.get_close_on_focus_loss(),
            enter_pastes: w.get_enter_pastes(),
            sensitive: match w.get_sensitive_index() {
                1 => SensitiveHandling::StoreMasked,
                2 => SensitiveHandling::Store,
                _ => SensitiveHandling::Drop,
            },
            kinds,
            apps: self.apps.clone(),
            max_items: w.get_max_items().to_string(),
            max_age_days: w.get_max_age().to_string(),
            max_storage_mb: w.get_max_storage().to_string(),
            ai: model::AiDraft {
                enabled: w.get_tr_enabled(),
                consent_version: self.ai_consent,
                shortcut: w.get_tr_shortcut().to_string(),
                direction: tr::direction_at(w.get_tr_direction_index()),
                model: tr::model_at(w.get_tr_model_index()).to_string(),
            },
        }
    }

    /// The clear dialog's question, naming how many clips the current checkbox state deletes.
    fn show_clear_question(&self, keep_pinned: bool) {
        let n = self.clear_counts.for_choice(keep_pinned);
        self.win
            .set_clear_question(fill(t(self.lang, Msg::SettingsClearConfirm), n).into());
    }

    /// Opens the cleanup dialog for `action`, naming `shown`.
    fn open_cleanup(&mut self, action: CleanupAction, shown: u64) {
        self.cleanup.ask(action, shown);
        if let Some((msg, n)) = self.cleanup.prompt() {
            let text = if matches!(action, CleanupAction::Compact) {
                t(self.lang, msg).replace("{n}", &data::format_bytes(n))
            } else {
                fill(t(self.lang, msg), n)
            };
            self.win.set_cleanup_question(text.into());
            self.win.set_cleanup_open(true);
        }
    }

    /// Cancels every open question (Esc, closing).
    fn close_overlays(&mut self) {
        self.apps.cancel_remove();
        self.clear.cancel();
        self.cleanup.cancel();
        self.win.set_cleanup_open(false);
        self.restore.cancel();
        self.win.set_restore_open(false);
        self.win.set_clear_open(false);
        self.win.set_picker_open(false);
        self.picker_rows.set_vec(Vec::new());
        self.picker_names.clear();
        self.win.set_limits_open(false);
        // A consent dialog closed without an answer leaves the feature off.
        if self.win.get_tr_consent_open() {
            self.win.set_tr_consent_open(false);
            self.win.set_tr_enabled(false);
        }
        self.picker_gen = self.picker_gen.wrapping_add(1);
        self.refresh_apps();
    }

    fn show_problems(&self, problems: &[Problem]) {
        let lang = self.lang;
        let w = &self.win;
        for p in problems {
            let text = SharedString::from(t(lang, p.msg));
            match p.field {
                Field::Shortcut => w.set_err_shortcut(text),
                Field::MaxItems => w.set_err_items(text),
                Field::MaxAgeDays => w.set_err_age(text),
                Field::MaxStorageMb => w.set_err_storage(text),
                Field::Kinds => w.set_err_kinds(text),
                Field::TranslateShortcut => w.set_err_tr_shortcut(text),
            }
        }
    }
}

/// After a save the form is clean only if nothing was edited since the form was read for it; an
/// edit made while the save was running keeps the save bar.
fn dirty_after_save(edits_at_save: u64, edits_now: u64) -> bool {
    edits_at_save == edits_now
}

/// `login` is the stored start-at-login choice after the save (the real state).
fn on_save_result(r: SaveResult, login: bool) {
    with_host(|h| {
        h.saving = false;
        let read_only = SNAP.with(|s| s.borrow().as_ref().is_some_and(|s| s.read_only));
        h.win.set_can_save(can_save(read_only, false));
        match r {
            SaveResult::Saved => {
                h.clear_errors();
                h.clear_dirty_if_unchanged();
                // The toggle shows the stored choice; a later save writes it only when the user
                // changes it again (D-056).
                h.win.set_launch_at_login(login);
                h.login_opened = login;
                h.set_status(Some((t(h.lang, Msg::SettingsSaved).to_string(), false)));
            }
            SaveResult::Invalid(p) => {
                h.clear_errors();
                h.show_problems(&p);
                h.set_status(Some((t(h.lang, Msg::SettingsFixErrors).to_string(), true)));
            }
            SaveResult::Failed => {
                h.set_status(Some((t(h.lang, Msg::SettingsSaveFailed).to_string(), true)));
            }
            SaveResult::SavedLoginFailed => {
                h.clear_errors();
                h.clear_dirty_if_unchanged();
                // The toggle goes back to what is really registered; the failure is shown.
                h.win.set_launch_at_login(login);
                h.login_opened = login;
                h.set_status(Some((
                    t(h.lang, Msg::SettingsAutostartFailed).to_string(),
                    true,
                )));
            }
            SaveResult::ReadOnly => {
                h.set_status(Some((
                    t(h.lang, Msg::SettingsSaveReadOnly).to_string(),
                    true,
                )));
            }
        }
    });
}

/// Save pressed: a lowered retention limit asks first (`limits-confirm` continues).
fn save() {
    let lowered = with_host(|h| {
        let snap = SNAP.with(|s| s.borrow().as_ref().map(|s| s.settings.clone()));
        let Some(snap) = snap else {
            return false;
        };
        match h.collect().apply_to(&snap) {
            Ok(new) => limits_lowered(&snap.retention, &new.retention),
            Err(_) => false,
        }
    });
    if lowered == Some(true) {
        with_host(|h| h.win.set_limits_open(true));
        return;
    }
    save_now();
}

fn save_now() {
    let Some(job) = with_host(|h| {
        let read_only = SNAP.with(|s| s.borrow().as_ref().is_some_and(|s| s.read_only));
        if !can_save(read_only, h.saving) {
            return None;
        }
        h.saving = true;
        h.win.set_can_save(false);
        h.set_status(None);
        h.edits_at_save = h.edits;
        Some((h.collect(), h.poster.clone()))
    })
    .flatten() else {
        return;
    };
    let (draft, poster) = job;
    poster.post(move |app: &App| {
        // Merged onto the settings the app holds now, so nothing the form lacks is lost; the
        // read-only state is checked here, where the write happens.
        let base = app.settings();
        let read_only = app.status().settings_read_only;
        // The registry write happens here, on the worker, never on the UI thread.
        let auto = crate::autostart();
        let result = run_save_with_login(read_only, &draft, &base, auto.as_deref(), |new| {
            app.update_settings(new).is_ok()
        });
        let login = app.settings().startup.launch_at_login;
        let _ = slint::invoke_from_event_loop(move || on_save_result(result, login));
    });
}

/// Reads the real start-at-login state on the worker (the registry), then shows it: the toggle
/// follows the system, and a note says when that differs from the saved setting. Skipped when
/// the toggle was already changed by the user.
fn request_login_state() {
    let Some(poster) = with_host(|h| h.poster.clone()) else {
        return;
    };
    poster.post_quiet(|_app: &App| {
        let live = crate::autostart().and_then(|a| a.is_enabled().ok());
        let _ = slint::invoke_from_event_loop(move || on_login_state(live));
    });
}

fn on_login_state(live: Option<bool>) {
    let stored = SNAP.with(|s| {
        s.borrow()
            .as_ref()
            .map(|s| s.settings.startup.launch_at_login)
    });
    let Some(stored) = stored else {
        return;
    };
    with_host(|h| {
        let (checked, drift) =
            model::toggle_after_live_read(h.win.get_launch_at_login(), stored, live);
        h.win.set_launch_at_login(checked);
        LOGIN_DRIFT.set(drift);
        let lang = h.lang;
        h.win.global::<SText>().set_login_drift(
            if drift {
                t(lang, Msg::SettingsStartAtLoginDrift)
            } else {
                ""
            }
            .into(),
        );
    });
}

fn request_stats() {
    let Some(poster) = with_host(|h| h.poster.clone()) else {
        return;
    };
    poster.post_quiet(|app: &App| {
        let stats = app.get_storage_stats(&Caller::Ui).ok();
        let _ = slint::invoke_from_event_loop(move || on_stats(stats));
    });
}

fn on_stats(stats: Option<StorageStatsView>) {
    with_host(|h| {
        let lang = h.lang;
        let Some(s) = stats else {
            h.win.set_stats_text(SharedString::new());
            h.win.set_skip_text(SharedString::new());
            return;
        };
        let mb = (s.db_bytes.saturating_add(s.blob_bytes)) / (1024 * 1024);
        let counts = format!(
            "{}  |  {}  |  {}",
            fill(t(lang, Msg::SettingsStatsClips), s.clips),
            fill(t(lang, Msg::SettingsStatsPinned), s.pinned),
            fill(t(lang, Msg::SettingsStatsFavorites), s.favorites)
        );
        let sizes = format!(
            "{}  |  {}  |  {}",
            t(lang, Msg::SettingsStatsDb).replace("{n}", &data::format_bytes(s.db_bytes)),
            t(lang, Msg::SettingsStatsBlobs).replace("{n}", &data::format_bytes(s.blob_bytes)),
            fill(t(lang, Msg::SettingsStatsMb), mb)
        );
        let kinds = s
            .by_kind
            .iter()
            .map(|k| format!("{} {}", t(lang, kind_msg(k.kind)), k.count))
            .collect::<Vec<_>>()
            .join("  |  ");
        let line = [counts, sizes, kinds]
            .into_iter()
            .filter(|l| !l.is_empty())
            .collect::<Vec<_>>()
            .join("\n");
        h.win.set_stats_text(line.into());
        let rows = model::skip_rows(&s.skipped);
        let skip = if rows.is_empty() {
            t(lang, Msg::SettingsSkipNone).to_string()
        } else {
            rows.into_iter()
                .map(|(m, n)| format!("{}: {n}", t(lang, m)))
                .collect::<Vec<_>>()
                .join("\n")
        };
        h.win.set_skip_text(skip.into());
    });
}

fn create() -> Option<()> {
    let (poster, data_dir) = INIT.with(|i| {
        i.borrow()
            .as_ref()
            .map(|i| (i.poster.clone(), i.data_dir.clone()))
    })?;
    let lang = SNAP.with(|s| s.borrow().as_ref().map(|s| s.lang));
    let lang = lang.unwrap_or(Lang::En);
    let w = SettingsWindow::new().ok()?;
    crate::theme::apply(&w);
    let app_rows = Rc::new(VecModel::<AppRow>::default());
    let kind_rows = Rc::new(VecModel::<KindItem>::default());
    let picker_rows = Rc::new(VecModel::<SharedString>::default());
    w.set_apps(ModelRc::from(app_rows.clone()));
    w.set_kinds(ModelRc::from(kind_rows.clone()));
    w.set_picker(ModelRc::from(picker_rows.clone()));
    w.set_data_path(data_dir.display().to_string().into());
    w.set_clear_keep_pinned(true);
    HOST.with(|cell| {
        *cell.borrow_mut() = Some(Host {
            win: w.clone_strong(),
            poster,
            data_dir,
            lang,
            apps: model::AppList::default(),
            clear: ClearFlow::default(),
            clear_counts: ClearCounts::default(),
            cleanup: CleanupFlow::default(),
            restore: RestoreFlow::default(),
            saving: false,
            app_rows,
            kind_rows,
            picker_rows,
            picker_names: Vec::new(),
            picker_gen: 0,
            login_opened: false,
            edits: 0,
            edits_at_save: 0,
            ai_consent: 0,
            key_mask: None,
            key_calls: tr::KeyCalls::default(),
            test_gen: 0,
        });
    });
    connect(&w);
    Some(())
}

fn add_app(text: &str) {
    with_host(|h| {
        h.win.set_err_app(SharedString::new());
        match h.apps.add(text, model::current_platform()) {
            Ok(_) => {
                h.win.set_new_app(SharedString::new());
                h.mark_dirty();
                h.refresh_apps();
            }
            Err(m) => h.win.set_err_app(t(h.lang, m).into()),
        }
    });
}

fn open_picker() {
    let Some(gen) = with_host(|h| {
        h.picker_gen = h.picker_gen.wrapping_add(1);
        h.picker_rows.set_vec(Vec::new());
        h.win.set_picker_loading(true);
        h.win.set_picker_open(true);
        h.picker_gen
    }) else {
        return;
    };
    // Process names only (SR-11); listing runs off the UI thread.
    let started = std::thread::Builder::new()
        .name("darkclip-processes".into())
        .spawn(move || {
            let names = sys::running_programs();
            let _ = slint::invoke_from_event_loop(move || {
                with_host(|h| {
                    if h.picker_gen == gen {
                        let rows: Vec<SharedString> = names
                            .iter()
                            .map(|n| SharedString::from(sys::display_name(n)))
                            .collect();
                        h.picker_names = names;
                        h.picker_rows.set_vec(rows);
                        h.win.set_picker_loading(false);
                    }
                });
            });
        });
    if started.is_err() {
        with_host(|h| h.win.set_picker_loading(false));
    }
}

/// "Clear history...": counts both choices on the worker, then asks.
fn clear_ask() {
    let Some(poster) = with_host(|h| h.poster.clone()) else {
        return;
    };
    poster.post_quiet(|app: &App| {
        let keeping = app.count_clearable(&Caller::Ui, true);
        let everything = app.count_clearable(&Caller::Ui, false);
        let counts = keeping
            .and_then(|k| {
                everything.map(|e| ClearCounts {
                    keeping: k,
                    everything: e,
                })
            })
            .ok();
        let _ = slint::invoke_from_event_loop(move || {
            with_host(|h| match counts {
                Some(c) => {
                    h.clear_counts = c;
                    h.clear.ask();
                    h.win.set_clear_keep_pinned(true);
                    h.show_clear_question(true);
                    h.win.set_clear_open(true);
                }
                None => h.set_status(Some((
                    t(h.lang, Msg::SettingsCleanupFailed).to_string(),
                    true,
                ))),
            });
        });
    });
}

/// "Delete old clips...": checks the days, counts what would go on the worker, then asks.
fn older_ask(days_text: SharedString) {
    let Some(days) = data::parse_days(days_text.as_str()) else {
        with_host(|h| {
            h.set_status(Some((
                t(h.lang, Msg::SettingsDaysInvalid).to_string(),
                true,
            )))
        });
        return;
    };
    let Some(poster) = with_host(|h| h.poster.clone()) else {
        return;
    };
    poster.post_quiet(move |app: &App| {
        let n = app.count_older_than(&Caller::Ui, days).ok();
        let _ =
            slint::invoke_from_event_loop(move || ask_cleanup(CleanupAction::OlderThan(days), n));
    });
}

/// "Remove orphaned files...": counts them on the worker, then asks.
fn orphans_ask() {
    let Some(poster) = with_host(|h| h.poster.clone()) else {
        return;
    };
    poster.post_quiet(|app: &App| {
        let n = app.scan_orphans(&Caller::Ui).ok().map(|r| r.files);
        let _ = slint::invoke_from_event_loop(move || ask_cleanup(CleanupAction::Orphans, n));
    });
}

/// "Compact database...": names the current database size.
fn compact_ask() {
    let size =
        with_host(|h| std::fs::metadata(h.data_dir.join("history.db")).map_or(0, |m| m.len()));
    ask_cleanup(CleanupAction::Compact, size);
}

fn ask_cleanup(action: CleanupAction, shown: Option<u64>) {
    with_host(|h| match shown {
        Some(n) => h.open_cleanup(action, n),
        None => h.set_status(Some((
            t(h.lang, Msg::SettingsCleanupFailed).to_string(),
            true,
        ))),
    });
}

/// The confirmed cleanup: runs on the worker and reports in the status line.
fn cleanup_confirm() {
    let Some((action, poster, data_dir)) = with_host(|h| {
        let action = h.cleanup.confirm()?;
        h.win.set_cleanup_open(false);
        Some((action, h.poster.clone(), h.data_dir.clone()))
    })
    .flatten() else {
        return;
    };
    // Measured here, on the UI thread: a quick OS call, and the worker has no OS access.
    let free = sys::free_disk_bytes(&data_dir);
    poster.post(move |app: &App| {
        let ui = &Caller::Ui;
        let outcome = match action {
            CleanupAction::OlderThan(days) => app
                .delete_older_than(ui, days)
                .map(|n| (Msg::UiDeletedMany, n, 0)),
            CleanupAction::Orphans => app
                .sweep_orphan_blobs(ui)
                .map(|r| (Msg::SettingsOrphansDone, r.files, r.bytes)),
            CleanupAction::Compact => app
                .compact_database(ui, free)
                .map(|r| (Msg::SettingsCompactDone, r.bytes_before, r.bytes_after)),
        };
        let _ = slint::invoke_from_event_loop(move || {
            with_host(|h| {
                let lang = h.lang;
                let msg = match outcome {
                    Ok((Msg::SettingsOrphansDone, files, bytes)) => (
                        fill(t(lang, Msg::SettingsOrphansDone), files)
                            .replace("{b}", &data::format_bytes(bytes)),
                        false,
                    ),
                    Ok((Msg::SettingsCompactDone, before, after)) => (
                        t(lang, Msg::SettingsCompactDone)
                            .replace("{n}", &data::format_bytes(before))
                            .replace("{b}", &data::format_bytes(after)),
                        false,
                    ),
                    Ok((done, n, _)) => (fill(t(lang, done), n), false),
                    Err(darkclip_app::ApiError::InvalidArgument(
                        "not enough free disk space" | "free disk space unknown",
                    )) => (t(lang, Msg::SettingsCompactNoSpace).to_string(), true),
                    Err(_) => (t(lang, Msg::SettingsCleanupFailed).to_string(), true),
                };
                h.set_status(Some(msg));
            });
            request_stats();
        });
    });
}

// ---- Backup and transfer (Task 405) ----

fn set_transfer_busy(busy: bool) {
    with_host(|h| h.win.set_transfer_busy(busy));
}

fn transfer_status(text: String, error: bool) {
    with_host(|h| h.set_status(Some((text, error))));
}

/// Opens the file picker; `then` runs on the event loop with the chosen file. The buttons are
/// disabled while the picker is open.
fn pick_then(
    save: bool,
    title: Msg,
    ext: &'static str,
    default_name: Option<String>,
    then: impl FnOnce(PathBuf) + Send + 'static,
) {
    let Some(lang) = with_host(|h| h.lang) else {
        return;
    };
    let filter = if ext == transfer::BACKUP_EXT {
        Msg::TransferFilterBackup
    } else {
        Msg::TransferFilterExport
    };
    let req = crate::filedlg::Pick {
        save,
        title: t(lang, title).to_string(),
        filter_name: t(lang, filter).to_string(),
        ext,
        default_name,
    };
    set_transfer_busy(true);
    let started = crate::filedlg::pick(req, move |path| {
        set_transfer_busy(false);
        if let Some(p) = path {
            then(p);
        }
    });
    match started {
        crate::filedlg::Started::Yes => {}
        crate::filedlg::Started::AlreadyOpen => set_transfer_busy(false),
        crate::filedlg::Started::Unavailable => {
            set_transfer_busy(false);
            transfer_status(t(lang, Msg::TransferNoDialog).to_string(), true);
        }
    }
}

/// Runs `job` on the worker with the buttons disabled; `report` makes the status line text.
/// Starts a transfer on the worker (a quick send) and waits for it on its own thread, so the
/// worker keeps serving mode changes and searches meanwhile (Task 405 review).
fn run_transfer(
    start: impl FnOnce(&App) -> Result<TransferWait, ApiError> + Send + 'static,
    report: impl FnOnce(Lang, TransferReply) -> Option<String> + Send + 'static,
) {
    let Some(poster) = with_host(|h| {
        h.win.set_transfer_busy(true);
        h.set_status(Some((t(h.lang, Msg::TransferWorking).to_string(), false)));
        h.poster.clone()
    }) else {
        return;
    };
    let finish = move |r: Result<TransferReply, ApiError>| {
        with_host(|h| {
            h.win.set_transfer_busy(false);
            let msg = match r {
                Ok(v) => match report(h.lang, v) {
                    Some(text) => (text, false),
                    None => (t(h.lang, Msg::TransferFailed).to_string(), true),
                },
                Err(e) => (t(h.lang, transfer::error_msg(&e)).to_string(), true),
            };
            h.set_status(Some(msg));
        });
        request_stats();
    };
    poster.post_quiet(move |app: &App| match start(app) {
        Ok(wait) => crate::filedlg::wait_transfer(wait, finish),
        Err(e) => {
            let _ = slint::invoke_from_event_loop(move || finish(Err(e)));
        }
    });
}

/// "Back up now...": the user picks where, then the worker writes it.
fn backup_ask() {
    use darkclip_core::Clock;
    let name = transfer::backup_file_name(crate::SystemClock.now());
    pick_then(
        true,
        Msg::TransferDialogBackup,
        transfer::BACKUP_EXT,
        Some(name),
        |path| {
            run_transfer(
                move |app| app.ui_start(&Caller::Ui, UiTransfer::Backup(path)),
                |lang, r| match r {
                    TransferReply::Backup(r) => {
                        Some(fill(t(lang, Msg::TransferBackupDone), r.clips))
                    }
                    _ => None,
                },
            );
        },
    );
}

/// "Restore from backup...": the user picks the file, then confirms the replacement.
fn restore_ask() {
    pick_then(
        false,
        Msg::TransferDialogRestore,
        transfer::BACKUP_EXT,
        None,
        |path| {
            with_host(|h| {
                h.restore.ask(path);
                h.win
                    .set_restore_question(t(h.lang, Msg::SettingsRestoreConfirm).into());
                h.win.set_restore_open(true);
            });
        },
    );
}

fn restore_confirm() {
    let Some((path, data_dir)) = with_host(|h| {
        h.win.set_restore_open(false);
        h.restore.confirm().map(|p| (p, h.data_dir.clone()))
    })
    .flatten() else {
        return;
    };
    // Measured here, on the UI thread: a quick OS call, and the App has no OS access.
    let free_bytes = sys::free_disk_bytes(&data_dir);
    run_transfer(
        move |app| {
            app.ui_start(
                &Caller::Ui,
                UiTransfer::Restore {
                    archive: path,
                    free_bytes,
                },
            )
        },
        |lang, r| match r {
            TransferReply::Restore(r) => Some(fill(t(lang, Msg::TransferRestoreDone), r.clips)),
            _ => None,
        },
    );
}

/// "Import clips...": the user picks an export file; the counts are reported.
fn import_ask() {
    pick_then(
        false,
        Msg::TransferDialogImport,
        transfer::EXPORT_EXT,
        None,
        |path| {
            run_transfer(
                move |app| app.ui_start(&Caller::Ui, UiTransfer::Import(path)),
                |lang, r| match r {
                    TransferReply::Import(r) => Some(transfer::import_text(lang, &r)),
                    _ => None,
                },
            );
        },
    );
}

fn clear_history() {
    let Some((keep, poster)) = with_host(|h| {
        h.clear.set_keep_pinned(h.win.get_clear_keep_pinned());
        let keep = h.clear.confirm()?;
        h.win.set_clear_open(false);
        Some((keep, h.poster.clone()))
    })
    .flatten() else {
        return;
    };
    poster.post(move |app: &App| {
        let r = app.clear_history(&Caller::Ui, keep).ok();
        let _ = slint::invoke_from_event_loop(move || {
            with_host(|h| {
                let msg = match r {
                    Some(n) => (fill(t(h.lang, Msg::UiDeletedMany), n), false),
                    None => (t(h.lang, Msg::SettingsClearFailed).to_string(), true),
                };
                h.set_status(Some(msg));
            });
            request_stats();
        });
    });
}

/// The layers that can be open, as the key map sees them. A `Select` list owns its keys, so the
/// host never sees one open.
fn key_ctx(h: &Host, nav_focused: bool) -> KeyCtx {
    KeyCtx {
        nav_focused,
        dialog_open: h.clear.is_open()
            || h.cleanup.is_open()
            || h.restore.is_open()
            || h.apps.pending().is_some()
            || h.win.get_limits_open()
            || h.win.get_tr_consent_open(),
        picker_open: h.win.get_picker_open(),
        list_open: false,
    }
}

/// Esc: closes the open question or the picker (one layer per press), else hides the window.
/// Hiding keeps the form in memory, but showing the window again fills it from the saved
/// settings, so pending changes are discarded then (as before the redesign).
fn escape() {
    let layer = with_host(|h| {
        let layer = settings_keys::escape_layer(key_ctx(h, false));
        if layer != EscapeLayer::Window {
            h.close_overlays();
        }
        layer
    });
    if layer == Some(EscapeLayer::Window) {
        hide();
    }
}

/// Every key the window sees before the focused element. True when the key was handled.
fn on_key(
    text: SharedString,
    ctrl: bool,
    shift: bool,
    alt: bool,
    meta: bool,
    nav_focused: bool,
) -> bool {
    let mods = Mods {
        ctrl,
        shift,
        alt,
        meta,
    };
    let Some(action) =
        with_host(|h| settings_keys::map_text(text.as_str(), mods, key_ctx(h, nav_focused)))
            .flatten()
    else {
        return false;
    };
    match action {
        SettingsKey::Escape => escape(),
        SettingsKey::Section(n) => {
            with_host(|h| h.win.set_section(i32::from(n)));
        }
        SettingsKey::NavUp => {
            with_host(|h| h.win.set_section((h.win.get_section() - 1).max(0)));
        }
        SettingsKey::NavDown => {
            with_host(|h| {
                let last = settings_keys::SECTION_COUNT - 1;
                h.win.set_section((h.win.get_section() + 1).min(last));
            });
        }
        // Ctrl+S is clicking Save: the same `save()`, the same inline errors. The bar only shows
        // after a change and never on About, so there is nothing to save otherwise.
        SettingsKey::Save => {
            let go = with_host(|h| {
                h.win.get_can_save()
                    && h.win.get_dirty()
                    && h.win.get_section() != settings_keys::SECTION_COUNT - 1
            });
            if go == Some(true) {
                save();
            }
        }
    }
    true
}

fn connect(w: &SettingsWindow) {
    w.on_save(save);
    w.on_revert(|| {
        let snap = SNAP.with(|s| s.borrow().as_ref().map(|s| s.settings.clone()));
        if let Some(s) = snap {
            with_host(|h| {
                h.fill_form(&s);
                h.set_status(None);
            });
            // The toggle shows the live state again, as when the window opened (D-054): a
            // Revert never leads a Save into re-enabling an entry disabled in Task Manager.
            request_login_state();
        }
    });
    w.on_edited(|| {
        with_host(|h| {
            h.mark_dirty();
            h.set_status(None);
        });
    });
    w.on_set_mode(|i| {
        let Some(poster) = with_host(|h| h.poster.clone()) else {
            return;
        };
        let mode = match i {
            1 => CaptureMode::Private,
            2 => CaptureMode::Paused { until: FAR_FUTURE },
            _ => CaptureMode::Normal,
        };
        // Applied even if saving fails (the privacy choice wins), like the tray.
        poster.post(move |app: &App| {
            let _ = app.set_capture_mode(&Caller::Ui, mode);
        });
    });
    w.on_add_app(|text| add_app(text.as_str()));
    w.on_remove_app(|i| {
        with_host(|h| {
            if let Ok(i) = usize::try_from(i) {
                // A built-in entry only asks; it goes after `confirm_remove`.
                if h.apps.request_remove(i) == RemoveOutcome::Removed {
                    h.mark_dirty();
                }
            }
            h.refresh_apps();
        });
    });
    w.on_confirm_remove(|| {
        with_host(|h| {
            if h.apps.confirm_remove() {
                h.mark_dirty();
            }
            h.refresh_apps();
        });
    });
    w.on_cancel_remove(|| {
        with_host(|h| {
            h.apps.cancel_remove();
            h.refresh_apps();
        });
    });
    w.on_open_picker(open_picker);
    w.on_close_picker(|| {
        with_host(|h| {
            h.win.set_picker_open(false);
            h.picker_rows.set_vec(Vec::new());
            h.picker_names.clear();
            h.picker_gen = h.picker_gen.wrapping_add(1);
        });
    });
    w.on_pick_app(|i| {
        let name = with_host(|h| {
            usize::try_from(i)
                .ok()
                .and_then(|i| h.picker_names.get(i).cloned())
        })
        .flatten();
        if let Some(name) = name {
            add_app(&name);
        }
        with_host(|h| {
            h.picker_names.clear();
            h.win.set_picker_open(false);
            h.picker_rows.set_vec(Vec::new());
        });
    });
    w.on_limits_confirm(|| {
        with_host(|h| h.win.set_limits_open(false));
        save_now();
    });
    w.on_limits_cancel(|| {
        with_host(|h| h.win.set_limits_open(false));
    });
    w.on_clear_ask(clear_ask);
    w.on_clear_toggled(|keep| {
        with_host(|h| {
            h.clear.set_keep_pinned(keep);
            h.show_clear_question(keep);
        });
    });
    w.on_older_ask(older_ask);
    w.on_orphans_ask(orphans_ask);
    w.on_compact_ask(compact_ask);
    w.on_cleanup_confirm(cleanup_confirm);
    w.on_cleanup_cancel(|| {
        with_host(|h| {
            h.cleanup.cancel();
            h.win.set_cleanup_open(false);
        });
    });
    w.on_backup_ask(backup_ask);
    w.on_restore_ask(restore_ask);
    w.on_import_ask(import_ask);
    w.on_restore_confirm(restore_confirm);
    w.on_restore_cancel(|| {
        with_host(|h| {
            h.restore.cancel();
            h.win.set_restore_open(false);
        });
    });
    w.on_clear_confirm(clear_history);
    w.on_clear_cancel(|| {
        with_host(|h| {
            h.clear.cancel();
            h.win.set_clear_open(false);
        });
    });
    w.on_show_welcome(super::welcome_host::show);
    w.on_check_updates(check_updates);
    w.on_auto_update_toggled(auto_update_toggled);
    w.on_open_release(open_release);
    w.on_start_capturing(super::welcome_host::show);
    w.on_open_folder(|| {
        with_host(|h| {
            let ok = sys::open_folder(&h.data_dir);
            let msg = (!ok).then(|| (t(h.lang, Msg::SettingsOpenFolderFailed).to_string(), true));
            h.set_status(msg);
        });
    });
    w.on_escape(escape);
    w.on_tr_toggled(tr_toggled);
    w.on_tr_consent_confirm(|| {
        with_host(|h| {
            h.ai_consent = h.ai_consent.max(tr::CONSENT_VERSION);
            h.win.set_tr_consent_open(false);
            h.win.set_tr_enabled(true);
            h.mark_dirty();
            h.set_status(None);
        });
    });
    w.on_tr_consent_cancel(|| {
        with_host(|h| {
            h.win.set_tr_consent_open(false);
            h.win.set_tr_enabled(false);
        });
    });
    w.on_tr_save_key(tr_save_key);
    w.on_tr_replace_key(|| {
        with_host(|h| {
            h.win.set_tr_key_error(SharedString::new());
            h.win.set_tr_key_editing(true);
        });
    });
    w.on_tr_remove_key(tr_remove_key);
    w.on_tr_test(tr_test);
    w.on_key(on_key);
    w.on_select_key(|text, alt, open| settings_keys::select_code(text.as_str(), alt, open));
    w.window().on_close_requested(|| {
        with_host(|h| h.close_overlays());
        slint::CloseRequestResponse::HideWindow
    });
}

/// The latest status refresh: read-only state, capture status, capabilities, the shortcut
/// warning and the language. Runs on the main thread with every refresh; it never touches the
/// values the user is typing.
pub fn set_snapshot(
    settings: &Settings,
    status: AppStatus,
    caps: Capabilities,
    shortcut_warning: Option<Msg>,
    translate_warning: Option<Msg>,
) {
    let lang = settings.ui.language;
    SNAP.with(|s| {
        *s.borrow_mut() = Some(Snapshot {
            settings: settings.clone(),
            read_only: status.settings_read_only,
            capture: status.capture,
            caps: Some(caps),
            shortcut_warning,
            translate_warning,
            lang,
        });
    });
    with_host(|h| {
        let relabel = h.lang != lang;
        h.lang = lang;
        SNAP.with(|s| {
            let s = s.borrow();
            set_strings(&h.win, lang, s.as_ref());
        });
        h.win
            .set_can_save(can_save(status.settings_read_only, h.saving));
        if relabel {
            // Kind names and the confirmation text follow the language; the choices stay.
            let kinds: Vec<KindItem> = {
                use slint::Model;
                ALL_KINDS
                    .iter()
                    .enumerate()
                    .map(|(i, k)| KindItem {
                        label: t(lang, kind_msg(*k)).into(),
                        on: h.kind_rows.row_data(i).is_some_and(|x| x.on),
                    })
                    .collect()
            };
            h.kind_rows.set_vec(kinds);
            h.refresh_apps();
            h.show_key();
        }
    });
}

// ---- Settings > Translate (Task 606) -------------------------------------------------------

/// The OS credential store of the API key (Windows); `None` where translate is unavailable.
fn secret_store() -> Option<Box<dyn darkclip_platform::SecretStore>> {
    #[cfg(windows)]
    {
        Some(Box::new(darkclip_windows::WinSecretStore))
    }
    #[cfg(not(windows))]
    {
        None
    }
}

/// The Enable switch was clicked (it already shows `on`).
fn tr_toggled(on: bool) {
    with_host(|h| match tr::switch_step(on, h.ai_consent) {
        SwitchStep::AskConsent => {
            // The switch stays off until "Turn on".
            h.win.set_tr_enabled(false);
            let stored = SNAP.with(|s| s.borrow().as_ref().map(|s| s.settings.ai.shortcut.clone()));
            let shortcut =
                tr::consent_shortcut(h.win.get_tr_shortcut().as_str(), stored.as_deref());
            let body = tr::consent_body(h.lang, &shortcut);
            h.win.global::<TrText>().set_consent_body(body.into());
            h.win.set_tr_consent_open(true);
        }
        SwitchStep::On | SwitchStep::Off => {
            h.mark_dirty();
            h.set_status(None);
        }
    });
}

/// What a Credential Manager call of the key row answered (Task 611). Masks and codes only.
enum KeyAnswer {
    /// The read when the form fills: the stored key's mask.
    Read(Option<String>),
    Saved(Result<String, tr::KeyError>),
    Removed(Result<(), tr::KeyError>),
}

/// A typed key on its way to the worker thread: wiped when dropped unused (a failed spawn).
struct TypedKey(String);

impl Drop for TypedKey {
    fn drop(&mut self) {
        darkclip_core::sensitive::wipe_string(&mut self.0);
    }
}

/// Runs `call` with the credential store on a short worker thread, never the UI one (the
/// `SecretStore` contract); the answer comes back on the event loop for generation `gen`.
/// `false`: the thread did not start (`call` is dropped unrun).
fn spawn_key_call(
    gen: u64,
    call: impl FnOnce(&dyn darkclip_platform::SecretStore) -> KeyAnswer + Send + 'static,
) -> bool {
    std::thread::Builder::new()
        .name("darkclip-key-store".into())
        .spawn(move || {
            let answer = match secret_store() {
                Some(store) => call(store.as_ref()),
                None => KeyAnswer::Read(None),
            };
            let _ = slint::invoke_from_event_loop(move || on_key_answer(gen, answer));
        })
        .is_ok()
}

/// Starts a Save or Remove call unless one runs (the buttons are disabled then, so this only
/// guards Enter in the field). Clears the field when it starts.
fn start_key_press() -> Option<u64> {
    with_host(|h| {
        if h.key_calls.busy() {
            return None;
        }
        h.win.set_tr_key_input(SharedString::new());
        let gen = h.key_calls.start();
        h.win.set_tr_key_busy(true);
        Some(gen)
    })
    .flatten()
}

/// The key's Save: stored at once (not through the draft) on a worker thread, the field
/// cleared. The typed key is wiped on the worker (`tr::save_key`), or here if it never starts.
fn tr_save_key(typed: SharedString) {
    if secret_store().is_none() {
        return;
    }
    let input = TypedKey(typed.to_string());
    drop(typed);
    let Some(gen) = start_key_press() else {
        return;
    };
    let started = spawn_key_call(gen, move |store| {
        let mut input = input;
        KeyAnswer::Saved(tr::save_key(store, std::mem::take(&mut input.0)))
    });
    if !started {
        on_key_answer(
            gen,
            KeyAnswer::Saved(Err(tr::KeyError::Store(tr::busy_code()))),
        );
    }
}

/// The key's Remove, on a worker thread.
fn tr_remove_key() {
    if secret_store().is_none() {
        return;
    }
    let Some(gen) = start_key_press() else {
        return;
    };
    if !spawn_key_call(gen, |store| KeyAnswer::Removed(tr::remove_key(store))) {
        on_key_answer(
            gen,
            KeyAnswer::Removed(Err(tr::KeyError::Remove(tr::busy_code()))),
        );
    }
}

/// A key-row answer on the event loop. Only the latest call's answer is shown (an older one is
/// dropped); a key set or removed is reported to the flow whatever the generation.
fn on_key_answer(gen: u64, answer: KeyAnswer) {
    let changed = matches!(answer, KeyAnswer::Saved(Ok(_)) | KeyAnswer::Removed(Ok(())));
    with_host(|h| {
        if !h.key_calls.finish(gen) {
            return;
        }
        h.win.set_tr_key_busy(h.key_calls.busy());
        match answer {
            KeyAnswer::Read(mask) => {
                h.key_mask = mask;
                h.show_key();
            }
            KeyAnswer::Saved(Ok(mask)) => {
                h.key_mask = Some(mask);
                h.win.set_tr_key_error(SharedString::new());
                h.show_key();
                h.set_status(Some((
                    t(h.lang, Msg::SettingsTrKeySaved).to_string(),
                    false,
                )));
            }
            KeyAnswer::Removed(Ok(())) => {
                h.key_mask = None;
                h.win.set_tr_key_error(SharedString::new());
                h.show_key();
                h.set_status(Some((
                    t(h.lang, Msg::SettingsTrKeyRemoved).to_string(),
                    false,
                )));
            }
            KeyAnswer::Saved(Err(e)) | KeyAnswer::Removed(Err(e)) => {
                h.win.set_tr_key_error(tr::key_error_text(h.lang, e).into());
            }
        }
    });
    if changed {
        key_changed();
    }
}

/// The key was set or removed: the flow's cached presence is dropped and a status refresh
/// registers or unregisters the shortcut.
fn key_changed() {
    crate::translate::host::key_changed();
    if let Some(p) = with_host(|h| h.poster.clone()) {
        p.post(|_app: &App| {});
    }
}

/// What the Test button found.
enum TestAnswer {
    Ok { text: String, ms: u64 },
    NoKey,
    Failed(darkclip_ai::AiError),
}

/// The Test button: the fixed sample, the form's model, on its own thread (never the UI one).
fn tr_test() {
    let Some((gen, model)) = with_host(|h| {
        if h.key_mask.is_none() {
            h.win
                .set_tr_test_text(t(h.lang, Msg::SettingsTrTestNoKey).into());
            return None;
        }
        h.test_gen = h.test_gen.wrapping_add(1);
        h.win.set_tr_test_busy(true);
        h.win
            .set_tr_test_text(t(h.lang, Msg::SettingsTrTestRunning).into());
        Some((h.test_gen, tr::model_at(h.win.get_tr_model_index())))
    })
    .flatten() else {
        return;
    };
    let started = std::thread::Builder::new()
        .name("darkclip-translate-test".into())
        .spawn(move || {
            let answer = run_test(model);
            let _ = slint::invoke_from_event_loop(move || on_test_answer(gen, answer));
        });
    if started.is_err() {
        on_test_answer(gen, TestAnswer::Failed(darkclip_ai::AiError::Network));
    }
}

/// Translates [`tr::TEST_SAMPLE`] with the stored key (read here, wiped after use).
fn run_test(model: &'static str) -> TestAnswer {
    use darkclip_ai::{AnthropicProvider, Provider, TransformKind, TransformRequest};
    use darkclip_core::sensitive::wipe_string;
    let Some(store) = secret_store() else {
        return TestAnswer::NoKey;
    };
    let Some(key) = store
        .get(darkclip_platform::ANTHROPIC_KEY_TARGET)
        .ok()
        .flatten()
        .filter(|k| !k.0.trim().is_empty())
    else {
        return TestAnswer::NoKey;
    };
    let t0 = std::time::Instant::now();
    let provider = AnthropicProvider::new(crate::ai_net::UreqTransport::new(), key, model);
    let req = TransformRequest {
        kind: TransformKind::Translate {
            direction: darkclip_ai::Direction::Auto,
        },
        text: darkclip_core::Redacted(tr::TEST_SAMPLE.to_string()),
    };
    let answer = provider.transform(&req);
    drop(provider);
    let ms = u64::try_from(t0.elapsed().as_millis()).unwrap_or(u64::MAX);
    match answer {
        Ok(mut r) => {
            let text = std::mem::take(&mut r.text.0);
            wipe_string(&mut r.text.0);
            TestAnswer::Ok { text, ms }
        }
        Err(e) => TestAnswer::Failed(e),
    }
}

/// Shows the Test answer once (an older press's answer is dropped). Nothing is stored or logged.
fn on_test_answer(gen: u64, answer: TestAnswer) {
    with_host(|h| {
        if h.test_gen != gen {
            return;
        }
        h.win.set_tr_test_busy(false);
        let text = match &answer {
            TestAnswer::Ok { text, ms } => tr::test_ok_text(h.lang, text, *ms),
            TestAnswer::NoKey => t(h.lang, Msg::SettingsTrTestNoKey).to_string(),
            TestAnswer::Failed(e) => {
                let (m, _) = super::translate_host::ai_error_parts(*e);
                t(h.lang, m).to_string()
            }
        };
        h.win.set_tr_test_text(text.into());
    });
    if let TestAnswer::Ok { mut text, .. } = answer {
        darkclip_core::sensitive::wipe_string(&mut text);
    }
}

/// The data may have changed: a visible window refreshes its statistics.
pub fn app_changed() {
    if with_host(|h| h.win.window().is_visible()) == Some(true) {
        request_stats();
    }
}

/// Opens the settings window (or brings it to the front when it is already open).
pub fn show() {
    if HOST.with(|h| h.borrow().is_none()) && create().is_none() {
        return;
    }
    let Some(w) = with_host(|h| h.win.clone_strong()) else {
        return;
    };
    if w.window().is_visible() {
        w.window().set_minimized(false);
        focus_os_window(&w);
        return;
    }
    let snap = SNAP.with(|s| s.borrow().as_ref().map(|s| s.settings.clone()));
    let snap = Some(snap.unwrap_or_default());
    with_host(|h| {
        h.lang = SNAP.with(|s| s.borrow().as_ref().map_or(h.lang, |s| s.lang));
        SNAP.with(|s| set_strings(&h.win, h.lang, s.borrow().as_ref()));
        h.saving = false;
        h.close_overlays();
        h.win.set_section(0);
        h.set_status(None);
        if let Some(s) = &snap {
            h.fill_form(s);
        }
    });
    let _ = w.show();
    request_login_state();
    use slint::winit_030::{winit, WinitWindowAccessor};
    let _ = w
        .window()
        .with_winit_window(|ww| ww.set_theme(Some(winit::window::Theme::Dark)));
    focus_os_window(&w);
    request_stats();
}

/// Brings the window to the front with the keyboard focus.
fn focus_os_window(w: &SettingsWindow) {
    use slint::winit_030::WinitWindowAccessor;
    let _ = w.window().with_winit_window(|ww| ww.focus_window());
}

/// Closes the window; the process list and the open questions are dropped.
pub fn hide() {
    let Some(w) = with_host(|h| {
        h.close_overlays();
        // A key typed but not saved never stays in the window.
        h.win.set_tr_key_input(SharedString::new());
        h.win.clone_strong()
    }) else {
        return;
    };
    let _ = w.hide();
}

#[cfg(test)]
mod tests {
    use super::dirty_after_save;

    #[test]
    fn a_save_clears_dirty_only_when_nothing_changed_meanwhile() {
        assert!(dirty_after_save(3, 3));
        assert!(!dirty_after_save(3, 4));
        // The counter wraps without a false "clean".
        assert!(!dirty_after_save(u64::MAX, 0));
    }
}
