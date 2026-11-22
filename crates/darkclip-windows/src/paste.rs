//! Auto-paste and focus return (Task 205): "never paste blind".
//!
//! The decisions are pure and run against a `WindowSys` seam, so they are tested on every OS
//! with a fake (no input is ever sent by those tests). `WinPasteInjector` (Windows only) plugs
//! the real Win32 calls from `win.rs` into the same driver.
//!
//! Rules (package 205, D-044):
//! - A target is the window recorded when the popup showed, plus its process id.
//! - Paste only when that window still exists, belongs to the same process, is a normal
//!   top-level window (visible, not minimized, not cloaked, not a tool or no-activate window,
//!   not the taskbar or the desktop), is not ours, and became the foreground window.
//! - UIPI: input is never sent to a process with a higher integrity level than ours, or one
//!   whose level cannot be read (copy only).
//! - Shift, Alt, Win and Enter must be released before the keystroke (bounded wait), so the
//!   paste cannot turn into another shortcut and a held Enter cannot repeat into the target.
//!
//! Nothing here reads a window title or clipboard content, and nothing is logged (GC-06).

// Only the Windows injector uses the driver outside tests.
#![cfg_attr(not(windows), allow(dead_code))]

use darkclip_platform::{PasteTarget, PlatformError};
use std::time::Duration;

/// How long the target may take to become the foreground window.
pub(crate) const FOREGROUND_WAIT: Duration = Duration::from_millis(300);
/// How long the user may keep Shift, Alt, Win or Enter down before the paste is given up.
pub(crate) const KEYS_WAIT: Duration = Duration::from_millis(1_000);
/// Polling step for both waits.
pub(crate) const POLL: Duration = Duration::from_millis(10);

/// What the seam reports about a window. No title, no content.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct WindowFacts {
    pub pid: u32,
    pub visible: bool,
    /// Its own root window and not a child window.
    pub top_level: bool,
    pub minimized: bool,
    /// Hidden by DWM (another virtual desktop, a suspended app).
    pub cloaked: bool,
    /// `WS_EX_TOOLWINDOW`.
    pub tool_window: bool,
    /// `WS_EX_NOACTIVATE`.
    pub no_activate: bool,
    /// The taskbar, the desktop or the shell window.
    pub shell: bool,
}

impl WindowFacts {
    fn is_normal(&self) -> bool {
        self.visible
            && self.top_level
            && !self.minimized
            && !self.cloaked
            && !self.tool_window
            && !self.no_activate
            && !self.shell
    }
}

/// The OS calls the driver needs. `Windows`: `win.rs`; tests: a fake.
pub(crate) trait WindowSys {
    /// `None` when the window no longer exists.
    fn facts(&self, window: u64) -> Option<WindowFacts>;
    /// Mandatory integrity level RID of a process; `None` when it cannot be read.
    fn integrity(&self, pid: u32) -> Option<u32>;
    fn own_integrity(&self) -> Option<u32>;
    fn own_pid(&self) -> u32;
    /// Shift, Alt, Win or Enter is down.
    fn keys_held(&self) -> bool;
    fn set_foreground(&mut self, window: u64) -> bool;
    fn foreground(&self) -> Option<u64>;
    /// Sends Ctrl down, V down, V up, Ctrl up.
    fn send_ctrl_v(&mut self) -> Result<(), PlatformError>;
    fn sleep(&mut self, d: Duration);
}

/// Why the clip was only copied.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Refusal {
    /// No window was recorded.
    NoTarget,
    /// The recorded window is gone.
    Gone,
    /// The handle now belongs to another process (reused), or to us.
    WrongProcess,
    /// Not a normal top-level window (taskbar, tool window, hidden, minimized...).
    NotNormalWindow,
    /// Higher integrity level than ours, or unknown (UIPI would drop the input silently).
    Elevated,
    /// Shift, Alt, Win or Enter stayed down.
    KeysHeld,
    /// The window did not become the foreground window in time.
    NotForeground,
}

impl Refusal {
    /// Static, content-free name.
    pub(crate) fn name(self) -> &'static str {
        match self {
            Refusal::NoTarget => "no target window",
            Refusal::Gone => "target window gone",
            Refusal::WrongProcess => "target window changed process",
            Refusal::NotNormalWindow => "target is not a normal window",
            Refusal::Elevated => "target runs with higher privileges",
            Refusal::KeysHeld => "keys still held",
            Refusal::NotForeground => "target did not take the focus",
        }
    }

    /// `PermissionDenied` for UIPI (the UI explains elevation), `Unsupported` for the rest
    /// (the UI says "paste manually").
    pub(crate) fn to_error(self) -> PlatformError {
        match self {
            Refusal::Elevated => PlatformError::PermissionDenied(self.name()),
            other => PlatformError::Unsupported(other.name()),
        }
    }
}

/// The verdict of the static checks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Verdict {
    Paste,
    CopyOnly(Refusal),
}

/// What the driver does after the checks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Mode {
    /// Enter: return the focus, send nothing.
    Focus,
    /// Ctrl+Enter: return the focus and send Ctrl+V.
    Paste,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Failure {
    Refused(Refusal),
    /// `SendInput` did not insert every event.
    Input(PlatformError),
}

impl Failure {
    pub(crate) fn into_error(self) -> PlatformError {
        match self {
            Failure::Refused(r) => r.to_error(),
            Failure::Input(e) => e,
        }
    }
}

fn recorded(target: &PasteTarget) -> Result<(u64, u32), Refusal> {
    match (target.window, target.pid) {
        (Some(w), Some(p)) if w != 0 && p != 0 => Ok((w, p)),
        _ => Err(Refusal::NoTarget),
    }
}

/// Window checks only (existence, process, normal window). Shared by focus and paste.
fn check_window(sys: &dyn WindowSys, window: u64, pid: u32) -> Result<(), Refusal> {
    let facts = sys.facts(window).ok_or(Refusal::Gone)?;
    if facts.pid != pid || facts.pid == sys.own_pid() {
        return Err(Refusal::WrongProcess);
    }
    if !facts.is_normal() {
        return Err(Refusal::NotNormalWindow);
    }
    Ok(())
}

/// Pure static checks for a paste: window checks, then UIPI.
pub(crate) fn check_target(sys: &dyn WindowSys, target: &PasteTarget) -> Verdict {
    let verdict = recorded(target).and_then(|(window, pid)| {
        check_window(sys, window, pid)?;
        match (sys.integrity(pid), sys.own_integrity()) {
            (Some(theirs), Some(ours)) if theirs <= ours => Ok(()),
            _ => Err(Refusal::Elevated),
        }
    });
    match verdict {
        Ok(()) => Verdict::Paste,
        Err(r) => Verdict::CopyOnly(r),
    }
}

/// Polls `done` every `POLL` until it holds or `wait` has passed. Never sleeps longer than
/// `wait` in total.
fn wait_until(
    sys: &mut dyn WindowSys,
    wait: Duration,
    done: impl Fn(&dyn WindowSys) -> bool,
) -> bool {
    let mut waited = Duration::ZERO;
    loop {
        if done(sys) {
            return true;
        }
        if waited >= wait {
            return false;
        }
        let step = POLL.min(wait - waited);
        sys.sleep(step);
        waited += step;
    }
}

/// Returns the focus to the target and, in `Mode::Paste`, sends Ctrl+V. Every check runs
/// before any input; the window is checked again after it took the focus (a handle can be
/// reused) and right before the keystroke.
pub(crate) fn run(
    sys: &mut dyn WindowSys,
    target: &PasteTarget,
    mode: Mode,
) -> Result<(), Failure> {
    let (window, pid) = recorded(target).map_err(Failure::Refused)?;
    match mode {
        Mode::Focus => check_window(sys, window, pid).map_err(Failure::Refused)?,
        Mode::Paste => {
            if let Verdict::CopyOnly(r) = check_target(sys, target) {
                return Err(Failure::Refused(r));
            }
            // Before the focus moves: a refusal now leaves the popup in front with its toast.
            if !wait_until(sys, KEYS_WAIT, |s| !s.keys_held()) {
                return Err(Failure::Refused(Refusal::KeysHeld));
            }
        }
    }
    if sys.foreground() != Some(window) {
        let _ = sys.set_foreground(window);
        if !wait_until(sys, FOREGROUND_WAIT, |s| s.foreground() == Some(window)) {
            return Err(Failure::Refused(Refusal::NotForeground));
        }
    }
    if mode == Mode::Focus {
        return Ok(());
    }
    // The same window, the same process, still in front: only now is input sent.
    check_window(sys, window, pid).map_err(Failure::Refused)?;
    if sys.foreground() != Some(window) {
        return Err(Failure::Refused(Refusal::NotForeground));
    }
    sys.send_ctrl_v().map_err(Failure::Input)
}

/// The Windows `PasteInjector`: the driver above over the real Win32 calls.
#[cfg(windows)]
pub struct WinPasteInjector;

#[cfg(windows)]
impl darkclip_platform::PasteInjector for WinPasteInjector {
    fn paste(&mut self, target: &PasteTarget) -> Result<(), PlatformError> {
        run(&mut WinWindows, target, Mode::Paste).map_err(Failure::into_error)
    }

    fn focus(&mut self, target: &PasteTarget) -> Result<(), PlatformError> {
        run(&mut WinWindows, target, Mode::Focus).map_err(Failure::into_error)
    }
}

/// `WindowSys` over `win.rs`.
#[cfg(windows)]
struct WinWindows;

#[cfg(windows)]
impl WindowSys for WinWindows {
    fn facts(&self, window: u64) -> Option<WindowFacts> {
        crate::win::window_facts(window)
    }

    fn integrity(&self, pid: u32) -> Option<u32> {
        crate::win::process_integrity(pid)
    }

    fn own_integrity(&self) -> Option<u32> {
        crate::win::own_integrity()
    }

    fn own_pid(&self) -> u32 {
        std::process::id()
    }

    fn keys_held(&self) -> bool {
        crate::win::paste_blocking_keys_held()
    }

    fn set_foreground(&mut self, window: u64) -> bool {
        crate::win::set_foreground(window)
    }

    fn foreground(&self) -> Option<u64> {
        crate::win::foreground()
    }

    fn send_ctrl_v(&mut self) -> Result<(), PlatformError> {
        crate::win::send_ctrl_v()
    }

    fn sleep(&mut self, d: Duration) {
        std::thread::sleep(d);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    const OWN_PID: u32 = 10;
    const PID: u32 = 42;
    const WIN: u64 = 0x1234;
    const MEDIUM: u32 = 0x2000;
    const HIGH: u32 = 0x3000;

    fn normal(pid: u32) -> WindowFacts {
        WindowFacts {
            pid,
            visible: true,
            top_level: true,
            minimized: false,
            cloaked: false,
            tool_window: false,
            no_activate: false,
            shell: false,
        }
    }

    /// A fake desktop. `set_foreground` takes effect after `fg_after_polls` foreground reads
    /// (or never with `None`); keys are held for the first `keys_polls` reads.
    struct Fake {
        windows: HashMap<u64, WindowFacts>,
        integrity: HashMap<u32, u32>,
        own_integrity: Option<u32>,
        fg: Option<u64>,
        pending_fg: Option<u64>,
        fg_after_polls: Option<u32>,
        fg_reads: std::cell::Cell<u32>,
        keys_polls: std::cell::Cell<u32>,
        /// Applied when the target takes the focus (simulates a reused handle).
        on_focus: Option<WindowFacts>,
        events: Vec<&'static str>,
        sent: u32,
        send_result: Result<(), PlatformError>,
        slept: Duration,
    }

    impl Fake {
        fn new() -> Self {
            let mut windows = HashMap::new();
            windows.insert(WIN, normal(PID));
            let mut integrity = HashMap::new();
            integrity.insert(PID, MEDIUM);
            Fake {
                windows,
                integrity,
                own_integrity: Some(MEDIUM),
                fg: Some(0x9999), // our popup
                pending_fg: None,
                fg_after_polls: Some(0),
                fg_reads: std::cell::Cell::new(0),
                keys_polls: std::cell::Cell::new(0),
                on_focus: None,
                events: Vec::new(),
                sent: 0,
                send_result: Ok(()),
                slept: Duration::ZERO,
            }
        }
    }

    impl WindowSys for Fake {
        fn facts(&self, window: u64) -> Option<WindowFacts> {
            self.windows.get(&window).copied()
        }
        fn integrity(&self, pid: u32) -> Option<u32> {
            self.integrity.get(&pid).copied()
        }
        fn own_integrity(&self) -> Option<u32> {
            self.own_integrity
        }
        fn own_pid(&self) -> u32 {
            OWN_PID
        }
        fn keys_held(&self) -> bool {
            let left = self.keys_polls.get();
            if left > 0 {
                self.keys_polls.set(left - 1);
                true
            } else {
                false
            }
        }
        fn set_foreground(&mut self, window: u64) -> bool {
            self.events.push("set_foreground");
            match self.fg_after_polls {
                Some(0) => {
                    self.fg = Some(window);
                    if let Some(f) = self.on_focus {
                        self.windows.insert(window, f);
                    }
                    true
                }
                Some(_) => {
                    self.pending_fg = Some(window);
                    true
                }
                None => false,
            }
        }
        fn foreground(&self) -> Option<u64> {
            if let (Some(w), Some(n)) = (self.pending_fg, self.fg_after_polls) {
                let reads = self.fg_reads.get() + 1;
                self.fg_reads.set(reads);
                if reads > n {
                    return Some(w);
                }
            }
            self.fg
        }
        fn send_ctrl_v(&mut self) -> Result<(), PlatformError> {
            self.events.push("send");
            self.sent += 1;
            self.send_result.clone()
        }
        fn sleep(&mut self, d: Duration) {
            self.slept += d;
        }
    }

    fn target() -> PasteTarget {
        PasteTarget {
            app_id: None,
            window: Some(WIN),
            pid: Some(PID),
        }
    }

    fn paste(f: &mut Fake) -> Result<(), Failure> {
        run(f, &target(), Mode::Paste)
    }

    fn refused(r: Refusal) -> Result<(), Failure> {
        Err(Failure::Refused(r))
    }

    #[test]
    fn pastes_into_a_normal_window_of_the_recorded_process() {
        let mut f = Fake::new();
        assert_eq!(check_target(&f, &target()), Verdict::Paste);
        assert_eq!(paste(&mut f), Ok(()));
        assert_eq!(f.events, vec!["set_foreground", "send"]);
        assert_eq!(f.sent, 1);
    }

    #[test]
    fn no_recorded_window_is_copy_only() {
        let mut f = Fake::new();
        for t in [
            PasteTarget::default(),
            PasteTarget {
                window: Some(WIN),
                ..PasteTarget::default()
            },
            PasteTarget {
                pid: Some(PID),
                ..PasteTarget::default()
            },
            PasteTarget {
                window: Some(0),
                pid: Some(PID),
                app_id: None,
            },
        ] {
            assert_eq!(check_target(&f, &t), Verdict::CopyOnly(Refusal::NoTarget));
            assert_eq!(run(&mut f, &t, Mode::Paste), refused(Refusal::NoTarget));
            assert_eq!(run(&mut f, &t, Mode::Focus), refused(Refusal::NoTarget));
        }
        assert!(f.events.is_empty());
    }

    #[test]
    fn a_closed_window_is_copy_only() {
        let mut f = Fake::new();
        f.windows.clear();
        assert_eq!(
            check_target(&f, &target()),
            Verdict::CopyOnly(Refusal::Gone)
        );
        assert_eq!(paste(&mut f), refused(Refusal::Gone));
        assert!(f.events.is_empty());
    }

    #[test]
    fn a_handle_now_owned_by_another_process_is_copy_only() {
        let mut f = Fake::new();
        f.windows.insert(WIN, normal(77));
        f.integrity.insert(77, MEDIUM);
        assert_eq!(paste(&mut f), refused(Refusal::WrongProcess));
        assert!(f.events.is_empty());
    }

    #[test]
    fn our_own_window_is_never_a_target() {
        let mut f = Fake::new();
        f.windows.insert(WIN, normal(OWN_PID));
        let t = PasteTarget {
            pid: Some(OWN_PID),
            ..target()
        };
        assert_eq!(
            check_target(&f, &t),
            Verdict::CopyOnly(Refusal::WrongProcess)
        );
        assert_eq!(run(&mut f, &t, Mode::Paste), refused(Refusal::WrongProcess));
    }

    #[test]
    fn abnormal_windows_are_copy_only() {
        let cases: [fn(&mut WindowFacts); 7] = [
            |w| w.visible = false,
            |w| w.top_level = false,
            |w| w.minimized = true,
            |w| w.cloaked = true,
            |w| w.tool_window = true,
            |w| w.no_activate = true,
            // The taskbar after a tray-icon open (Task 204 review).
            |w| w.shell = true,
        ];
        for (i, change) in cases.iter().enumerate() {
            let mut f = Fake::new();
            let mut facts = normal(PID);
            change(&mut facts);
            f.windows.insert(WIN, facts);
            assert_eq!(
                check_target(&f, &target()),
                Verdict::CopyOnly(Refusal::NotNormalWindow),
                "case {i}"
            );
            assert_eq!(paste(&mut f), refused(Refusal::NotNormalWindow), "case {i}");
            assert_eq!(
                run(&mut f, &target(), Mode::Focus),
                refused(Refusal::NotNormalWindow),
                "case {i}"
            );
            assert!(f.events.is_empty(), "case {i}");
        }
    }

    #[test]
    fn an_elevated_target_is_copy_only_and_never_focused_for_paste() {
        let mut f = Fake::new();
        f.integrity.insert(PID, HIGH);
        assert_eq!(
            check_target(&f, &target()),
            Verdict::CopyOnly(Refusal::Elevated)
        );
        assert_eq!(paste(&mut f), refused(Refusal::Elevated));
        assert!(f.events.is_empty());
    }

    #[test]
    fn unknown_integrity_counts_as_elevated() {
        let mut f = Fake::new();
        f.integrity.clear();
        assert_eq!(paste(&mut f), refused(Refusal::Elevated));
        let mut f = Fake::new();
        f.own_integrity = None;
        assert_eq!(paste(&mut f), refused(Refusal::Elevated));
        assert_eq!(f.sent, 0);
    }

    #[test]
    fn equal_or_lower_integrity_pastes_and_elevated_us_can_paste_up_to_our_level() {
        let mut f = Fake::new();
        f.integrity.insert(PID, 0x1000); // low (sandboxed)
        assert_eq!(paste(&mut f), Ok(()));
        let mut f = Fake::new();
        f.integrity.insert(PID, HIGH);
        f.own_integrity = Some(HIGH);
        assert_eq!(paste(&mut f), Ok(()));
    }

    #[test]
    fn held_keys_are_waited_for_before_the_focus_moves() {
        let mut f = Fake::new();
        f.keys_polls.set(5);
        assert_eq!(paste(&mut f), Ok(()));
        assert_eq!(f.slept, POLL * 5);
        assert_eq!(f.events, vec!["set_foreground", "send"]);
    }

    #[test]
    fn keys_held_too_long_is_copy_only_without_moving_the_focus() {
        let mut f = Fake::new();
        f.keys_polls.set(u32::MAX);
        assert_eq!(paste(&mut f), refused(Refusal::KeysHeld));
        assert!(f.events.is_empty());
        assert!(f.slept <= KEYS_WAIT);
        assert!(f.slept >= KEYS_WAIT - POLL);
    }

    #[test]
    fn a_window_that_never_takes_the_focus_is_copy_only() {
        let mut f = Fake::new();
        f.fg_after_polls = None;
        assert_eq!(paste(&mut f), refused(Refusal::NotForeground));
        assert_eq!(f.sent, 0);
        assert!(f.slept <= FOREGROUND_WAIT);
        assert!(f.slept >= FOREGROUND_WAIT - POLL);
    }

    #[test]
    fn a_window_that_takes_the_focus_a_little_later_gets_the_paste() {
        let mut f = Fake::new();
        f.fg_after_polls = Some(3);
        assert_eq!(paste(&mut f), Ok(()));
        assert_eq!(f.sent, 1);
        assert!(f.slept < FOREGROUND_WAIT);
    }

    #[test]
    fn a_target_already_in_front_is_not_refocused() {
        let mut f = Fake::new();
        f.fg = Some(WIN);
        assert_eq!(paste(&mut f), Ok(()));
        assert_eq!(f.events, vec!["send"]);
    }

    #[test]
    fn a_handle_reused_while_focusing_gets_no_input() {
        let mut f = Fake::new();
        f.on_focus = Some(normal(77));
        assert_eq!(paste(&mut f), refused(Refusal::WrongProcess));
        assert_eq!(f.sent, 0);
    }

    #[test]
    fn focus_mode_never_sends_input() {
        let mut f = Fake::new();
        assert_eq!(run(&mut f, &target(), Mode::Focus), Ok(()));
        assert_eq!(f.events, vec!["set_foreground"]);
        // Focus return to an elevated window is allowed (no input is sent), and held keys do
        // not delay it.
        let mut f = Fake::new();
        f.integrity.insert(PID, HIGH);
        f.keys_polls.set(u32::MAX);
        assert_eq!(run(&mut f, &target(), Mode::Focus), Ok(()));
        assert_eq!(f.sent, 0);
        // A window that refuses the focus is reported.
        let mut f = Fake::new();
        f.fg_after_polls = None;
        assert_eq!(
            run(&mut f, &target(), Mode::Focus),
            refused(Refusal::NotForeground)
        );
    }

    #[test]
    fn an_input_failure_is_reported() {
        let mut f = Fake::new();
        f.send_result = Err(PlatformError::Backend { code: 5 });
        assert_eq!(
            paste(&mut f),
            Err(Failure::Input(PlatformError::Backend { code: 5 }))
        );
    }

    // Real Win32, but no input, no focus change, no other window read (GC-19): only this
    // process's own token and its own message-only window.
    #[cfg(windows)]
    #[test]
    fn own_integrity_is_readable_and_matches_our_process() {
        let own = crate::win::own_integrity().unwrap();
        assert!(own >= 0x1000, "at least low integrity");
        assert_eq!(crate::win::process_integrity(std::process::id()), Some(own));
        // A pid that cannot exist (pids are multiples of 4): unknown, so treated as elevated.
        assert_eq!(crate::win::process_integrity(3), None);
    }

    #[cfg(windows)]
    #[test]
    fn a_closed_or_null_handle_has_no_facts() {
        assert_eq!(crate::win::window_facts(0), None);
        let w = crate::win::MessageWindow::create(None).unwrap();
        let raw = w.raw().value();
        let facts = crate::win::window_facts(raw).unwrap();
        assert_eq!(facts.pid, std::process::id());
        assert!(
            !facts.is_normal(),
            "a message-only window is never a target"
        );
        drop(w);
        assert_eq!(crate::win::window_facts(raw), None);
    }

    #[cfg(windows)]
    #[test]
    fn the_injector_refuses_before_any_input() {
        use darkclip_platform::PasteInjector;
        let mut inj = WinPasteInjector;
        assert!(matches!(
            inj.paste(&PasteTarget::default()),
            Err(PlatformError::Unsupported(_))
        ));
        assert!(matches!(
            inj.focus(&PasteTarget::default()),
            Err(PlatformError::Unsupported(_))
        ));
        // Our own window: refused by the process check, before focus or input.
        let w = crate::win::MessageWindow::create(None).unwrap();
        let t = PasteTarget {
            app_id: None,
            window: Some(w.raw().value()),
            pid: Some(std::process::id()),
        };
        assert_eq!(
            inj.paste(&t),
            Err(PlatformError::Unsupported(Refusal::WrongProcess.name()))
        );
        assert_eq!(
            inj.focus(&t),
            Err(PlatformError::Unsupported(Refusal::WrongProcess.name()))
        );
    }

    /// OWNER ONLY, never run by agents (GC-19): it sends Ctrl+V to a real window.
    /// Open an empty Notepad, then run
    /// `cargo test -p darkclip-windows --lib real_paste_into_focused_window -- --ignored --nocapture`
    /// and click into Notepad within 3 seconds. Expected: whatever is on the clipboard is
    /// pasted once into Notepad.
    #[cfg(windows)]
    #[test]
    #[ignore = "sends input"]
    fn real_paste_into_focused_window() {
        use darkclip_platform::PasteInjector;
        std::thread::sleep(Duration::from_secs(3));
        let window = crate::win::foreground().unwrap();
        let pid = crate::win::window_facts(window).unwrap().pid;
        let t = PasteTarget {
            app_id: None,
            window: Some(window),
            pid: Some(pid),
        };
        assert_eq!(WinPasteInjector.paste(&t), Ok(()));
    }

    #[test]
    fn refusals_map_to_static_errors() {
        assert!(matches!(
            Refusal::Elevated.to_error(),
            PlatformError::PermissionDenied(_)
        ));
        for r in [
            Refusal::NoTarget,
            Refusal::Gone,
            Refusal::WrongProcess,
            Refusal::NotNormalWindow,
            Refusal::KeysHeld,
            Refusal::NotForeground,
        ] {
            assert_eq!(r.to_error(), PlatformError::Unsupported(r.name()));
        }
        assert_eq!(
            Failure::Refused(Refusal::Gone).into_error(),
            PlatformError::Unsupported(Refusal::Gone.name())
        );
    }
}
