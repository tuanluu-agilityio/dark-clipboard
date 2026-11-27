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
//! - Shift, Alt (AltGr included), Win and Enter must be released before the focus moves
//!   (bounded wait) and still be up right before the keystroke, so the paste cannot turn into
//!   another shortcut and a held Enter cannot repeat into the target. A focus return without
//!   paste waits (bounded) for Alt and Enter, then moves the focus anyway.
//! - Copy (Task 604, translate in place): the same checks as a paste, but the target must
//!   already be the foreground window; the focus is never moved. Ctrl+C, or Ctrl+A then
//!   Ctrl+C with the foreground and keys checked again between them.
//! - Only Ctrl+A, Ctrl+C and Ctrl+V can ever be sent (`CtrlKey`).
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
/// Pause between Ctrl+A and Ctrl+C, so the target applies the selection before the copy.
pub(crate) const SELECT_ALL_GAP: Duration = Duration::from_millis(30);

/// The only keys ever sent, each with Ctrl held. A closed set: no arbitrary key can be sent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CtrlKey {
    /// Select all.
    A,
    /// Copy.
    C,
    /// Paste.
    V,
}

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
    /// Alt (AltGr included) or Enter is down: a focus return waits for them, so their key-up
    /// or a repeat does not land in the target.
    fn focus_keys_held(&self) -> bool;
    fn set_foreground(&mut self, window: u64) -> bool;
    fn foreground(&self) -> Option<u64>;
    /// Sends Ctrl down, `key` down, `key` up, Ctrl up.
    fn send_ctrl(&mut self, key: CtrlKey) -> Result<(), PlatformError>;
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
            Refusal::KeysHeld => darkclip_platform::REFUSAL_KEYS_HELD,
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
    /// Translate in place (Task 604): Ctrl+C to the window already in front; no focus move.
    Copy,
    /// As `Copy`, after Ctrl+A.
    SelectAllCopy,
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
///
/// `Mode::Copy` / `Mode::SelectAllCopy` run the same checks but never move the focus: the
/// target must already be the foreground window. `SelectAllCopy` checks the window, the
/// foreground and the keys again between Ctrl+A and Ctrl+C (a refusal there leaves only a harmless
/// selection). The caller's own shortcut key is not waited for here (Task 605 does it).
pub(crate) fn run(
    sys: &mut dyn WindowSys,
    target: &PasteTarget,
    mode: Mode,
) -> Result<(), Failure> {
    let (window, pid) = recorded(target).map_err(Failure::Refused)?;
    match mode {
        Mode::Focus => {
            check_window(sys, window, pid).map_err(Failure::Refused)?;
            // Bounded; on timeout the focus moves anyway (no input is sent in this mode).
            let _ = wait_until(sys, KEYS_WAIT, |s| !s.focus_keys_held());
        }
        Mode::Paste | Mode::Copy | Mode::SelectAllCopy => {
            if let Verdict::CopyOnly(r) = check_target(sys, target) {
                return Err(Failure::Refused(r));
            }
            // Before the focus moves: a refusal now leaves the popup in front with its toast.
            if !wait_until(sys, KEYS_WAIT, |s| !s.keys_held()) {
                return Err(Failure::Refused(Refusal::KeysHeld));
            }
        }
    }
    match mode {
        Mode::Focus | Mode::Paste => {
            if sys.foreground() != Some(window) {
                let _ = sys.set_foreground(window);
                if !wait_until(sys, FOREGROUND_WAIT, |s| s.foreground() == Some(window)) {
                    return Err(Failure::Refused(Refusal::NotForeground));
                }
            }
        }
        // The copy never steals the focus.
        Mode::Copy | Mode::SelectAllCopy => {
            if sys.foreground() != Some(window) {
                return Err(Failure::Refused(Refusal::NotForeground));
            }
        }
    }
    if mode == Mode::Focus {
        return Ok(());
    }
    // The same window, the same process, no key pressed meanwhile, still in front: only now
    // is input sent.
    check_window(sys, window, pid).map_err(Failure::Refused)?;
    still_ready(sys, window)?;
    match mode {
        Mode::Paste => sys.send_ctrl(CtrlKey::V).map_err(Failure::Input),
        Mode::Copy => sys.send_ctrl(CtrlKey::C).map_err(Failure::Input),
        Mode::SelectAllCopy => {
            sys.send_ctrl(CtrlKey::A).map_err(Failure::Input)?;
            sys.sleep(SELECT_ALL_GAP);
            // The handle may have been closed or reused during the gap.
            check_window(sys, window, pid).map_err(Failure::Refused)?;
            still_ready(sys, window)?;
            sys.send_ctrl(CtrlKey::C).map_err(Failure::Input)
        }
        // Returned above.
        Mode::Focus => Ok(()),
    }
}

/// No blocking key is down and `window` is still the foreground window.
fn still_ready(sys: &dyn WindowSys, window: u64) -> Result<(), Failure> {
    if sys.keys_held() {
        return Err(Failure::Refused(Refusal::KeysHeld));
    }
    if sys.foreground() != Some(window) {
        return Err(Failure::Refused(Refusal::NotForeground));
    }
    Ok(())
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

    /// Ctrl+C (or Ctrl+A, then Ctrl+C) to `target`, which must already be in front; never
    /// moves the focus. Does not wait for the caller's own shortcut key (e.g. the `T` of the
    /// translate shortcut) to come up: Task 605 waits for it before calling.
    fn copy(&mut self, target: &PasteTarget, select_all: bool) -> Result<(), PlatformError> {
        let mode = if select_all {
            Mode::SelectAllCopy
        } else {
            Mode::Copy
        };
        run(&mut WinWindows, target, mode).map_err(Failure::into_error)
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

    fn focus_keys_held(&self) -> bool {
        crate::win::focus_blocking_keys_held()
    }

    fn set_foreground(&mut self, window: u64) -> bool {
        crate::win::set_foreground(window)
    }

    fn foreground(&self) -> Option<u64> {
        crate::win::foreground()
    }

    fn send_ctrl(&mut self, key: CtrlKey) -> Result<(), PlatformError> {
        crate::win::send_ctrl(key)
    }

    fn sleep(&mut self, d: Duration) {
        std::thread::sleep(d);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[test]
    fn keys_held_refusal_uses_the_shared_reason() {
        assert_eq!(
            Refusal::KeysHeld.to_error(),
            PlatformError::Unsupported(darkclip_platform::REFUSAL_KEYS_HELD)
        );
    }

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

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    enum Step {
        Key(CtrlKey),
        Sleep(Duration),
    }

    fn keys(f: &Fake) -> Vec<CtrlKey> {
        f.log
            .iter()
            .filter_map(|s| match s {
                Step::Key(k) => Some(*k),
                Step::Sleep(_) => None,
            })
            .collect()
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
        /// Alt or Enter held for the first n reads (focus-mode wait).
        focus_keys_polls: std::cell::Cell<u32>,
        /// Keys go down when the target takes the focus (user presses during the wait).
        keys_down_on_focus: bool,
        /// Applied when the target takes the focus (simulates a reused handle).
        on_focus: Option<WindowFacts>,
        events: Vec<&'static str>,
        /// Keys sent and sleeps, in order (copy-mode tests).
        log: Vec<Step>,
        /// The foreground window right after Ctrl+A was sent (the focus moved meanwhile).
        fg_after_a: Option<u64>,
        /// Shift, Alt, Win or Enter go down right after Ctrl+A was sent.
        keys_down_after_a: bool,
        /// The target's facts right after Ctrl+A: `Some(None)` closes it, `Some(Some(f))`
        /// replaces them (a reused handle).
        facts_after_a: Option<Option<WindowFacts>>,
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
                focus_keys_polls: std::cell::Cell::new(0),
                keys_down_on_focus: false,
                on_focus: None,
                events: Vec::new(),
                log: Vec::new(),
                fg_after_a: None,
                keys_down_after_a: false,
                facts_after_a: None,
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
        fn focus_keys_held(&self) -> bool {
            let left = self.focus_keys_polls.get();
            if left > 0 {
                self.focus_keys_polls.set(left - 1);
                true
            } else {
                false
            }
        }
        fn set_foreground(&mut self, window: u64) -> bool {
            self.events.push("set_foreground");
            if self.keys_down_on_focus {
                self.keys_polls.set(u32::MAX);
            }
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
        fn send_ctrl(&mut self, key: CtrlKey) -> Result<(), PlatformError> {
            self.events.push("send");
            self.log.push(Step::Key(key));
            self.sent += 1;
            if key == CtrlKey::A {
                if let Some(w) = self.fg_after_a {
                    self.fg = Some(w);
                }
                if self.keys_down_after_a {
                    self.keys_polls.set(u32::MAX);
                }
                match self.facts_after_a {
                    Some(Some(f)) => {
                        self.windows.insert(WIN, f);
                    }
                    Some(None) => {
                        self.windows.remove(&WIN);
                    }
                    None => {}
                }
            }
            self.send_result.clone()
        }
        fn sleep(&mut self, d: Duration) {
            self.slept += d;
            self.log.push(Step::Sleep(d));
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
    fn keys_pressed_while_the_focus_moves_stop_the_paste() {
        let mut f = Fake::new();
        f.keys_down_on_focus = true;
        assert_eq!(paste(&mut f), refused(Refusal::KeysHeld));
        assert_eq!(f.sent, 0);
        assert_eq!(f.events, vec!["set_foreground"]);
        // Also when the focus arrives a little later.
        let mut f = Fake::new();
        f.keys_down_on_focus = true;
        f.fg_after_polls = Some(3);
        assert_eq!(paste(&mut f), refused(Refusal::KeysHeld));
        assert_eq!(f.sent, 0);
    }

    #[test]
    fn focus_mode_waits_for_enter_and_alt_before_moving_the_focus() {
        let mut f = Fake::new();
        f.focus_keys_polls.set(5);
        assert_eq!(run(&mut f, &target(), Mode::Focus), Ok(()));
        assert_eq!(f.slept, POLL * 5);
        assert_eq!(f.events, vec!["set_foreground"]);
    }

    #[test]
    fn focus_mode_moves_the_focus_after_the_bounded_wait_anyway() {
        let mut f = Fake::new();
        f.focus_keys_polls.set(u32::MAX);
        assert_eq!(run(&mut f, &target(), Mode::Focus), Ok(()));
        assert!(f.slept <= KEYS_WAIT + FOREGROUND_WAIT);
        assert!(f.slept >= KEYS_WAIT - POLL);
        assert_eq!(f.events, vec!["set_foreground"]);
        assert_eq!(f.sent, 0);
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

    // --- Copy modes (Task 604): the target must already be in front; the focus never moves.

    fn copy_fake() -> Fake {
        let mut f = Fake::new();
        f.fg = Some(WIN);
        f
    }

    #[test]
    fn copy_sends_ctrl_c_only_when_foreground() {
        let mut f = copy_fake();
        assert_eq!(run(&mut f, &target(), Mode::Copy), Ok(()));
        assert_eq!(keys(&f), vec![CtrlKey::C]);
        assert_eq!(f.events, vec!["send"], "no set_foreground");
    }

    #[test]
    fn copy_refuses_when_not_foreground_and_never_moves_focus() {
        for mode in [Mode::Copy, Mode::SelectAllCopy] {
            let mut f = Fake::new(); // our popup is in front
            assert_eq!(
                run(&mut f, &target(), mode),
                refused(Refusal::NotForeground)
            );
            assert!(keys(&f).is_empty());
            assert!(f.events.is_empty(), "no set_foreground, no input");
        }
    }

    #[test]
    fn select_all_copy_sends_a_then_c_with_gap() {
        let mut f = copy_fake();
        assert_eq!(run(&mut f, &target(), Mode::SelectAllCopy), Ok(()));
        assert_eq!(
            f.log,
            vec![
                Step::Key(CtrlKey::A),
                Step::Sleep(SELECT_ALL_GAP),
                Step::Key(CtrlKey::C)
            ]
        );
        assert_eq!(f.events, vec!["send", "send"], "no set_foreground");
    }

    #[test]
    fn select_all_copy_stops_after_a_if_focus_moves() {
        let mut f = copy_fake();
        f.fg_after_a = Some(0x7777);
        assert_eq!(
            run(&mut f, &target(), Mode::SelectAllCopy),
            refused(Refusal::NotForeground)
        );
        assert_eq!(keys(&f), vec![CtrlKey::A]);
        assert!(!f.events.contains(&"set_foreground"));
    }

    #[test]
    fn select_all_copy_stops_after_a_if_keys_go_down() {
        let mut f = copy_fake();
        f.keys_down_after_a = true;
        assert_eq!(
            run(&mut f, &target(), Mode::SelectAllCopy),
            refused(Refusal::KeysHeld)
        );
        assert_eq!(keys(&f), vec![CtrlKey::A]);
    }

    #[test]
    fn select_all_copy_stops_after_a_if_window_changes() {
        // Closed during the gap.
        let mut f = copy_fake();
        f.facts_after_a = Some(None);
        assert_eq!(
            run(&mut f, &target(), Mode::SelectAllCopy),
            refused(Refusal::Gone)
        );
        assert_eq!(keys(&f), vec![CtrlKey::A]);
        // Reused by another process during the gap (still in front).
        let mut f = copy_fake();
        f.facts_after_a = Some(Some(normal(77)));
        assert_eq!(
            run(&mut f, &target(), Mode::SelectAllCopy),
            refused(Refusal::WrongProcess)
        );
        assert_eq!(keys(&f), vec![CtrlKey::A]);
    }

    #[test]
    fn copy_refuses_elevated_and_unknown_integrity() {
        for mode in [Mode::Copy, Mode::SelectAllCopy] {
            let mut f = copy_fake();
            f.integrity.insert(PID, HIGH);
            assert_eq!(run(&mut f, &target(), mode), refused(Refusal::Elevated));
            assert!(f.events.is_empty());
            let mut f = copy_fake();
            f.integrity.clear();
            assert_eq!(run(&mut f, &target(), mode), refused(Refusal::Elevated));
            assert!(f.events.is_empty());
            let mut f = copy_fake();
            f.own_integrity = None;
            assert_eq!(run(&mut f, &target(), mode), refused(Refusal::Elevated));
            assert!(f.events.is_empty());
        }
    }

    #[test]
    fn copy_waits_for_held_keys_then_refuses() {
        for mode in [Mode::Copy, Mode::SelectAllCopy] {
            let mut f = copy_fake();
            f.keys_polls.set(u32::MAX);
            assert_eq!(run(&mut f, &target(), mode), refused(Refusal::KeysHeld));
            assert!(f.events.is_empty());
            assert!(f.slept <= KEYS_WAIT);
            assert!(f.slept >= KEYS_WAIT - POLL);
        }
        // Keys released within the wait: the copy goes ahead.
        let mut f = copy_fake();
        f.keys_polls.set(5);
        assert_eq!(run(&mut f, &target(), Mode::Copy), Ok(()));
        assert_eq!(f.slept, POLL * 5);
        assert_eq!(keys(&f), vec![CtrlKey::C]);
    }

    #[test]
    fn copy_refuses_own_window_and_reused_handle() {
        for mode in [Mode::Copy, Mode::SelectAllCopy] {
            // Our own window, in front, with our pid recorded.
            let mut f = copy_fake();
            f.windows.insert(WIN, normal(OWN_PID));
            f.integrity.insert(OWN_PID, MEDIUM);
            let t = PasteTarget {
                pid: Some(OWN_PID),
                ..target()
            };
            assert_eq!(run(&mut f, &t, mode), refused(Refusal::WrongProcess));
            assert!(f.events.is_empty());
            // The handle now belongs to another process than the recorded one.
            let mut f = copy_fake();
            f.windows.insert(WIN, normal(77));
            f.integrity.insert(77, MEDIUM);
            assert_eq!(run(&mut f, &target(), mode), refused(Refusal::WrongProcess));
            assert!(f.events.is_empty());
        }
    }

    #[test]
    fn copy_refuses_abnormal_windows_and_missing_targets() {
        let mut f = copy_fake();
        let mut facts = normal(PID);
        facts.tool_window = true;
        f.windows.insert(WIN, facts);
        assert_eq!(
            run(&mut f, &target(), Mode::Copy),
            refused(Refusal::NotNormalWindow)
        );
        let mut f = copy_fake();
        assert_eq!(
            run(&mut f, &PasteTarget::default(), Mode::SelectAllCopy),
            refused(Refusal::NoTarget)
        );
        assert!(f.events.is_empty());
    }

    #[test]
    fn copy_input_failure_is_reported_and_stops_before_c() {
        let mut f = copy_fake();
        f.send_result = Err(PlatformError::Backend { code: 5 });
        assert_eq!(
            run(&mut f, &target(), Mode::SelectAllCopy),
            Err(Failure::Input(PlatformError::Backend { code: 5 }))
        );
        assert_eq!(keys(&f), vec![CtrlKey::A]);
    }

    #[test]
    fn paste_still_sends_v() {
        let mut f = Fake::new();
        assert_eq!(paste(&mut f), Ok(()));
        assert_eq!(keys(&f), vec![CtrlKey::V]);
        let mut f = Fake::new();
        f.fg = Some(WIN);
        assert_eq!(paste(&mut f), Ok(()));
        assert_eq!(keys(&f), vec![CtrlKey::V]);
        let mut f = Fake::new();
        assert_eq!(run(&mut f, &target(), Mode::Focus), Ok(()));
        assert!(keys(&f).is_empty());
    }

    // Real Win32, but no input, no focus change, no other window read (GC-19).
    #[cfg(windows)]
    #[test]
    fn the_injector_copy_refuses_before_any_input() {
        use darkclip_platform::PasteInjector;
        let mut inj = WinPasteInjector;
        for select_all in [false, true] {
            assert!(matches!(
                inj.copy(&PasteTarget::default(), select_all),
                Err(PlatformError::Unsupported(_))
            ));
            // Our own window: refused by the process check, before any input.
            let w = crate::win::MessageWindow::create(None).unwrap();
            let t = PasteTarget {
                app_id: None,
                window: Some(w.raw().value()),
                pid: Some(std::process::id()),
            };
            assert_eq!(
                inj.copy(&t, select_all),
                Err(PlatformError::Unsupported(Refusal::WrongProcess.name()))
            );
        }
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
