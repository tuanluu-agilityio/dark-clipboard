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

use super::{status_msg, Poster, SText, SettingsWindow};
use super::{AppRow, KindItem};
use crate::popup::row::fill;
use crate::prefs::about::{self, CapValue};
use crate::prefs::model::{
    self, can_save, limits_lowered, run_save_with_login, ClearFlow, Draft, Field, Problem,
    RemoveOutcome, SaveResult, ALL_KINDS,
};
use crate::prefs::sys;
use darkclip_app::api::{AppStatus, Caller, CaptureStatus, StorageStatsView};
use darkclip_app::settings::{PopupPosition, Settings};
use darkclip_app::{App, ClipApi};
use darkclip_core::i18n::{t, Lang, Msg};
use darkclip_core::{CaptureMode, ClipType, SensitiveHandling, FAR_FUTURE};
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
    lang: Lang,
}

struct Host {
    win: SettingsWindow,
    poster: Poster,
    data_dir: PathBuf,
    lang: Lang,
    apps: model::AppList,
    clear: ClearFlow,
    saving: bool,
    app_rows: Rc<VecModel<AppRow>>,
    kind_rows: Rc<VecModel<KindItem>>,
    picker_rows: Rc<VecModel<SharedString>>,
    /// The real program names behind the picker rows (the rows show `sys::display_name`).
    picker_names: Vec<String>,
    /// Bumped by every picker request: a late answer for an older one is dropped.
    picker_gen: u64,
}

thread_local! {
    static INIT: RefCell<Option<Init>> = const { RefCell::new(None) };
    static HOST: RefCell<Option<Host>> = const { RefCell::new(None) };
    static SNAP: RefCell<Option<Snapshot>> = const { RefCell::new(None) };
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
    s.set_version(m(Msg::SettingsVersion));
    s.set_caps_label(m(Msg::SettingsCapabilities));
    s.set_licences_label(m(Msg::SettingsLicences));
    s.set_licence_text(m(Msg::SettingsLicenceText));
    s.set_third_party(about::third_party_text().into());
    s.set_folder_label(m(Msg::SettingsDataFolder));
    s.set_open_folder(m(Msg::SettingsOpenFolder));
    s.set_show_welcome(m(Msg::SettingsShowWelcome));
    w.set_version_text(format!("{} {}", t(lang, Msg::SettingsVersion), about::version()).into());
    // Parts that follow the latest status refresh.
    let (read_only, caps, warning, capture) = snap
        .map_or((false, None, None, CaptureStatus::Active), |s| {
            (s.read_only, s.caps, s.shortcut_warning, s.capture)
        });
    s.set_banner(model::banner(read_only).map_or("", |b| t(lang, b)).into());
    s.set_shortcut_warning(warning.map_or("", |b| t(lang, b)).into());
    // Honest about start at login: the adapter says whether it can register (Task 208).
    let can_register =
        caps.is_some_and(|c| matches!(c.autostart, darkclip_platform::Support::Native));
    s.set_login_note(
        if can_register {
            ""
        } else {
            t(lang, Msg::SettingsStartAtLoginNote)
        }
        .into(),
    );
    s.set_mode_current(t(lang, status_msg(capture)).into());
    w.set_caps_text(caps_text(caps.as_ref(), lang).into());
    w.set_can_save(can_save(read_only, false));
    w.set_mode(mode_index(capture));
}

impl Host {
    fn refresh_apps(&self) {
        let rows: Vec<AppRow> = self
            .apps
            .items()
            .iter()
            .map(|a| AppRow {
                name: a.name.clone().unwrap_or_default().into(),
                id: a.app_id.clone().into(),
                builtin: a.builtin,
                platform: t(self.lang, platform_msg(a.platform)).into(),
            })
            .collect();
        self.app_rows.set_vec(rows);
        let pending = self.apps.pending().map(|a| {
            let name = a.name.as_deref().unwrap_or(&a.app_id);
            format!(
                "{name}: {}",
                t(self.lang, Msg::SettingsConfirmRemoveBuiltin)
            )
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
        w.set_position_index(i32::from(d.popup_position == PopupPosition::Center));
        w.set_close_on_focus_loss(d.close_on_focus_loss);
        w.set_enter_pastes(d.enter_pastes);
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
        self.apps = d.apps;
        self.clear_errors();
        self.refresh_apps();
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
        }
    }

    /// Cancels every open question (Esc, closing).
    fn close_overlays(&mut self) {
        self.apps.cancel_remove();
        self.clear.cancel();
        self.win.set_clear_open(false);
        self.win.set_picker_open(false);
        self.picker_rows.set_vec(Vec::new());
        self.picker_names.clear();
        self.win.set_limits_open(false);
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
            }
        }
    }
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
                // The toggle goes back to what is really registered; the failure is shown.
                h.win.set_launch_at_login(login);
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
        let line = format!(
            "{}  |  {}  |  {}",
            fill(t(lang, Msg::SettingsStatsClips), s.clips),
            fill(t(lang, Msg::SettingsStatsPinned), s.pinned),
            fill(t(lang, Msg::SettingsStatsMb), mb)
        );
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
            saving: false,
            app_rows,
            kind_rows,
            picker_rows,
            picker_names: Vec::new(),
            picker_gen: 0,
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

fn connect(w: &SettingsWindow) {
    w.on_save(save);
    w.on_revert(|| {
        let snap = SNAP.with(|s| s.borrow().as_ref().map(|s| s.settings.clone()));
        if let Some(s) = snap {
            with_host(|h| {
                h.fill_form(&s);
                h.set_status(None);
            });
        }
    });
    w.on_edited(|| {
        with_host(|h| h.set_status(None));
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
                let _: RemoveOutcome = h.apps.request_remove(i);
            }
            h.refresh_apps();
        });
    });
    w.on_confirm_remove(|| {
        with_host(|h| {
            h.apps.confirm_remove();
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
    w.on_clear_ask(|| {
        with_host(|h| {
            h.clear.ask();
            h.win.set_clear_keep_pinned(true);
            h.win.set_clear_open(true);
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
    w.on_open_folder(|| {
        with_host(|h| {
            let ok = sys::open_folder(&h.data_dir);
            let msg = (!ok).then(|| (t(h.lang, Msg::SettingsOpenFolderFailed).to_string(), true));
            h.set_status(msg);
        });
    });
    w.on_escape(|| {
        let closed = with_host(|h| {
            let open = h.clear.is_open()
                || h.apps.pending().is_some()
                || h.win.get_limits_open()
                || h.win.get_picker_open();
            if open {
                h.close_overlays();
            }
            open
        });
        if closed == Some(false) {
            hide();
        }
    });
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
) {
    let lang = settings.ui.language;
    SNAP.with(|s| {
        *s.borrow_mut() = Some(Snapshot {
            settings: settings.clone(),
            read_only: status.settings_read_only,
            capture: status.capture,
            caps: Some(caps),
            shortcut_warning,
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
        }
    });
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
        h.win.clone_strong()
    }) else {
        return;
    };
    let _ = w.hide();
}
