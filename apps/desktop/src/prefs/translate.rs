//! Settings → Translate (Task 606) without a window: the consent step of the Enable switch, the
//! API key entry (length cap, mask, store and remove through `SecretStore`), the direction and
//! model choices and the Test result text. Pure apart from the `SecretStore` it is handed, so
//! the real Credential Manager store and the fake one behave the same here, and the headless
//! tests (`tests/translate_settings.rs`) compile this file as it is.
//!
//! Privacy (GC-06): the key is never logged, formatted with `Debug` or returned. Only its last
//! four characters leave this module, inside the mask. Every `String` that held the key is
//! wiped before it is dropped.

use darkclip_ai::{Direction, DEFAULT_MODEL, MODELS};
use darkclip_core::i18n::{t, Lang, Msg};
use darkclip_core::sensitive::wipe_string;
use darkclip_core::Redacted;
use darkclip_platform::{PlatformError, SecretStore, ANTHROPIC_KEY_TARGET};

/// The consent version the dialog records (`AiSettings::consent_version`).
pub const CONSENT_VERSION: u32 = 1;
/// The largest key accepted: the Credential Manager blob maximum (`CRED_MAX_CREDENTIAL_BLOB_SIZE`).
pub const MAX_KEY_BYTES: usize = 2560;
/// The fixed text the Test button translates (privacy-design §9): never clip data.
pub const TEST_SAMPLE: &str = "Xin chào";
/// At most this many characters of the Test answer are shown.
pub const TEST_SHOWN_CHARS: usize = 60;
/// How many trailing characters of the key the mask shows.
const MASK_TAIL: usize = 4;
const KEY_PREFIX: &str = "sk-ant-";

/// What a click on the Enable switch leads to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SwitchStep {
    /// Turning on without consent: the switch stays off and the consent dialog opens.
    AskConsent,
    /// Turning on with the consent already given.
    On,
    /// Turning off: no dialog.
    Off,
}

/// The step for a switch that the user wants `want_on`, with the stored or drafted consent.
pub fn switch_step(want_on: bool, consent_version: u32) -> SwitchStep {
    match (want_on, consent_version >= CONSENT_VERSION) {
        (false, _) => SwitchStep::Off,
        (true, true) => SwitchStep::On,
        (true, false) => SwitchStep::AskConsent,
    }
}

/// The consent dialog's body: the prototype's five lines as a bulleted list. `shortcut` is the
/// translate shortcut the form holds.
pub fn consent_body(lang: Lang, shortcut: &str) -> String {
    [
        Msg::SettingsTrConsent1,
        Msg::SettingsTrConsent2,
        Msg::SettingsTrConsent3,
        Msg::SettingsTrConsent4,
        Msg::SettingsTrConsent5,
    ]
    .iter()
    .map(|m| format!("\u{2022} {}", t(lang, *m).replace("{key}", shortcut)))
    .collect::<Vec<_>>()
    .join("\n")
}

/// The shortcut the consent dialog names: the form's, else the stored one, else the default.
pub fn consent_shortcut(form: &str, stored: Option<&str>) -> String {
    let form = form.trim();
    if !form.is_empty() {
        return form.to_string();
    }
    match stored.map(str::trim) {
        Some(s) if !s.is_empty() => s.to_string(),
        _ => darkclip_app::settings::AiSettings::default().shortcut,
    }
}

/// The key as shown once it is stored: the `sk-ant-` prefix (when the key has it) and the last
/// four characters only. A key of eight characters or fewer shows no tail at all.
pub fn key_mask(key: &str) -> String {
    let key = key.trim();
    let n = key.chars().count();
    let head = if key.starts_with(KEY_PREFIX) {
        KEY_PREFIX
    } else {
        ""
    };
    if n <= 2 * MASK_TAIL {
        return format!("{head}\u{2026}");
    }
    let tail: String = key.chars().skip(n - MASK_TAIL).collect();
    format!("{head}\u{2026}{tail}")
}

/// Why a key was not stored or removed. Codes only.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyError {
    Empty,
    TooLong,
    Store(i64),
    Remove(i64),
}

/// A content-free code for a store error.
fn code(e: &PlatformError) -> i64 {
    match e {
        PlatformError::Backend { code } => *code,
        PlatformError::Unsupported(_) => -1,
        PlatformError::PermissionDenied(_) => -2,
        PlatformError::Busy => -3,
        PlatformError::TooLarge { .. } => -4,
    }
}

/// The code shown when a key call could not start (as for a busy store): try again.
pub fn busy_code() -> i64 {
    code(&PlatformError::Busy)
}

/// Checks a typed key: trimmed, not empty, at most [`MAX_KEY_BYTES`].
pub fn check_key(input: &str) -> Result<&str, KeyError> {
    let key = input.trim();
    if key.is_empty() {
        Err(KeyError::Empty)
    } else if key.len() > MAX_KEY_BYTES {
        Err(KeyError::TooLong)
    } else {
        Ok(key)
    }
}

/// Stores the typed key (wiped here whatever happens) and returns its mask. Nothing is stored
/// when the key is empty or too long.
pub fn save_key(store: &dyn SecretStore, mut input: String) -> Result<String, KeyError> {
    let result = match check_key(&input) {
        Ok(key) => {
            let mut secret = Redacted(key.to_string());
            let stored = store.set(ANTHROPIC_KEY_TARGET, &secret);
            let mask = key_mask(&secret.0);
            wipe_string(&mut secret.0);
            stored.map(|()| mask).map_err(|e| KeyError::Store(code(&e)))
        }
        Err(e) => Err(e),
    };
    wipe_string(&mut input);
    result
}

/// Removes the stored key (nothing stored is fine).
pub fn remove_key(store: &dyn SecretStore) -> Result<(), KeyError> {
    store
        .delete(ANTHROPIC_KEY_TARGET)
        .map_err(|e| KeyError::Remove(code(&e)))
}

/// The mask of the stored key, read once (the key itself is wiped at once); `None` when no key
/// is stored or the store cannot be read.
pub fn stored_mask(store: &dyn SecretStore) -> Option<String> {
    let mut key = store.get(ANTHROPIC_KEY_TARGET).ok().flatten()?;
    let mask = (!key.0.trim().is_empty()).then(|| key_mask(&key.0));
    wipe_string(&mut key.0);
    mask
}

/// The Credential Manager calls of the key row (the read when the form fills, Save, Remove)
/// run on short worker threads, never on the UI thread (Task 611). Each one starts a new
/// generation; only the latest generation's answer is shown, an older one is dropped. Save and
/// Remove are disabled while the latest call runs.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct KeyCalls {
    gen: u64,
    busy: bool,
}

impl KeyCalls {
    /// A call starts: its generation, to hand back with its answer.
    pub fn start(&mut self) -> u64 {
        self.gen = self.gen.wrapping_add(1);
        self.busy = true;
        self.gen
    }

    /// No call is needed (the form filled without consent): an answer still running is dropped.
    pub fn cancel(&mut self) {
        self.gen = self.gen.wrapping_add(1);
        self.busy = false;
    }

    /// The answer of the call `gen` arrived: `true` when it is the latest one (show it; the row
    /// is no longer busy), `false` for an older one (drop it, nothing changes).
    pub fn finish(&mut self, gen: u64) -> bool {
        if gen != self.gen {
            return false;
        }
        self.busy = false;
        true
    }

    /// The latest call has not answered yet: Save and Remove are disabled.
    pub fn busy(&self) -> bool {
        self.busy
    }
}

/// The status line of the key row.
pub fn key_status(lang: Lang, mask: Option<&str>) -> String {
    match mask {
        Some(m) => t(lang, Msg::SettingsTrKeyStored).replace("{key}", m),
        None => t(lang, Msg::SettingsTrKeyNone).to_string(),
    }
}

/// The message for a key error: the code only, never the key.
pub fn key_error_text(lang: Lang, e: KeyError) -> String {
    match e {
        KeyError::Empty => t(lang, Msg::SettingsTrKeyEmpty).to_string(),
        KeyError::TooLong => t(lang, Msg::SettingsTrKeyTooLong).to_string(),
        KeyError::Store(c) => t(lang, Msg::SettingsTrKeyStoreFailed).replace("{n}", &c.to_string()),
        KeyError::Remove(c) => {
            t(lang, Msg::SettingsTrKeyRemoveFailed).replace("{n}", &c.to_string())
        }
    }
}

/// The direction select: Auto, Always to English, Always to Vietnamese.
pub const DIRECTIONS: [Direction; 3] = [
    Direction::Auto,
    Direction::ToEnglish,
    Direction::ToVietnamese,
];

pub fn direction_index(d: Direction) -> i32 {
    match d {
        Direction::Auto => 0,
        Direction::ToEnglish => 1,
        Direction::ToVietnamese => 2,
    }
}

pub fn direction_at(i: i32) -> Direction {
    usize::try_from(i)
        .ok()
        .and_then(|i| DIRECTIONS.get(i).copied())
        .unwrap_or_default()
}

pub fn direction_labels(lang: Lang) -> [&'static str; 3] {
    [
        t(lang, Msg::SettingsTrDirAuto),
        t(lang, Msg::SettingsTrDirEn),
        t(lang, Msg::SettingsTrDirVi),
    ]
}

/// The model select, in the order of `darkclip_ai::MODELS`; an unknown saved model shows the
/// default.
pub fn model_index(model: &str) -> i32 {
    let m = if MODELS.contains(&model) {
        model
    } else {
        DEFAULT_MODEL
    };
    MODELS
        .iter()
        .position(|x| *x == m)
        .and_then(|i| i32::try_from(i).ok())
        .unwrap_or(0)
}

pub fn model_at(i: i32) -> &'static str {
    usize::try_from(i)
        .ok()
        .and_then(|i| MODELS.get(i).copied())
        .unwrap_or(DEFAULT_MODEL)
}

pub fn model_labels(lang: Lang) -> [&'static str; 2] {
    [
        t(lang, Msg::SettingsTrModelOpus),
        t(lang, Msg::SettingsTrModelHaiku),
    ]
}

/// "OK: "Xin chào" → "…" in 1.2 s": the answer cut to [`TEST_SHOWN_CHARS`] characters (with an
/// ellipsis when cut), the time with one decimal (a comma in Vietnamese).
pub fn test_ok_text(lang: Lang, answer: &str, elapsed_ms: u64) -> String {
    let one_line: String = answer
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect();
    let one_line = one_line.trim();
    let mut shown: String = one_line.chars().take(TEST_SHOWN_CHARS).collect();
    if one_line.chars().count() > TEST_SHOWN_CHARS {
        shown.push('\u{2026}');
    }
    let tenths = (elapsed_ms + 50) / 100;
    let mut secs = format!("{}.{}", tenths / 10, tenths % 10);
    if lang == Lang::Vi {
        secs = secs.replace('.', ",");
    }
    t(lang, Msg::SettingsTrTestOk)
        .replace("{out}", &shown)
        .replace("{secs}", &secs)
}

#[cfg(test)]
mod tests {
    use super::*;
    use darkclip_platform::fake::FakeSecretStore;

    fn synthetic_key(tail: &str) -> String {
        // Runtime-assembled (GC-09): never a key-shaped literal.
        [KEY_PREFIX, "api03-", &"x".repeat(40), tail].concat()
    }

    #[test]
    fn the_switch_asks_for_consent_only_when_turning_on_without_it() {
        assert_eq!(switch_step(true, 0), SwitchStep::AskConsent);
        assert_eq!(switch_step(true, 1), SwitchStep::On);
        assert_eq!(switch_step(true, 2), SwitchStep::On);
        assert_eq!(switch_step(false, 0), SwitchStep::Off);
        assert_eq!(switch_step(false, 1), SwitchStep::Off);
    }

    #[test]
    fn the_consent_names_a_shortcut_even_when_the_field_is_empty() {
        assert_eq!(
            consent_shortcut(" Ctrl+Alt+Y ", Some("Ctrl+Alt+U")),
            "Ctrl+Alt+Y"
        );
        assert_eq!(consent_shortcut("  ", Some("Ctrl+Alt+U")), "Ctrl+Alt+U");
        assert_eq!(consent_shortcut("", Some(" ")), "Ctrl+Alt+T");
        assert_eq!(consent_shortcut("", None), "Ctrl+Alt+T");
    }

    #[test]
    fn the_key_mask_shows_only_the_last_four_characters() {
        let key = synthetic_key("7Qxz");
        let mask = key_mask(&key);
        assert_eq!(mask, "sk-ant-\u{2026}7Qxz");
        assert!(!mask.contains("api03"));
        assert!(!mask.contains("xxxx7"));
        // Without the usual prefix: the tail only.
        assert_eq!(key_mask("abcdefghijklmnop"), "\u{2026}mnop");
        // A short key shows no tail at all.
        assert_eq!(key_mask("abcdefgh"), "\u{2026}");
        assert_eq!(key_mask(""), "\u{2026}");
        // Multi-byte characters are counted as characters.
        assert_eq!(key_mask("ááááááááábcdé"), "\u{2026}bcdé");
    }

    #[test]
    fn save_rejects_empty_and_too_long_keys_and_stores_nothing() {
        let store = FakeSecretStore::default();
        assert_eq!(save_key(&store, String::new()), Err(KeyError::Empty));
        assert_eq!(save_key(&store, "   ".into()), Err(KeyError::Empty));
        let long = "k".repeat(MAX_KEY_BYTES + 1);
        assert_eq!(save_key(&store, long), Err(KeyError::TooLong));
        assert_eq!(stored_mask(&store), None);
        // Exactly the cap is fine.
        let max = "k".repeat(MAX_KEY_BYTES);
        assert!(save_key(&store, max).is_ok());
    }

    #[test]
    fn save_then_remove() {
        let store = FakeSecretStore::default();
        let key = synthetic_key("ab12");
        let mask = save_key(&store, format!("  {key} \n")).unwrap();
        assert_eq!(mask, "sk-ant-\u{2026}ab12");
        // Stored trimmed.
        assert_eq!(store.get(ANTHROPIC_KEY_TARGET).unwrap().unwrap().0, key);
        assert_eq!(stored_mask(&store).as_deref(), Some(mask.as_str()));
        remove_key(&store).unwrap();
        assert_eq!(stored_mask(&store), None);
        // Removing again is fine.
        assert_eq!(remove_key(&store), Ok(()));
    }

    #[test]
    fn only_the_latest_key_call_is_shown() {
        let mut calls = KeyCalls::default();
        assert!(!calls.busy());
        // The form fills (a read), then Save is pressed before the read answers.
        let read = calls.start();
        assert!(calls.busy());
        let save = calls.start();
        // The read answers late: dropped, still busy for the save.
        assert!(!calls.finish(read));
        assert!(calls.busy());
        assert!(calls.finish(save));
        assert!(!calls.busy());
        // A cancel (the form filled without consent) drops the answer still running.
        let remove = calls.start();
        calls.cancel();
        assert!(!calls.busy());
        assert!(!calls.finish(remove));
        assert!(!calls.busy());
        // The counter wraps without matching an older call.
        let mut calls = KeyCalls {
            gen: u64::MAX,
            busy: false,
        };
        let old = calls.gen;
        let new = calls.start();
        assert_eq!(new, 0);
        assert!(!calls.finish(old));
        assert!(calls.finish(new));
    }

    #[test]
    fn texts_carry_codes_and_masks_only() {
        for lang in [Lang::En, Lang::Vi] {
            let s = key_error_text(lang, KeyError::Store(1312));
            assert!(s.contains("1312") && !s.contains('{'), "{s}");
            let s = key_error_text(lang, KeyError::Remove(5));
            assert!(s.contains('5') && !s.contains('{'), "{s}");
            for e in [KeyError::Empty, KeyError::TooLong] {
                assert!(!key_error_text(lang, e).contains('{'));
            }
            let s = key_status(lang, Some("sk-ant-\u{2026}ab12"));
            assert!(
                s.contains("Windows Credential Manager (sk-ant-\u{2026}ab12)"),
                "{s}"
            );
            assert!(!key_status(lang, None).contains('{'));
            let body = consent_body(lang, "Ctrl+Alt+T");
            assert_eq!(body.lines().count(), 5);
            assert!(body.contains("Ctrl+Alt+T") && !body.contains('{'), "{body}");
        }
    }

    #[test]
    fn selects_round_trip() {
        for d in DIRECTIONS {
            assert_eq!(direction_at(direction_index(d)), d);
        }
        assert_eq!(direction_at(9), Direction::Auto);
        assert_eq!(direction_at(-1), Direction::Auto);
        for m in MODELS {
            assert_eq!(model_at(model_index(m)), *m);
        }
        assert_eq!(model_index("gpt"), model_index(DEFAULT_MODEL));
        assert_eq!(model_at(7), DEFAULT_MODEL);
        assert_eq!(model_labels(Lang::En).len(), MODELS.len());
    }

    #[test]
    fn the_test_answer_is_cut_to_sixty_characters() {
        let s = test_ok_text(Lang::En, "Hello", 1234);
        assert_eq!(s, "OK: \"Xin chào\" → \"Hello\" in 1.2 s");
        let s = test_ok_text(Lang::Vi, "Hello", 1250);
        assert_eq!(s, "OK: \"Xin chào\" → \"Hello\" trong 1,3 s");
        let long = "a".repeat(200);
        let s = test_ok_text(Lang::En, &long, 0);
        assert!(
            s.contains(&format!("\"{}\u{2026}\"", "a".repeat(60))),
            "{s}"
        );
        assert!(!s.contains(&"a".repeat(61)));
        // Line breaks of the answer never break the row.
        assert!(!test_ok_text(Lang::En, "a\nb", 0).contains('\n'));
    }
}
