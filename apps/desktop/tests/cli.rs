//! Black-box tests of the `dark-clipboard` binary. They never write the clipboard; the
//! headless run only opens a clipboard listener and stops after a short timer.

use std::process::Command;

fn bin() -> Command {
    Command::new(env!("CARGO_BIN_EXE_dark-clipboard"))
}

#[test]
fn cli_without_flag_prints_usage() {
    let out = bin().output().unwrap();
    assert_eq!(out.status.code(), Some(0));
    assert!(!out.stdout.is_empty());
}

#[test]
fn unknown_flag_exits_with_code_2() {
    let out = bin().arg("--nope").output().unwrap();
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
