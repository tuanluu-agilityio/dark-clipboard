//! `dark-clipboard`: Phase 2 developer binary. `--headless` runs capture without a UI until
//! Ctrl+C. The windowed app arrives in Phase 3.
//!
//! Logging is content-free (GC-06): see `log.rs`. Nothing here formats a model type or any
//! clipboard data. Threads and channels only (GC-05); no network (GC-08).

#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used, clippy::panic))]

mod log;

use darkclip_app::events::{UNAVAILABLE_READ, UNAVAILABLE_RETENTION, UNAVAILABLE_STORAGE_WRITE};
use darkclip_app::{App, AppEvent, CaptureConfig};
use darkclip_core::i18n::{t, Lang, Msg};
use darkclip_core::{Clock, Timestamp};
use darkclip_platform::{Capabilities, ShortcutSupport, SourceSupport, Support};
use log::{Log, Val};
use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::mpsc;
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

/// Parsed command line.
#[derive(Debug, PartialEq, Eq)]
struct Cli {
    headless: bool,
    data_dir: Option<PathBuf>,
    /// Hidden test flag: stop by itself after this many milliseconds.
    exit_after_ms: Option<u64>,
}

/// `Err(())` is a malformed command line (exit code 2).
fn parse_args(args: &[String]) -> Result<Cli, ()> {
    let mut cli = Cli {
        headless: false,
        data_dir: None,
        exit_after_ms: None,
    };
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--headless" => cli.headless = true,
            "--data-dir" => cli.data_dir = Some(PathBuf::from(it.next().ok_or(())?)),
            "--exit-after-ms" => {
                cli.exit_after_ms = Some(it.next().ok_or(())?.parse().map_err(|_| ())?);
            }
            _ => return Err(()),
        }
    }
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

#[cfg(windows)]
fn default_data_dir() -> Option<PathBuf> {
    std::env::var_os("LOCALAPPDATA").map(|p| PathBuf::from(p).join("DarkClipboard"))
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
fn platform() -> Result<darkclip_platform::PlatformBundle, darkclip_platform::PlatformError> {
    Err(darkclip_platform::PlatformError::Unsupported(
        "no adapter for this operating system yet",
    ))
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let lang = Lang::En;
    let Ok(cli) = parse_args(&args) else {
        eprintln!("{}", t(lang, Msg::CliUsage));
        return ExitCode::from(2);
    };
    if !cli.headless {
        println!("{}", t(lang, Msg::CliUsage));
        return ExitCode::SUCCESS;
    }
    if !cfg!(windows) {
        eprintln!("{}", t(lang, Msg::CliNotSupportedYet));
        return ExitCode::from(2);
    }
    run_headless(&cli, lang)
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
            eprintln!("{}", t(lang, Msg::CliNotSupportedYet));
            return ExitCode::from(2);
        }
    };

    let (ev_tx, ev_rx) = mpsc::channel::<AppEvent>();
    let settings_path = data_dir.join("settings.json");
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
        log.event("start_failed", &[("stage", Val::Name("ctrl_handler"))]);
    }

    // Counts events; blocks on the channel (no polling).
    let ev_log = Arc::clone(&log);
    let events = std::thread::Builder::new()
        .name("darkclip-cli-events".into())
        .spawn(move || {
            let (mut added, mut bumped, mut skipped) = (0u64, 0u64, 0u64);
            for e in ev_rx {
                match e {
                    AppEvent::ClipAdded(_) => added = added.saturating_add(1),
                    AppEvent::ClipBumped(_) => bumped = bumped.saturating_add(1),
                    AppEvent::Skipped(_) => skipped = skipped.saturating_add(1),
                    AppEvent::Unavailable(what) => ev_log.event(
                        "unavailable",
                        &[("what", Val::Name(unavailable_name(what)))],
                    ),
                    AppEvent::ModeChanged(_) | AppEvent::RetentionOk => {}
                }
            }
            (added, bumped, skipped)
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
            let (added, bumped, skipped) =
                events.ok().and_then(|h| h.join().ok()).unwrap_or((0, 0, 0));
            log.event(
                "stop",
                &[
                    ("ms", Val::Num(ms)),
                    ("added", Val::Num(added)),
                    ("bumped", Val::Num(bumped)),
                    ("skipped", Val::Num(skipped)),
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
        assert!(c.headless);
        assert_eq!(c.data_dir, Some(PathBuf::from("x")));
        assert_eq!(c.exit_after_ms, Some(5));
        assert!(parse_args(&a(&[])).is_ok());
        assert!(parse_args(&a(&["--data-dir"])).is_err());
        assert!(parse_args(&a(&["--exit-after-ms", "x"])).is_err());
        assert!(parse_args(&a(&["--what"])).is_err());
    }
}
