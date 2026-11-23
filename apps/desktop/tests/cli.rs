//! Black-box tests of the `dark-clipboard` binary. They never write the clipboard; the
//! headless run only opens a clipboard listener and stops after a short timer.

use std::process::Command;

/// Every run of the binary from a test skips the WER registration (debug builds honour
/// `DARKCLIP_TEST_SKIP_WER`), so the gate never writes the real per-user WER list (Task 413).
fn bin() -> Command {
    let mut c = Command::new(env!("CARGO_BIN_EXE_dark-clipboard"));
    c.env("DARKCLIP_TEST_SKIP_WER", "1");
    c
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
    "capture_off",
    // Task 413: WER set-up (skipped in tests) and the data-folder DACL.
    "wer",
    "wer_failed",
    "wer_skipped",
    "data_dir_acl",
    "data_dir_acl_failed",
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
    // Task 413: the test never registers with WER. The temp folder existed (empty) before the
    // run and is not a Dark Clipboard folder, so its ACL is left alone.
    assert!(lines.iter().any(|l| l.ends_with(" wer_skipped")), "{text}");
    assert!(
        !lines
            .iter()
            .any(|l| l.contains(" wer ") || l.contains(" wer_failed")),
        "{text}"
    );
    assert!(
        lines
            .iter()
            .any(|l| l.ends_with(" data_dir_acl result=not_ours")),
        "{text}"
    );
    // A fresh data folder: capture is off until Start is pressed in the app (Task 208). The
    // run says so, in the log (a static event) and on the console.
    assert!(lines.iter().any(|l| l.ends_with(" capture_off")), "{text}");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("press Start"), "{stdout}");
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
        .args(["--exit-after-ms", "20000", "--no-global-shortcut"])
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
        .args(["--exit-after-ms", "1500", "--no-global-shortcut"])
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

/// Runs the app for 1.5 s with `toggle_popup` set in a temp settings file; returns the log.
/// Never uses the default Ctrl+Shift+V, so the owner's real shortcut is not touched.
#[cfg(windows)]
#[allow(clippy::unwrap_used)]
fn run_with_shortcut(shortcut: &str, extra: &[&str]) -> String {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("settings.json"),
        format!(r#"{{"settings_version":1,"shortcuts":{{"toggle_popup":"{shortcut}"}}}}"#),
    )
    .unwrap();
    let out = bin()
        .arg("--data-dir")
        .arg(dir.path())
        .args(["--exit-after-ms", "1500"])
        .args(extra)
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(0));
    std::fs::read_to_string(dir.path().join("logs").join("dark-clipboard.log")).unwrap()
}

/// Task 203: the configured shortcut is registered with the OS (an unusual combination, so no
/// real app shortcut is taken); nothing is pressed. Fails if another app already holds
/// Ctrl+Alt+Shift+F24 (the registration then reports `in_use`).
#[cfg(windows)]
#[test]
fn configured_shortcut_is_registered() {
    let log = run_with_shortcut("Ctrl+Alt+Shift+F24", &[]);
    assert!(log.contains(" shortcut status=registered"), "{log}");
}

#[cfg(windows)]
#[test]
fn invalid_shortcut_is_reported_and_the_app_keeps_running() {
    let log = run_with_shortcut("Ctrl+Banana", &[]);
    assert!(log.contains(" shortcut status=invalid"), "{log}");
    assert!(log.contains(" running"), "{log}");
}

#[cfg(windows)]
#[test]
fn no_global_shortcut_flag_registers_nothing() {
    let log = run_with_shortcut("Ctrl+Alt+Shift+F24", &["--no-global-shortcut"]);
    assert!(!log.contains(" shortcut status=registered"), "{log}");
}

/// Task 413 (F407-02): a data folder the app creates gets the private DACL; the run logs it.
/// A temporary folder only, never the real data folder.
#[cfg(windows)]
#[test]
fn headless_restricts_a_data_folder_it_creates() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("new-data");
    let out = bin()
        .arg("--data-dir")
        .arg(&dir)
        .args(["--headless", "--exit-after-ms", "300"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(0));
    let text = std::fs::read_to_string(dir.join("logs").join("dark-clipboard.log")).unwrap();
    assert!(text.contains(" data_dir_acl result=applied"), "{text}");
    assert!(text.contains(" wer_skipped"), "{text}");
    // The folder is marked as Dark Clipboard's own, so later starts keep it private.
    assert!(dir.join(".dark-clipboard-data").is_file());
    let again = bin()
        .arg("--data-dir")
        .arg(&dir)
        .args(["--headless", "--exit-after-ms", "300"])
        .output()
        .unwrap();
    assert_eq!(again.status.code(), Some(0));
    let text = std::fs::read_to_string(dir.join("logs").join("dark-clipboard.log")).unwrap();
    assert!(
        text.contains(" data_dir_acl result=already_private"),
        "{text}"
    );
}

/// The access list of `dir` as `icacls` prints it (read only).
#[cfg(windows)]
fn acl_listing(dir: &std::path::Path) -> String {
    let out = Command::new("icacls").arg(dir).output().unwrap();
    assert!(out.status.success());
    String::from_utf8_lossy(&out.stdout).into_owned()
}

/// Task 413 review B1: a folder the user chose that already holds other files is never given
/// the private DACL, not on the first run and not on the second (when it holds a `history.db`).
/// A temporary folder only.
#[cfg(windows)]
#[test]
fn an_existing_folder_with_other_files_is_never_restricted() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("Shared");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("notes.txt"), b"synthetic, not ours").unwrap();
    let before = acl_listing(&dir);
    for run in 0..2 {
        let out = bin()
            .arg("--data-dir")
            .arg(&dir)
            .args(["--headless", "--exit-after-ms", "300"])
            .output()
            .unwrap();
        assert_eq!(out.status.code(), Some(0), "run {run}");
    }
    assert!(dir.join("history.db").is_file());
    let text = std::fs::read_to_string(dir.join("logs").join("dark-clipboard.log")).unwrap();
    assert_eq!(
        text.matches(" data_dir_acl result=not_ours").count(),
        2,
        "{text}"
    );
    assert!(!dir.join(".dark-clipboard-data").exists());
    assert_eq!(acl_listing(&dir), before);
}
