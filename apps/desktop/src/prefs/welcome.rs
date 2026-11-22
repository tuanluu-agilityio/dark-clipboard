//! The first-run welcome's answer (Task 208): what Start and Not now change, without a window.
//! Start turns capture on, records the answer and (when asked) registers start at login; Not
//! now records the answer and keeps capture off. Nothing here logs.

use super::model::sync_login;
use darkclip_app::settings::Settings;
use darkclip_platform::Autostart;

/// What answering the welcome did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WelcomeResult {
    Done,
    /// Done, but start at login could not be registered (capture is on regardless).
    DoneLoginFailed,
    /// Nothing was saved; the welcome stays open.
    Failed,
    /// The settings file is read-only (D-028): nothing was written.
    ReadOnly,
}

/// Answers the welcome. `login` is the checkbox (only used by Start). `update` writes the
/// settings (true = written). A failed write puts a changed registration back.
pub fn run_welcome(
    read_only: bool,
    start: bool,
    login: bool,
    base: &Settings,
    auto: Option<&dyn Autostart>,
    update: impl FnOnce(Settings) -> bool,
) -> WelcomeResult {
    if read_only {
        return WelcomeResult::ReadOnly;
    }
    let mut new = base.clone();
    new.finish_onboarding(start);
    let mut before = None;
    let mut failed = false;
    if start {
        match sync_login(auto, login, base.startup.launch_at_login) {
            Ok(prev) => {
                before = prev;
                new.startup.launch_at_login = login;
            }
            Err(actual) => {
                failed = true;
                new.startup.launch_at_login = actual;
            }
        }
    }
    if !update(new) {
        if let (Some(before), Some(auto)) = (before, auto) {
            let _ = auto.set_enabled(before);
        }
        return WelcomeResult::Failed;
    }
    if failed {
        WelcomeResult::DoneLoginFailed
    } else {
        WelcomeResult::Done
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use darkclip_platform::fake::FakeAutostart;
    use std::cell::RefCell;

    fn answer(
        start: bool,
        login: bool,
        base: &Settings,
        auto: Option<&dyn Autostart>,
        update_ok: bool,
    ) -> (WelcomeResult, Option<Settings>) {
        let written = RefCell::new(None);
        let r = run_welcome(false, start, login, base, auto, |s| {
            *written.borrow_mut() = Some(s);
            update_ok
        });
        (r, written.into_inner())
    }

    #[test]
    fn start_enables_capture_and_records_the_answer() {
        let (r, w) = answer(true, false, &Settings::first_run(), None, true);
        assert_eq!(r, WelcomeResult::Done);
        let w = w.unwrap();
        assert!(w.privacy.capture_enabled && !w.needs_welcome());
        assert!(!w.startup.launch_at_login);
    }

    #[test]
    fn not_now_keeps_capture_off_records_the_answer_and_ignores_the_checkbox() {
        let fake = FakeAutostart::new(false, false);
        let (r, w) = answer(false, true, &Settings::first_run(), Some(&fake), true);
        assert_eq!(r, WelcomeResult::Done);
        let w = w.unwrap();
        assert!(!w.privacy.capture_enabled && !w.needs_welcome());
        assert!(!w.startup.launch_at_login);
        assert!(!fake.enabled());
    }

    #[test]
    fn start_with_the_checkbox_registers_start_at_login() {
        let fake = FakeAutostart::new(false, false);
        let (r, w) = answer(true, true, &Settings::first_run(), Some(&fake), true);
        assert_eq!(r, WelcomeResult::Done);
        assert!(fake.enabled());
        assert!(w.unwrap().startup.launch_at_login);
    }

    #[test]
    fn a_failed_registration_still_starts_capture_and_says_so() {
        let fake = FakeAutostart::new(false, true);
        let (r, w) = answer(true, true, &Settings::first_run(), Some(&fake), true);
        assert_eq!(r, WelcomeResult::DoneLoginFailed);
        let w = w.unwrap();
        assert!(w.privacy.capture_enabled && !w.startup.launch_at_login);
    }

    #[test]
    fn a_failed_write_changes_nothing_and_undoes_the_registration() {
        let fake = FakeAutostart::new(false, false);
        let (r, _) = answer(true, true, &Settings::first_run(), Some(&fake), false);
        assert_eq!(r, WelcomeResult::Failed);
        assert!(!fake.enabled());
    }

    #[test]
    fn a_read_only_file_is_never_written() {
        let mut called = false;
        let r = run_welcome(true, true, true, &Settings::first_run(), None, |_| {
            called = true;
            true
        });
        assert_eq!(r, WelcomeResult::ReadOnly);
        assert!(!called);
    }
}
