//! The global popup shortcut (Task 203): a pure parser, the press debounce and the register /
//! change / roll back logic. Everything here is plain data and logic, tested on every platform;
//! the OS registration (`global-hotkey`) lives in `ui/shortcut_host.rs`.
//!
//! The shortcut string comes from `settings.shortcuts.toggle_popup` (the settings field that
//! already exists since Task 011; no schema change). Nothing here touches clipboard content.

// The shortcut is registered on Windows first; other platforms compile the logic and its tests.
#![cfg_attr(not(windows), allow(dead_code))]

use darkclip_core::i18n::Msg;

/// Two presses closer than this count once.
pub const DEBOUNCE_MS: u64 = 150;

/// Why a shortcut string was refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ParseError {
    Empty,
    /// Only modifiers, no key.
    NoKey,
    /// More than one key (`Ctrl+A+B`).
    ManyKeys,
    /// A bare key or Shift + key would break typing; Ctrl, Alt or Win is required.
    NoModifier,
    /// The same modifier twice (`Ctrl+Control+V`).
    Duplicate,
    UnknownKey,
}

impl ParseError {
    /// The message that explains the error (English and Vietnamese through `i18n`).
    pub fn msg(self) -> Msg {
        match self {
            ParseError::Empty => Msg::ShortcutErrEmpty,
            ParseError::NoKey => Msg::ShortcutErrNoKey,
            ParseError::ManyKeys => Msg::ShortcutErrManyKeys,
            ParseError::NoModifier => Msg::ShortcutErrNoModifier,
            ParseError::Duplicate => Msg::ShortcutErrDuplicate,
            ParseError::UnknownKey => Msg::ShortcutErrUnknownKey,
        }
    }
}

/// A parsed shortcut. `code` is the W3C key code name (`KeyV`, `Digit1`, `F5`, `ArrowUp`), the
/// same names `global-hotkey` parses.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Shortcut {
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
    pub win: bool,
    pub code: String,
}

/// Key token (lower case) to code name, or `None`.
fn code_for(token: &str) -> Option<String> {
    let b = token.as_bytes();
    if b.len() == 1 && b[0].is_ascii_lowercase() {
        return Some(format!("Key{}", token.to_ascii_uppercase()));
    }
    if b.len() == 1 && b[0].is_ascii_digit() {
        return Some(format!("Digit{token}"));
    }
    if let Some(digits) = token.strip_prefix('f') {
        let plain = !digits.is_empty()
            && !digits.starts_with('0')
            && digits.bytes().all(|c| c.is_ascii_digit());
        if plain {
            if let Ok(n) = digits.parse::<u8>() {
                if n <= 24 {
                    return Some(format!("F{n}"));
                }
            }
        }
    }
    let name = match token {
        "space" => "Space",
        "enter" | "return" => "Enter",
        "tab" => "Tab",
        "backspace" => "Backspace",
        "delete" | "del" => "Delete",
        "insert" | "ins" => "Insert",
        "home" => "Home",
        "end" => "End",
        "pageup" | "pgup" => "PageUp",
        "pagedown" | "pgdn" => "PageDown",
        "up" => "ArrowUp",
        "down" => "ArrowDown",
        "left" => "ArrowLeft",
        "right" => "ArrowRight",
        _ => return None,
    };
    Some(name.to_string())
}

/// Parses `Ctrl|Alt|Shift|Win` plus exactly one key, joined with `+`; case-insensitive, spaces
/// around parts are ignored. Aliases: `Control`, `Option`, `Super`, `Meta`, `Cmd`, `Command`.
/// At least one of Ctrl, Alt or Win is required.
pub fn parse(input: &str) -> Result<Shortcut, ParseError> {
    if input.trim().is_empty() {
        return Err(ParseError::Empty);
    }
    let mut s = Shortcut {
        ctrl: false,
        alt: false,
        shift: false,
        win: false,
        code: String::new(),
    };
    for part in input.split('+') {
        let token = part.trim().to_ascii_lowercase();
        let flag = match token.as_str() {
            "ctrl" | "control" => Some(&mut s.ctrl),
            "alt" | "option" => Some(&mut s.alt),
            "shift" => Some(&mut s.shift),
            "win" | "super" | "meta" | "cmd" | "command" => Some(&mut s.win),
            _ => None,
        };
        if let Some(flag) = flag {
            if *flag {
                return Err(ParseError::Duplicate);
            }
            *flag = true;
        } else if !s.code.is_empty() {
            return Err(ParseError::ManyKeys);
        } else {
            s.code = code_for(&token).ok_or(ParseError::UnknownKey)?;
        }
    }
    if s.code.is_empty() {
        return Err(ParseError::NoKey);
    }
    if !(s.ctrl || s.alt || s.win) {
        return Err(ParseError::NoModifier);
    }
    Ok(s)
}

impl Shortcut {
    /// Display form, e.g. `Ctrl+Shift+V` (saved by the settings window, Task 207).
    pub fn display(&self) -> String {
        let mut out = String::new();
        for (on, name) in [
            (self.ctrl, "Ctrl"),
            (self.alt, "Alt"),
            (self.shift, "Shift"),
            (self.win, "Win"),
        ] {
            if on {
                out.push_str(name);
                out.push('+');
            }
        }
        let key = self
            .code
            .strip_prefix("Key")
            .or_else(|| self.code.strip_prefix("Digit"))
            .or_else(|| self.code.strip_prefix("Arrow"))
            .unwrap_or(&self.code);
        out.push_str(key);
        out
    }
}

/// Counts two presses within [`DEBOUNCE_MS`] once. Time is passed in (milliseconds from any
/// monotonic origin), so the logic is a pure state machine.
#[derive(Debug, Default)]
pub struct Debounce {
    last: Option<u64>,
}

impl Debounce {
    /// True when this press counts. A refused press does not extend the window.
    pub fn press(&mut self, now_ms: u64) -> bool {
        match self.last {
            Some(prev) if now_ms.saturating_sub(prev) < DEBOUNCE_MS => false,
            _ => {
                self.last = Some(now_ms);
                true
            }
        }
    }
}

/// Why the OS refused a registration.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RegError {
    /// Another app owns the combination.
    InUse,
    Other,
}

/// The OS side, so the change / rollback logic runs without an OS.
pub trait Backend {
    fn register(&mut self, s: &Shortcut) -> Result<(), RegError>;
    fn unregister(&mut self, s: &Shortcut);
}

/// Why the popup shortcut is not working (shown in the tray tooltip and the settings window).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Failure {
    Invalid(ParseError),
    InUse,
    Other,
}

/// Where the shortcut stands. A failure never stops the app; `--toggle` and the tray still work.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    Active,
    Failed(Failure),
    /// Registration is switched off (tests, platforms without support).
    Off,
}

impl Status {
    /// The warning to show, or `None` when nothing is wrong.
    pub fn warning(self) -> Option<Msg> {
        match self {
            Status::Active | Status::Off => None,
            Status::Failed(Failure::InUse) => Some(Msg::ShortcutInUse),
            Status::Failed(Failure::Other) => Some(Msg::ShortcutRegisterFailed),
            Status::Failed(Failure::Invalid(e)) => Some(e.msg()),
        }
    }

    /// Static name for the log.
    pub fn log_name(self) -> &'static str {
        match self {
            Status::Active => "registered",
            Status::Off => "off",
            Status::Failed(Failure::InUse) => "in_use",
            Status::Failed(Failure::Other) => "register_failed",
            Status::Failed(Failure::Invalid(_)) => "invalid",
        }
    }
}

/// Registers the configured shortcut and follows changes: the old one is unregistered first and
/// restored when the new one cannot be registered.
pub struct Registrar<B: Backend> {
    backend: B,
    active: Option<Shortcut>,
    /// The last configured string that was acted on, so a refresh does not retry a failure.
    last_wanted: Option<String>,
    status: Status,
}

impl<B: Backend> Registrar<B> {
    pub fn new(backend: B) -> Self {
        Self {
            backend,
            active: None,
            last_wanted: None,
            status: Status::Off,
        }
    }

    /// Brings the registration in line with `wanted` and returns the status. Does nothing when
    /// `wanted` is what was last handled.
    pub fn apply(&mut self, wanted: &str) -> Status {
        if self.last_wanted.as_deref() == Some(wanted) {
            return self.status;
        }
        self.last_wanted = Some(wanted.to_string());
        let new = match parse(wanted) {
            // An invalid string leaves any working registration in place.
            Err(e) => {
                self.status = Status::Failed(Failure::Invalid(e));
                return self.status;
            }
            Ok(s) => s,
        };
        if self.active.as_ref() == Some(&new) {
            self.status = Status::Active;
            return self.status;
        }
        let old = self.active.take();
        if let Some(o) = &old {
            self.backend.unregister(o);
        }
        self.status = match self.backend.register(&new) {
            Ok(()) => {
                self.active = Some(new);
                Status::Active
            }
            Err(e) => {
                // Roll back: put the previous shortcut back.
                if let Some(o) = old {
                    if self.backend.register(&o).is_ok() {
                        self.active = Some(o);
                    }
                }
                Status::Failed(match e {
                    RegError::InUse => Failure::InUse,
                    RegError::Other => Failure::Other,
                })
            }
        };
        self.status
    }

    /// Unregisters the active shortcut (shutdown).
    pub fn release(&mut self) {
        if let Some(a) = self.active.take() {
            self.backend.unregister(&a);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use darkclip_core::i18n::{t, Lang};

    fn ok(s: &str) -> Shortcut {
        parse(s).unwrap()
    }

    #[test]
    fn default_shortcut_parses_and_displays() {
        let s = ok("Ctrl+Shift+V");
        assert!(s.ctrl && s.shift && !s.alt && !s.win);
        assert_eq!(s.code, "KeyV");
        assert_eq!(s.display(), "Ctrl+Shift+V");
    }

    #[test]
    fn parsing_is_case_insensitive_and_ignores_spaces() {
        assert_eq!(ok("ctrl + SHIFT + v"), ok("Ctrl+Shift+V"));
        assert_eq!(ok("CONTROL+Alt+f5").display(), "Ctrl+Alt+F5");
    }

    #[test]
    fn platform_names_are_accepted() {
        assert_eq!(ok("Win+Space"), ok("super+space"));
        assert_eq!(ok("Cmd+Shift+9"), ok("Win+Shift+9"));
        assert_eq!(ok("Option+Up").code, "ArrowUp");
        assert_eq!(ok("Ctrl+PgDn").code, "PageDown");
        assert_eq!(ok("Ctrl+Alt+Del").code, "Delete");
        assert_eq!(ok("Ctrl+F24").code, "F24");
    }

    #[test]
    fn invalid_strings_give_specific_errors() {
        assert_eq!(parse(""), Err(ParseError::Empty));
        assert_eq!(parse("   "), Err(ParseError::Empty));
        assert_eq!(parse("Ctrl+Shift"), Err(ParseError::NoKey));
        assert_eq!(parse("Ctrl+"), Err(ParseError::UnknownKey));
        assert_eq!(parse("V"), Err(ParseError::NoModifier));
        assert_eq!(parse("Shift+V"), Err(ParseError::NoModifier));
        assert_eq!(parse("Ctrl+A+B"), Err(ParseError::ManyKeys));
        assert_eq!(parse("Ctrl+Banana"), Err(ParseError::UnknownKey));
        assert_eq!(parse("Ctrl+F25"), Err(ParseError::UnknownKey));
        assert_eq!(parse("Ctrl+F0"), Err(ParseError::UnknownKey));
        assert_eq!(parse("Ctrl++V"), Err(ParseError::UnknownKey));
    }

    #[test]
    fn duplicate_modifiers_are_refused() {
        assert_eq!(parse("Ctrl+Ctrl+V"), Err(ParseError::Duplicate));
        assert_eq!(parse("Ctrl+Control+V"), Err(ParseError::Duplicate));
        assert_eq!(parse("Win+Super+V"), Err(ParseError::Duplicate));
    }

    #[test]
    fn every_error_has_an_english_and_a_vietnamese_message() {
        for e in [
            ParseError::Empty,
            ParseError::NoKey,
            ParseError::ManyKeys,
            ParseError::NoModifier,
            ParseError::Duplicate,
            ParseError::UnknownKey,
        ] {
            let (en, vi) = (t(Lang::En, e.msg()), t(Lang::Vi, e.msg()));
            assert!(!en.is_empty() && !vi.is_empty());
            assert_ne!(en, vi);
        }
        assert_eq!(
            t(Lang::En, ParseError::NoModifier.msg()),
            "Shortcut needs Ctrl, Alt or Win"
        );
    }

    #[test]
    fn two_presses_within_150ms_count_once() {
        let mut d = Debounce::default();
        assert!(d.press(1_000));
        assert!(!d.press(1_100));
        assert!(!d.press(1_149));
        // The refused presses did not move the window: 150 ms after the first one counts.
        assert!(d.press(1_150));
        assert!(!d.press(1_200));
        assert!(d.press(5_000));
    }

    #[test]
    fn a_clock_that_goes_back_does_not_panic_or_count() {
        let mut d = Debounce::default();
        assert!(d.press(1_000));
        assert!(!d.press(500));
    }

    /// Records what the registrar asks the OS to do.
    #[derive(Default)]
    struct Fake {
        held: Vec<String>,
        calls: Vec<String>,
        /// Shortcuts (display form) that fail with `InUse`.
        taken: Vec<String>,
    }

    impl Backend for &mut Fake {
        fn register(&mut self, s: &Shortcut) -> Result<(), RegError> {
            self.calls.push(format!("reg {}", s.display()));
            if self.taken.contains(&s.display()) {
                return Err(RegError::InUse);
            }
            self.held.push(s.display());
            Ok(())
        }
        fn unregister(&mut self, s: &Shortcut) {
            self.calls.push(format!("unreg {}", s.display()));
            self.held.retain(|h| *h != s.display());
        }
    }

    #[test]
    fn registers_once_and_does_not_retry_the_same_string() {
        let mut fake = Fake::default();
        let mut r = Registrar::new(&mut fake);
        assert_eq!(r.apply("Ctrl+Shift+V"), Status::Active);
        assert_eq!(r.apply("Ctrl+Shift+V"), Status::Active);
        drop(r);
        assert_eq!(fake.calls, vec!["reg Ctrl+Shift+V"]);
    }

    #[test]
    fn registration_failure_is_reported_and_not_retried() {
        let mut fake = Fake {
            taken: vec!["Ctrl+Shift+V".into()],
            ..Fake::default()
        };
        let mut r = Registrar::new(&mut fake);
        let st = r.apply("Ctrl+Shift+V");
        assert_eq!(st, Status::Failed(Failure::InUse));
        assert_eq!(st.warning(), Some(Msg::ShortcutInUse));
        assert_eq!(r.apply("Ctrl+Shift+V"), st);
        assert_eq!(
            t(Lang::En, Msg::ShortcutInUse),
            "Shortcut in use by another app"
        );
        drop(r);
        assert_eq!(fake.calls, vec!["reg Ctrl+Shift+V"]);
    }

    #[test]
    fn changing_unregisters_the_old_one_first() {
        let mut fake = Fake::default();
        let mut r = Registrar::new(&mut fake);
        r.apply("Ctrl+Shift+V");
        assert_eq!(r.apply("Ctrl+Alt+H"), Status::Active);
        drop(r);
        assert_eq!(
            fake.calls,
            vec!["reg Ctrl+Shift+V", "unreg Ctrl+Shift+V", "reg Ctrl+Alt+H"]
        );
        assert_eq!(fake.held, vec!["Ctrl+Alt+H"]);
    }

    #[test]
    fn a_failed_change_rolls_back_to_the_old_shortcut() {
        let mut fake = Fake {
            taken: vec!["Ctrl+Alt+H".into()],
            ..Fake::default()
        };
        let mut r = Registrar::new(&mut fake);
        r.apply("Ctrl+Shift+V");
        assert_eq!(r.apply("Ctrl+Alt+H"), Status::Failed(Failure::InUse));
        // Back to the old one; a later release unregisters it.
        r.release();
        drop(r);
        assert_eq!(
            fake.calls,
            vec![
                "reg Ctrl+Shift+V",
                "unreg Ctrl+Shift+V",
                "reg Ctrl+Alt+H",
                "reg Ctrl+Shift+V",
                "unreg Ctrl+Shift+V"
            ]
        );
        assert!(fake.held.is_empty());
    }

    #[test]
    fn an_invalid_string_keeps_the_working_shortcut() {
        let mut fake = Fake::default();
        let mut r = Registrar::new(&mut fake);
        r.apply("Ctrl+Shift+V");
        let st = r.apply("Ctrl+Banana");
        assert_eq!(st, Status::Failed(Failure::Invalid(ParseError::UnknownKey)));
        assert_eq!(st.warning(), Some(Msg::ShortcutErrUnknownKey));
        drop(r);
        assert_eq!(fake.calls, vec!["reg Ctrl+Shift+V"]);
        assert_eq!(fake.held, vec!["Ctrl+Shift+V"]);
    }

    #[test]
    fn ok_and_off_have_no_warning() {
        assert_eq!(Status::Active.warning(), None);
        assert_eq!(Status::Off.warning(), None);
        assert_eq!(Status::Failed(Failure::Other).log_name(), "register_failed");
    }
}
