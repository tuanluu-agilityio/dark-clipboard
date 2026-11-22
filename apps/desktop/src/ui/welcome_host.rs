//! The first-run welcome window (Task 208), wired to `crate::prefs::welcome` and the worker.
//!
//! Runs on the Slint event loop thread; the state lives in thread-locals. The window is created
//! when it is first shown: on its own once per process while the welcome is unanswered, and
//! again from Settings > About. Answers (`App::update_settings`, the registry write for start at
//! login) run on the worker via `Poster`; the result returns with `invoke_from_event_loop`. The
//! UI thread never waits for them. Closing the window with Esc or the close button counts as
//! "Not now" while the welcome is unanswered, and changes nothing afterwards.
//!
//! Nothing is logged here: the shortcut, the data path and settings values are not formatted
//! into log lines (GC-06).

use super::{Poster, WText, WelcomeWindow};
use crate::prefs::model::login_availability;
use crate::prefs::welcome::{run_welcome, WelcomeResult};
use darkclip_app::settings::Settings;
use darkclip_app::App;
use darkclip_core::i18n::{t, Lang, Msg};
use darkclip_platform::Capabilities;
use slint::{ComponentHandle, SharedString};
use std::cell::{Cell, RefCell};
use std::path::PathBuf;

struct Init {
    poster: Poster,
    data_dir: PathBuf,
}

/// What the last status refresh found.
struct Snapshot {
    settings: Settings,
    can_register: bool,
}

thread_local! {
    static INIT: RefCell<Option<Init>> = const { RefCell::new(None) };
    static WIN: RefCell<Option<WelcomeWindow>> = const { RefCell::new(None) };
    static SNAP: RefCell<Option<Snapshot>> = const { RefCell::new(None) };
    /// The welcome opened itself already in this process (it never nags again).
    static AUTO_SHOWN: Cell<bool> = const { Cell::new(false) };
}

/// Stores what the window needs to be created later. Call once, on the main thread.
pub fn install(poster: Poster, data_dir: PathBuf) {
    INIT.with(|i| *i.borrow_mut() = Some(Init { poster, data_dir }));
}

fn window() -> Option<WelcomeWindow> {
    WIN.with(|w| w.borrow().as_ref().map(WelcomeWindow::clone_strong))
}

fn language() -> Lang {
    SNAP.with(|s| {
        s.borrow()
            .as_ref()
            .map_or(Lang::En, |s| s.settings.ui.language)
    })
}

/// The latest status refresh. Opens the welcome once per process while it is unanswered (a
/// read-only settings file never needs it: `Settings::needs_welcome` is false there). Updates
/// the texts of an open window (language change).
pub fn set_snapshot(settings: &Settings, caps: &Capabilities) {
    let can_register = matches!(caps.autostart, darkclip_platform::Support::Native);
    let needs = settings.needs_welcome();
    SNAP.with(|s| {
        *s.borrow_mut() = Some(Snapshot {
            settings: settings.clone(),
            can_register,
        });
    });
    if needs && !AUTO_SHOWN.replace(true) {
        show();
        return;
    }
    if let Some(w) = window() {
        if w.window().is_visible() {
            set_strings(&w);
        }
    }
}

/// The texts, in the language and with the shortcut of the latest snapshot.
fn set_strings(w: &WelcomeWindow) {
    let Some(data_dir) = INIT.with(|i| i.borrow().as_ref().map(|i| i.data_dir.clone())) else {
        return;
    };
    let lang = language();
    let shortcut = SNAP.with(|s| {
        s.borrow()
            .as_ref()
            .map(|s| s.settings.shortcuts.toggle_popup.clone())
            .unwrap_or_default()
    });
    let m = |msg: Msg| SharedString::from(t(lang, msg));
    let g = w.global::<WText>();
    g.set_title(m(Msg::WelcomeTitle));
    g.set_shortcut(
        t(lang, Msg::WelcomeShortcut)
            .replace("{shortcut}", &shortcut)
            .into(),
    );
    g.set_never(m(Msg::WelcomeNever));
    g.set_data(
        t(lang, Msg::WelcomeData)
            .replace("{path}", &data_dir.display().to_string())
            .into(),
    );
    g.set_no_cloud(m(Msg::WelcomeNoCloud));
    g.set_capture_off(m(Msg::WelcomeCaptureOff));
    g.set_login(m(Msg::WelcomeAutostart));
    g.set_login_note(m(Msg::SettingsStartAtLoginCustomProfile));
    g.set_start(m(Msg::WelcomeStart));
    g.set_not_now(m(Msg::WelcomeNotNow));
    g.set_close(m(Msg::UiClose));
}

fn create() -> Option<WelcomeWindow> {
    INIT.with(|i| i.borrow().is_some()).then_some(())?;
    let w = WelcomeWindow::new().ok()?;
    crate::theme::apply(&w);
    w.on_begin(|| answer(true));
    w.on_not_now(|| answer(false));
    w.on_dismiss(close);
    w.window().on_close_requested(|| {
        // The window's own close button: the same as Esc, and like Esc not while an answer is
        // being saved.
        if window().is_some_and(|w| w.get_busy()) {
            return slint::CloseRequestResponse::KeepWindowShown;
        }
        close_answer();
        slint::CloseRequestResponse::HideWindow
    });
    Some(w)
}

/// Shows the welcome (or brings it to the front).
pub fn show() {
    let w = match window() {
        Some(w) => w,
        None => {
            let Some(w) = create() else {
                return;
            };
            WIN.with(|c| *c.borrow_mut() = Some(w.clone_strong()));
            w
        }
    };
    if !w.window().is_visible() {
        let (login, can_login, capture_off) = SNAP.with(|s| {
            s.borrow().as_ref().map_or((false, false, true), |s| {
                (
                    s.settings.startup.launch_at_login,
                    s.can_register,
                    !s.settings.privacy.capture_enabled,
                )
            })
        });
        set_strings(&w);
        w.set_login(login);
        w.set_can_login(can_login);
        w.set_login_enabled(login_availability(can_login, crate::custom_profile()).editable());
        w.set_show_capture_off(capture_off);
        w.set_error(SharedString::new());
        w.set_busy(false);
        w.set_finished(false);
    }
    let _ = w.show();
    use slint::winit_030::{winit, WinitWindowAccessor};
    let _ = w
        .window()
        .with_winit_window(|ww| ww.set_theme(Some(winit::window::Theme::Dark)));
    let _ = w.window().with_winit_window(|ww| ww.focus_window());
}

fn hide() {
    if let Some(w) = window() {
        let _ = w.hide();
    }
}

/// Esc or the Close button.
fn close() {
    if window().is_some_and(|w| w.get_busy()) {
        return;
    }
    close_answer();
    hide();
}

/// Closing without choosing counts as "Not now" while the welcome is unanswered.
fn close_answer() {
    let unanswered = SNAP.with(|s| {
        s.borrow()
            .as_ref()
            .is_some_and(|s| s.settings.needs_welcome())
    });
    let finished = window().is_some_and(|w| w.get_finished());
    if unanswered && !finished {
        send(false, false, false);
    }
}

fn answer(start: bool) {
    let Some(w) = window() else {
        return;
    };
    if w.get_busy() {
        return;
    }
    w.set_busy(true);
    w.set_error(SharedString::new());
    send(start, w.get_login(), true);
}

/// Posts the answer to the worker; with `waiting` the window is told the result.
fn send(start: bool, login: bool, waiting: bool) {
    let Some(poster) = INIT.with(|i| i.borrow().as_ref().map(|i| i.poster.clone())) else {
        return;
    };
    poster.post(move |app: &App| {
        let base = app.settings();
        let read_only = app.status().settings_read_only;
        // The registry write happens here, on the worker, never on the UI thread.
        let auto = crate::autostart();
        let result = run_welcome(read_only, start, login, &base, auto.as_deref(), |s| {
            app.update_settings(s).is_ok()
        });
        if waiting {
            let _ = slint::invoke_from_event_loop(move || on_result(result));
        }
    });
}

fn on_result(r: WelcomeResult) {
    let Some(w) = window() else {
        return;
    };
    let lang = language();
    w.set_busy(false);
    match r {
        WelcomeResult::Done => hide(),
        WelcomeResult::DoneLoginFailed => {
            w.set_login(false);
            w.set_error(t(lang, Msg::WelcomeAutostartFailed).into());
            w.set_finished(true);
        }
        WelcomeResult::Failed => w.set_error(t(lang, Msg::WelcomeSaveFailed).into()),
        WelcomeResult::ReadOnly => w.set_error(t(lang, Msg::SettingsSaveReadOnly).into()),
    }
}
