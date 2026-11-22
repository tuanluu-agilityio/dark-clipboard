//! `dark-clipboard`: the desktop app. No flag starts the Slint app hidden (single instance, see
//! `single_instance.rs`); `--headless` runs capture without a UI until Ctrl+C; `--toggle` and
//! `--quit` ask the running instance to toggle the popup or quit.
//!
//! Logging is content-free (GC-06): see `log.rs`. Nothing here formats a model type or any
//! clipboard data. Threads and channels only (GC-05); no network (GC-08).

#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
// The only unsafe code is in `single_instance::win`, which opts in explicitly.
#![deny(unsafe_code)]
// No console window of its own on Windows; `winsys::attach_parent_console` reconnects to the
// terminal the app was started from (Task 202).
#![cfg_attr(not(test), windows_subsystem = "windows")]
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used, clippy::panic))]

mod log;
mod shortcut;
mod single_instance;
mod theme;
mod tray;
mod ui;
#[cfg(windows)]
mod winsys;

use darkclip_app::events::{UNAVAILABLE_READ, UNAVAILABLE_RETENTION, UNAVAILABLE_STORAGE_WRITE};
use darkclip_app::{App, AppEvent, CaptureConfig};
use darkclip_core::i18n::{t, Lang, Msg};
use darkclip_core::policy::SkipReason;
use darkclip_core::{Clock, Timestamp};
use darkclip_platform::{Capabilities, ShortcutSupport, SourceSupport, Support};
use log::{Log, Val};
use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::mpsc;
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

/// What the process does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Mode {
    /// Start the UI app hidden (or forward to the running instance).
    App,
    /// Capture without a window until Ctrl+C.
    Headless,
    /// Ask the running instance to toggle the popup.
    Toggle,
    /// Ask the running instance to quit.
    Quit,
    Help,
}

/// Parsed command line.
#[derive(Debug, PartialEq, Eq)]
struct Cli {
    mode: Mode,
    data_dir: Option<PathBuf>,
    /// Hidden test flag: stop by itself after this many milliseconds.
    exit_after_ms: Option<u64>,
    /// Hidden test flag: do not register the global shortcut with the OS.
    no_global_shortcut: bool,
}

/// `Err(())` is a malformed command line (exit code 2).
fn parse_args(args: &[String]) -> Result<Cli, ()> {
    let mut cli = Cli {
        mode: Mode::App,
        data_dir: None,
        exit_after_ms: None,
        no_global_shortcut: false,
    };
    let mut chosen: Option<Mode> = None;
    let mut pick = |m: Mode| -> Result<(), ()> {
        if chosen.is_some_and(|c| c != m) {
            return Err(());
        }
        chosen = Some(m);
        Ok(())
    };
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--headless" => pick(Mode::Headless)?,
            "--toggle" => pick(Mode::Toggle)?,
            "--quit" => pick(Mode::Quit)?,
            "--help" => pick(Mode::Help)?,
            "--no-global-shortcut" => cli.no_global_shortcut = true,
            "--data-dir" => cli.data_dir = Some(PathBuf::from(it.next().ok_or(())?)),
            "--exit-after-ms" => {
                cli.exit_after_ms = Some(it.next().ok_or(())?.parse().map_err(|_| ())?);
            }
            _ => return Err(()),
        }
    }
    cli.mode = chosen.unwrap_or(Mode::App);
    Ok(cli)
}

struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> Timestamp {
        let ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX));
        Timestamp(ms)
    }
}

fn support_name(s: Support) -> &'static str {
    match s {
        Support::Native => "native",
        Support::Polling { .. } => "polling",
        Support::Unsupported { .. } => "unsupported",
    }
}

fn source_name(s: SourceSupport) -> &'static str {
    match s {
        SourceSupport::Owner => "owner",
        SourceSupport::Frontmost => "frontmost",
        SourceSupport::None => "none",
    }
}

fn shortcut_name(s: ShortcutSupport) -> &'static str {
    match s {
        ShortcutSupport::Native => "native",
        ShortcutSupport::Portal => "portal",
        ShortcutSupport::UserBound => "user_bound",
    }
}

fn log_capabilities(log: &Log, c: &Capabilities) {
    log.event(
        "capabilities",
        &[
            ("backend", Val::Name(c.backend)),
            ("monitor", Val::Name(support_name(c.monitor))),
            ("read", Val::Name(support_name(c.read_without_focus))),
            ("write", Val::Name(support_name(c.write))),
            ("markers", Val::Name(support_name(c.concealed_markers))),
            ("source", Val::Name(source_name(c.source_app))),
            ("shortcut", Val::Name(shortcut_name(c.global_shortcut))),
            ("paste", Val::Name(support_name(c.auto_paste))),
        ],
    );
}

/// Explicit mapping to a log-safe name (the event text itself may contain spaces).
fn unavailable_name(what: &'static str) -> &'static str {
    match what {
        UNAVAILABLE_RETENTION => "retention",
        UNAVAILABLE_STORAGE_WRITE => "storage_write",
        UNAVAILABLE_READ => "clipboard_read",
        _ => "platform",
    }
}

/// Skips per reason, in first-seen order. Logged once on stop as `<name>=<count>` with the
/// reason's static `snake_case` name (`SkipReason::name`): numbers and names only, no
/// content (GC-06).
#[derive(Default)]
struct SkipTally(Vec<(SkipReason, u64)>);

impl SkipTally {
    fn add(&mut self, reason: SkipReason) {
        match self.0.iter_mut().find(|(r, _)| *r == reason) {
            Some((_, n)) => *n = n.saturating_add(1),
            None => self.0.push((reason, 1)),
        }
    }

    fn total(&self) -> u64 {
        self.0.iter().fold(0u64, |a, (_, n)| a.saturating_add(*n))
    }

    fn fields(&self) -> Vec<(&'static str, Val)> {
        self.0
            .iter()
            .map(|(r, n)| (r.name(), Val::Num(*n)))
            .collect()
    }
}

#[cfg(windows)]
fn default_data_dir() -> Option<PathBuf> {
    std::env::var_os("LOCALAPPDATA").map(|p| PathBuf::from(p).join("DarkClipboard"))
}

/// Default settings file (system-overview §10: config lives in `%APPDATA%`, not with the data).
#[cfg(windows)]
fn default_settings_path() -> Option<PathBuf> {
    std::env::var_os("APPDATA")
        .map(|p| PathBuf::from(p).join("DarkClipboard").join("settings.json"))
}

#[cfg(windows)]
fn platform() -> Result<darkclip_platform::PlatformBundle, darkclip_platform::PlatformError> {
    darkclip_windows::create()
}

#[cfg(not(windows))]
fn default_data_dir() -> Option<PathBuf> {
    None
}

#[cfg(not(windows))]
fn default_settings_path() -> Option<PathBuf> {
    None
}

#[cfg(not(windows))]
fn platform() -> Result<darkclip_platform::PlatformBundle, darkclip_platform::PlatformError> {
    Err(darkclip_platform::PlatformError::Unsupported(
        "no adapter for this operating system yet",
    ))
}

fn main() -> ExitCode {
    #[cfg(windows)]
    winsys::attach_parent_console();
    let args: Vec<String> = std::env::args().skip(1).collect();
    // Until the settings are read, messages follow the OS language.
    let lang = ui::os_lang();
    let Ok(cli) = parse_args(&args) else {
        eprintln!("{}", t(lang, Msg::CliUsage));
        return ExitCode::from(2);
    };
    if cli.mode == Mode::Help {
        println!("{}", t(lang, Msg::CliUsage));
        return ExitCode::SUCCESS;
    }
    if !cfg!(windows) {
        eprintln!("{}", t(lang, Msg::CliNotSupportedYet));
        return ExitCode::from(2);
    }
    match cli.mode {
        Mode::Headless => run_headless(&cli, lang),
        _ => run_app(&cli, lang),
    }
}

#[cfg(not(windows))]
fn run_app(_cli: &Cli, _lang: Lang) -> ExitCode {
    ExitCode::from(2)
}

/// Default start, `--toggle` and `--quit`: take the instance lock or forward a command.
#[cfg(windows)]
fn run_app(cli: &Cli, lang: Lang) -> ExitCode {
    use single_instance::{start_or_forward, Command, Start, WinTransport};

    let Some(data_dir) = cli.data_dir.clone().or_else(default_data_dir) else {
        eprintln!("{}", t(lang, Msg::CliStartFailed));
        return ExitCode::from(1);
    };
    if std::fs::create_dir_all(&data_dir).is_err() {
        eprintln!("{}", t(lang, Msg::CliStartFailed));
        return ExitCode::from(1);
    }
    let log = Arc::new(Log::open(&data_dir));
    // A `--data-dir` run gets its own instance scope, so a test never talks to the user's app.
    let scope = single_instance::scope_for(cli.data_dir.as_deref(), default_data_dir().as_deref());
    let Some(transport) = WinTransport::new(scope.as_deref()) else {
        log.event("start_failed", &[("stage", Val::Name("instance"))]);
        eprintln!("{}", t(lang, Msg::CliStartFailed));
        return ExitCode::from(1);
    };
    let (cmd, start_if_free) = match cli.mode {
        Mode::Toggle => (Command::Toggle, false),
        Mode::Quit => (Command::Quit, false),
        _ => (Command::Toggle, true),
    };
    let guard = match start_or_forward(&transport, cmd, start_if_free) {
        Start::Primary(g) => g,
        Start::Forwarded => {
            log.event("forwarded", &[("cmd", Val::Name(cmd.name()))]);
            return ExitCode::SUCCESS;
        }
        Start::NotRunning => {
            eprintln!("{}", t(lang, Msg::ErrAppNotRunning));
            return ExitCode::from(1);
        }
        Start::Failed => {
            log.event("start_failed", &[("stage", Val::Name("forward"))]);
            eprintln!("{}", t(lang, Msg::CliStartFailed));
            return ExitCode::from(1);
        }
    };
    log.event(
        "start",
        &[("version", Val::Name(env!("CARGO_PKG_VERSION")))],
    );
    // `--data-dir` keeps everything (settings and data) in that folder, for tests.
    let settings_path = if cli.data_dir.is_some() {
        Some(data_dir.join("settings.json"))
    } else {
        default_settings_path()
    };
    let Some(settings_path) = settings_path else {
        log.event("start_failed", &[("stage", Val::Name("settings_path"))]);
        eprintln!("{}", t(lang, Msg::CliStartFailed));
        return ExitCode::from(1);
    };
    if let Some(dir) = settings_path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let ok = ui::run(
        ui::RunArgs {
            start: ui::StartArgs {
                data_dir,
                settings_path,
                os_lang: lang,
            },
            exit_after_ms: cli.exit_after_ms,
            log: Arc::clone(&log),
            global_shortcut: !cli.no_global_shortcut,
        },
        |handler| transport.listen(Arc::clone(&log), handler),
    );
    drop(guard);
    if ok {
        ExitCode::SUCCESS
    } else {
        eprintln!("{}", t(lang, Msg::CliStartFailed));
        ExitCode::from(1)
    }
}

fn run_headless(cli: &Cli, lang: Lang) -> ExitCode {
    let Some(data_dir) = cli.data_dir.clone().or_else(default_data_dir) else {
        eprintln!("{}", t(lang, Msg::CliStartFailed));
        return ExitCode::from(1);
    };
    if std::fs::create_dir_all(&data_dir).is_err() {
        eprintln!("{}", t(lang, Msg::CliStartFailed));
        return ExitCode::from(1);
    }
    let log = Arc::new(Log::open(&data_dir));
    log.event(
        "start",
        &[("version", Val::Name(env!("CARGO_PKG_VERSION")))],
    );

    let bundle = match platform() {
        Ok(b) => b,
        Err(_) => {
            log.event("start_failed", &[("stage", Val::Name("platform"))]);
            eprintln!("{}", t(lang, Msg::CliStartFailed));
            return ExitCode::from(1);
        }
    };

    let (ev_tx, ev_rx) = mpsc::channel::<AppEvent>();
    // `--data-dir` keeps everything (settings and data) in that folder, for tests.
    let settings_path = if cli.data_dir.is_some() {
        Some(data_dir.join("settings.json"))
    } else {
        default_settings_path()
    };
    let Some(settings_path) = settings_path else {
        log.event("start_failed", &[("stage", Val::Name("settings_path"))]);
        eprintln!("{}", t(lang, Msg::CliStartFailed));
        return ExitCode::from(1);
    };
    if let Some(dir) = settings_path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let started = App::start(
        bundle,
        &data_dir,
        &settings_path,
        Arc::new(SystemClock),
        CaptureConfig::default(),
        ev_tx,
    );
    let app = match started {
        Ok((app, _)) => app,
        Err(e) => {
            log.event("start_failed", &[("code", Val::Name(e.code()))]);
            eprintln!("{}", t(lang, Msg::CliStartFailed));
            return ExitCode::from(1);
        }
    };
    log_capabilities(&log, &app.capabilities());

    // Ctrl+C: the `ctrlc` crate (a handler thread that sends on a channel; no unsafe here).
    let (stop_tx, stop_rx) = mpsc::channel::<()>();
    if ctrlc::set_handler(move || {
        let _ = stop_tx.send(());
    })
    .is_err()
    {
        // Without a handler Ctrl+C would kill the process without a clean stop.
        log.event("start_failed", &[("stage", Val::Name("no_handler"))]);
        eprintln!("{}", t(lang, Msg::CliStartFailed));
        let _ = app.shutdown();
        return ExitCode::from(1);
    }

    // Counts events; blocks on the channel (no polling).
    let ev_log = Arc::clone(&log);
    let events = std::thread::Builder::new()
        .name("darkclip-cli-events".into())
        .spawn(move || {
            let (mut added, mut bumped, mut skips) = (0u64, 0u64, SkipTally::default());
            for e in ev_rx {
                match e {
                    AppEvent::ClipAdded(_) => added = added.saturating_add(1),
                    AppEvent::ClipBumped(_) => bumped = bumped.saturating_add(1),
                    AppEvent::Skipped(r) => skips.add(r),
                    AppEvent::Unavailable(what) => ev_log.event(
                        "unavailable",
                        &[("what", Val::Name(unavailable_name(what)))],
                    ),
                    AppEvent::ModeChanged(_) | AppEvent::RetentionOk => {}
                }
            }
            (added, bumped, skips)
        });

    println!("{}", t(app.lang(), Msg::CliRunning));
    log.event("running", &[]);
    let why = match cli.exit_after_ms {
        Some(ms) => match stop_rx.recv_timeout(Duration::from_millis(ms)) {
            Ok(()) => "signal",
            Err(_) => "timer",
        },
        None => {
            let _ = stop_rx.recv();
            "signal"
        }
    };
    log.event("stop_requested", &[("why", Val::Name(why))]);
    let t0 = Instant::now();
    let result = app.shutdown();
    let ms = u64::try_from(t0.elapsed().as_millis()).unwrap_or(u64::MAX);
    match result {
        Ok(()) => {
            // A clean shutdown joins the event pump, so the counting thread ends.
            let (added, bumped, skips) =
                events
                    .ok()
                    .and_then(|h| h.join().ok())
                    .unwrap_or((0, 0, SkipTally::default()));
            // Which skip path each skip took (static reason names and counts only).
            log.event("skips", &skips.fields());
            log.event(
                "stop",
                &[
                    ("ms", Val::Num(ms)),
                    ("added", Val::Num(added)),
                    ("bumped", Val::Num(bumped)),
                    ("skipped", Val::Num(skips.total())),
                ],
            );
            ExitCode::SUCCESS
        }
        Err(e) => {
            log.event(
                "stop_failed",
                &[("ms", Val::Num(ms)), ("code", Val::Name(e.code()))],
            );
            ExitCode::from(1)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn a(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| (*s).to_string()).collect()
    }

    #[test]
    fn parses_flags() {
        let c = parse_args(&a(&[
            "--headless",
            "--data-dir",
            "x",
            "--exit-after-ms",
            "5",
        ]))
        .unwrap();
        assert_eq!(c.mode, Mode::Headless);
        assert_eq!(c.data_dir, Some(PathBuf::from("x")));
        assert_eq!(c.exit_after_ms, Some(5));
        assert!(parse_args(&a(&[])).is_ok());
        assert!(parse_args(&a(&["--data-dir"])).is_err());
        assert!(parse_args(&a(&["--exit-after-ms", "x"])).is_err());
        assert!(parse_args(&a(&["--what"])).is_err());
    }

    #[test]
    fn no_global_shortcut_flag_is_parsed() {
        assert!(!parse_args(&a(&[])).unwrap().no_global_shortcut);
        let c = parse_args(&a(&["--no-global-shortcut", "--data-dir", "d"])).unwrap();
        assert!(c.no_global_shortcut);
        assert_eq!(c.mode, Mode::App);
    }

    #[test]
    fn parses_mode_flags() {
        assert_eq!(parse_args(&a(&[])).unwrap().mode, Mode::App);
        assert_eq!(
            parse_args(&a(&["--headless"])).unwrap().mode,
            Mode::Headless
        );
        assert_eq!(parse_args(&a(&["--toggle"])).unwrap().mode, Mode::Toggle);
        assert_eq!(parse_args(&a(&["--quit"])).unwrap().mode, Mode::Quit);
        assert_eq!(parse_args(&a(&["--help"])).unwrap().mode, Mode::Help);
        // --data-dir and --exit-after-ms work with every mode.
        for m in ["--headless", "--toggle", "--quit"] {
            let c = parse_args(&a(&[m, "--data-dir", "d", "--exit-after-ms", "7"])).unwrap();
            assert_eq!(c.data_dir, Some(PathBuf::from("d")));
            assert_eq!(c.exit_after_ms, Some(7));
        }
        let c = parse_args(&a(&["--data-dir", "d", "--exit-after-ms", "7"])).unwrap();
        assert_eq!((c.mode, c.exit_after_ms), (Mode::App, Some(7)));
    }

    #[test]
    fn two_modes_at_once_are_rejected() {
        assert!(parse_args(&a(&["--headless", "--toggle"])).is_err());
        assert!(parse_args(&a(&["--toggle", "--quit"])).is_err());
        assert!(parse_args(&a(&["--quit", "--help"])).is_err());
        // The same flag twice is harmless.
        assert!(parse_args(&a(&["--toggle", "--toggle"])).is_ok());
    }

    #[test]
    fn skip_tally_fields_use_static_names() {
        let mut tally = SkipTally::default();
        assert!(tally.fields().is_empty());
        for r in [
            SkipReason::Busy,
            SkipReason::DarkshotExcluded,
            SkipReason::Busy,
            SkipReason::ExcludedApp,
        ] {
            tally.add(r);
        }
        let got: Vec<(&str, u64)> = tally
            .fields()
            .into_iter()
            .map(|(k, v)| match v {
                Val::Num(n) => (k, n),
                Val::Name(_) => panic!("counts are numbers"),
            })
            .collect();
        assert_eq!(
            got,
            vec![("busy", 2), ("darkshot_excluded", 1), ("excluded_app", 1)]
        );
    }
}
