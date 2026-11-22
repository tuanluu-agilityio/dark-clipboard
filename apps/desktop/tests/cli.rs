//! Black-box tests of the `dark-clipboard` binary. They never write the clipboard; the
//! headless run only opens a clipboard listener and stops after a short timer.

use std::process::Command;

fn bin() -> Command {
    Command::new(env!("CARGO_BIN_EXE_dark-clipboard"))
}

#[test]
fn help_prints_usage() {
    let out = bin().arg("--help").output().unwrap();
    assert_eq!(out.status.code(), Some(0));
    assert!(!out.stdout.is_empty());
}

#[test]
fn unknown_flag_exits_with_code_2() {
    let out = bin().arg("--nope").output().unwrap();
    assert_eq!(out.status.code(), Some(2));
}

#[test]
fn two_modes_exit_with_code_2() {
    let out = bin().args(["--toggle", "--headless"]).output().unwrap();
    assert_eq!(out.status.code(), Some(2));
}

/// Event names the binary may write (log.rs). A line is `<unix_ms> <event> [key=value ...]`
/// with only digits, ASCII letters and `_ = .` in it: nothing that could be clipboard text.
#[cfg(windows)]
const KNOWN_EVENTS: &[&str] = &[
    "start",
    "capabilities",
    "running",
    "unavailable",
    "stop_requested",
    "stop",
    "skips",
    "stop_failed",
    "start_failed",
    "forwarded",
    "ipc_command",
    "ipc_dropped",
    "ipc_stopped",
    "tray_failed",
    "tray_quit",
];

#[cfg(windows)]
#[test]
fn headless_with_data_dir_starts_and_stops() {
    let dir = tempfile::tempdir().unwrap();
    let out = bin()
        .arg("--data-dir")
        .arg(dir.path())
        .args(["--headless", "--exit-after-ms", "500"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(0));
    let log = dir.path().join("logs").join("dark-clipboard.log");
    let text = std::fs::read_to_string(&log).unwrap();
    let lines: Vec<&str> = text.lines().collect();
    assert!(lines.len() >= 3, "{text}");
    for line in &lines {
        let mut parts = line.split(' ');
        assert!(parts.next().unwrap().chars().all(|c| c.is_ascii_digit()));
        let event = parts.next().unwrap();
        assert!(KNOWN_EVENTS.contains(&event), "unknown event {event}");
        for kv in parts {
            assert!(
                kv.chars()
                    .all(|c| c.is_ascii_alphanumeric() || "_=.-".contains(c)),
                "unexpected token {kv}"
            );
        }
    }
    assert!(lines.iter().any(|l| l.contains(" start")));
    assert!(lines.iter().any(|l| l.contains(" stop")));
    // One content-free line of per-reason skip counts on stop (Task 110).
    let skips: Vec<&&str> = lines.iter().filter(|l| l.contains(" skips")).collect();
    assert_eq!(skips.len(), 1, "{text}");
    for kv in skips[0].split(' ').skip(2) {
        let (k, v) = kv.split_once('=').unwrap();
        assert!(
            k.chars().all(|c| c.is_ascii_lowercase() || c == '_'),
            "{kv}"
        );
        assert!(v.chars().all(|c| c.is_ascii_digit()), "{kv}");
    }
}

/// Waits until the log of `dir` has a `running` line (the first instance listens before that).
#[cfg(windows)]
fn wait_running(dir: &std::path::Path) {
    let log = dir.join("logs").join("dark-clipboard.log");
    for _ in 0..100 {
        if std::fs::read_to_string(&log).is_ok_and(|t| t.contains(" running")) {
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    unreachable!("first instance did not reach running");
}

/// Task 201: the second instance forwards its command and exits 0; the first one gets the
/// command and ends. `--data-dir` gives the pair its own instance scope (never the user's
/// real app). No clipboard writes, no input.
#[cfg(windows)]
#[test]
fn second_instance_forwards_and_exits_zero() {
    let dir = tempfile::tempdir().unwrap();
    let mut first = bin()
        .arg("--data-dir")
        .arg(dir.path())
        .args(["--exit-after-ms", "20000"])
        .spawn()
        .unwrap();
    wait_running(dir.path());
    // --quit is forwarded like --toggle (same path) but ends the first instance for the test.
    let second = bin()
        .arg("--data-dir")
        .arg(dir.path())
        .arg("--quit")
        .output()
        .unwrap();
    assert_eq!(second.status.code(), Some(0));
    // The first instance stops long before its 20 s timer.
    let started = std::time::Instant::now();
    let status = loop {
        if let Some(s) = first.try_wait().unwrap() {
            break s;
        }
        if started.elapsed() > std::time::Duration::from_secs(10) {
            let _ = first.kill();
            unreachable!("first instance did not quit");
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    };
    assert_eq!(status.code(), Some(0));
    let log = std::fs::read_to_string(dir.path().join("logs").join("dark-clipboard.log")).unwrap();
    assert!(log.contains(" ipc_command cmd=quit"), "{log}");
    assert!(log.contains(" forwarded cmd=quit"), "{log}");
}

#[cfg(windows)]
#[test]
fn toggle_without_a_running_instance_exits_one() {
    let dir = tempfile::tempdir().unwrap();
    let out = bin()
        .arg("--data-dir")
        .arg(dir.path())
        .arg("--toggle")
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
}

#[cfg(windows)]
#[test]
fn app_mode_starts_hidden_and_stops_on_timer() {
    let dir = tempfile::tempdir().unwrap();
    let out = bin()
        .arg("--data-dir")
        .arg(dir.path())
        .args(["--exit-after-ms", "1500"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(0));
    let log = std::fs::read_to_string(dir.path().join("logs").join("dark-clipboard.log")).unwrap();
    assert!(log.contains(" running"), "{log}");
    assert!(
        log.contains(
            " stop
"
        ) || log.ends_with(" stop"),
        "{log}"
    );
}
