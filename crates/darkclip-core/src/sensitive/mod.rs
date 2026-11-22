//! Sensitive-data detection (secondary safeguard, `docs/security/privacy-design.md` §4).
//!
//! Hand-written, linear-time matchers; no regex. Results carry categories only,
//! never matched text. At most the first [`MAX_SCAN_BYTES`] of the input are scanned.
//! Detection is best effort; known misses are listed in each detector file.
//!
//! This module never logs (GC-06).

mod api_key;
mod card;
mod email;
mod password;
mod phone;
mod private_key;
mod token;
mod wipe;

pub(crate) use wipe::{wipe, wipe_bytes, wipe_slice};

use crate::i18n::{t, Lang, Msg};
use crate::model::SensitiveCategory;

/// Only this many leading bytes of a text are scanned (rounded down to a char boundary).
pub const MAX_SCAN_BYTES: usize = 256 * 1024;

/// Characters of the matched token that a masked preview may show.
pub const MASK_PREFIX_CHARS: usize = 3;

/// Outcome of [`scan`]. Contains categories only, never matched text.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ScanResult {
    /// Enabled categories that were found, in `SensitiveCategory` declaration order, no duplicates.
    pub categories: Vec<SensitiveCategory>,
    /// Hint for a masked preview, taken from the earliest match.
    pub mask_hint: Option<MaskHint>,
}

/// What a masked preview may show: at most [`MASK_PREFIX_CHARS`] leading characters.
#[derive(Clone, PartialEq, Eq)]
pub struct MaskHint {
    /// At most 3 characters.
    pub visible_prefix: String,
    pub category: SensitiveCategory,
}

/// Never prints the visible prefix (GC-06).
impl std::fmt::Debug for MaskHint {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MaskHint")
            .field("visible_prefix", &"<redacted>")
            .field("category", &self.category)
            .finish()
    }
}

/// A match position. Byte offsets into the scanned text; never the text itself.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Hit {
    pub category: SensitiveCategory,
    pub start: usize,
    pub end: usize,
}

/// One matcher per category. Implementations must run in time linear in `text.len()`.
pub(crate) trait Detector {
    fn scan(&self, text: &str, out: &mut Vec<Hit>);
}

/// Detectors in `SensitiveCategory` declaration order.
fn detectors() -> [(SensitiveCategory, &'static dyn Detector); 7] {
    [
        (SensitiveCategory::ApiKey, &api_key::ApiKeyDetector),
        (SensitiveCategory::Token, &token::JwtDetector),
        (
            SensitiveCategory::PrivateKey,
            &private_key::PrivateKeyDetector,
        ),
        (SensitiveCategory::Password, &password::PasswordDetector),
        (SensitiveCategory::PaymentCard, &card::CardDetector),
        (SensitiveCategory::Email, &email::EmailDetector),
        (SensitiveCategory::Phone, &phone::PhoneDetector),
    ]
}

/// The longest prefix of `text` that is at most [`MAX_SCAN_BYTES`] long and ends on a char boundary.
fn scan_window(text: &str) -> &str {
    if text.len() <= MAX_SCAN_BYTES {
        return text;
    }
    let mut end = MAX_SCAN_BYTES;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    text.get(..end).unwrap_or("")
}

/// Scans `text` for the `enabled` categories.
pub fn scan(text: &str, enabled: &[SensitiveCategory]) -> ScanResult {
    let text = scan_window(text);
    let mut categories = Vec::new();
    let mut first: Option<Hit> = None;
    let mut hits = Vec::new();
    for (category, detector) in detectors() {
        if !enabled.contains(&category) {
            continue;
        }
        hits.clear();
        detector.scan(text, &mut hits);
        if let Some(h) = hits.iter().min_by_key(|h| h.start) {
            categories.push(category);
            if first.is_none_or(|f| h.start < f.start) {
                first = Some(*h);
            }
        }
    }
    let mask_hint = first.map(|h| MaskHint {
        visible_prefix: if shows_prefix(h.category) {
            text.get(h.start..h.end)
                .unwrap_or("")
                .chars()
                .take(MASK_PREFIX_CHARS)
                .collect()
        } else {
            String::new()
        },
        category: h.category,
    });
    ScanResult {
        categories,
        mask_hint,
    }
}

fn label(category: SensitiveCategory) -> Msg {
    match category {
        SensitiveCategory::ApiKey => Msg::CategoryApiKey,
        SensitiveCategory::Token => Msg::CategoryToken,
        SensitiveCategory::PrivateKey => Msg::CategoryPrivateKey,
        SensitiveCategory::Password => Msg::CategoryPassword,
        SensitiveCategory::PaymentCard => Msg::CategoryPaymentCard,
        SensitiveCategory::Email => Msg::CategoryEmail,
        SensitiveCategory::Phone => Msg::CategoryPhone,
    }
}

/// Email previews show the category only: any local-part characters identify a person.
fn shows_prefix(category: SensitiveCategory) -> bool {
    category != SensitiveCategory::Email
}

/// Renders a masked preview such as `sk-…•••• (API key)`.
/// Shows at most the first [`MASK_PREFIX_CHARS`] characters of the hint (control characters
/// among them are dropped, never replaced by later ones), and none for Email.
pub fn masked_preview(hint: &MaskHint, lang: Lang) -> String {
    let mut prefix: String = if shows_prefix(hint.category) {
        hint.visible_prefix
            .chars()
            .take(MASK_PREFIX_CHARS)
            .filter(|c| !c.is_control())
            .collect()
    } else {
        String::new()
    };
    let preview = format!("{prefix}…•••• ({})", t(lang, label(hint.category)));
    wipe(&mut prefix);
    preview
}

/// ASCII letter or digit.
pub(crate) fn is_alnum(b: u8) -> bool {
    b.is_ascii_alphanumeric()
}

/// True if the byte before `start` (if any) is not an ASCII letter or digit.
pub(crate) fn boundary_before(bytes: &[u8], start: usize) -> bool {
    start == 0 || !bytes.get(start - 1).is_some_and(|&b| is_alnum(b))
}

/// True if the byte at `end` (if any) is not an ASCII letter or digit.
pub(crate) fn boundary_after(bytes: &[u8], end: usize) -> bool {
    !bytes.get(end).is_some_and(|&b| is_alnum(b))
}

/// Shared helpers for tests: secret-shaped strings are assembled at runtime (GC-09).
#[cfg(test)]
pub(crate) mod testutil {
    use super::*;

    pub fn only(text: &str, cat: SensitiveCategory) -> Vec<SensitiveCategory> {
        scan(text, &[cat]).categories
    }

    pub fn all() -> Vec<SensitiveCategory> {
        vec![
            SensitiveCategory::ApiKey,
            SensitiveCategory::Token,
            SensitiveCategory::PrivateKey,
            SensitiveCategory::Password,
            SensitiveCategory::PaymentCard,
            SensitiveCategory::Email,
            SensitiveCategory::Phone,
        ]
    }

    /// `n` characters cycling through letters and digits.
    pub fn body(n: usize) -> String {
        let pool = b"aB3dE5gH7jK9mN1pQ2rS4tU6vW8xY0z";
        (0..n)
            .map(|i| char::from(pool[i % pool.len()]))
            .collect::<String>()
    }

    /// `n` uppercase letters and digits.
    pub fn upper_body(n: usize) -> String {
        let pool = b"Q7W2E9R4T6Y1U3I8O5P0";
        (0..n)
            .map(|i| char::from(pool[i % pool.len()]))
            .collect::<String>()
    }
}

#[cfg(test)]
mod tests {
    use super::testutil::*;
    use super::*;
    use crate::model::PrivacyPolicy;
    use SensitiveCategory as C;

    fn api_key() -> String {
        format!("{}{}", concat!("sk", "-"), body(32))
    }

    #[test]
    fn false_positive_corpus_is_empty() {
        let corpus = [
            "da39a3ee5e6b4b0d3255bfef95601890afd80709",
            "commit 4b825dc642cb6eb9a060e54bf8d69288fbee4904 merged",
            "550e8400-e29b-41d4-a716-446655440000",
            "123e4567-e89b-12d3-a456-426614174000",
            "version 1.2.3",
            "1.2.3",
            "192.168.100.100",
            "10.0.0.1:8080",
            "2026-11-22",
            "2026-11-22T10:15:30Z",
            "1,234,567.89",
            "$1,234,567,890.12",
            "@mention",
            "thanks @someone for the review",
            "x@2x.png",
            "icon@3x.jpg",
            "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNkYPhfDwAChwGA60e6kgAAAABJRU5ErkJggg==",
            "data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAA",
            "Forgot your password?",
            "The password field is required.",
            "risk-free sk-learn ghp_short",
            "1700000000000",
            "1234567890123456",
            "-----BEGIN PUBLIC KEY-----",
            "eyJ is a prefix",
            "if password == other {",
        ];
        for text in corpus {
            assert_eq!(scan(text, &all()), ScanResult::default());
        }
    }

    #[test]
    fn scan_result_has_no_text() {
        let key = api_key();
        let text = format!("token {key} end");
        let r = scan(&text, &all());
        assert_eq!(r.categories, vec![C::ApiKey]);
        let dbg = format!("{r:?}");
        assert!(!dbg.contains(&key));
        assert!(!dbg.contains(concat!("sk", "-")));
        let alt = format!("{r:#?}");
        assert!(!alt.contains(&key));
    }

    #[test]
    fn mask_hint_uses_first_three_chars() {
        let text = format!("x {}", api_key());
        let hint = scan(&text, &all()).mask_hint.unwrap();
        assert_eq!(hint.category, C::ApiKey);
        assert_eq!(hint.visible_prefix, concat!("sk", "-"));
        assert_eq!(
            masked_preview(&hint, Lang::En),
            concat!("sk", "-…•••• (API key)")
        );
        assert_eq!(
            masked_preview(&hint, Lang::Vi),
            concat!("sk", "-…•••• (Khóa API)")
        );
    }

    #[test]
    fn masked_preview_caps_prefix() {
        let hint = MaskHint {
            visible_prefix: "abcdef".to_string(),
            category: C::Password,
        };
        assert_eq!(masked_preview(&hint, Lang::En), "abc…•••• (Password)");
    }

    #[test]
    fn masked_preview_takes_three_chars_before_filtering() {
        let hint = MaskHint {
            visible_prefix: "\u{7}\u{7}\u{7}def".to_string(),
            category: C::Password,
        };
        let p = masked_preview(&hint, Lang::En);
        assert!(!p.contains("def"));
        assert!(!p.contains('\u{7}'));
        assert_eq!(p, "…•••• (Password)");
    }

    #[test]
    fn email_mask_shows_label_only() {
        let text = concat!("mail ", "someone", "@example.com");
        let hint = scan(text, &all()).mask_hint.unwrap();
        assert_eq!(hint.category, C::Email);
        assert_eq!(hint.visible_prefix, "");
        assert_eq!(masked_preview(&hint, Lang::En), "…•••• (Email)");
        let forged = MaskHint {
            visible_prefix: "som".to_string(),
            category: C::Email,
        };
        assert_eq!(masked_preview(&forged, Lang::En), "…•••• (Email)");
    }

    #[test]
    fn mask_hint_for_password_shows_label_not_value() {
        let text = concat!("pass", "word: ", "hunter2");
        let hint = scan(text, &all()).mask_hint.unwrap();
        assert_eq!(hint.category, C::Password);
        assert_eq!(hint.visible_prefix, "pas");
    }

    #[test]
    fn disabled_categories_are_not_reported() {
        let text = format!("{} and {}", api_key(), concat!("pwd", "=abc123"));
        assert_eq!(scan(&text, &[C::Password]).categories, vec![C::Password]);
        assert_eq!(scan(&text, &[]), ScanResult::default());
        assert_eq!(
            scan(&text, &[C::Password, C::ApiKey]).categories,
            vec![C::ApiKey, C::Password]
        );
    }

    #[test]
    fn phone_and_email_disabled_by_default() {
        let enabled = PrivacyPolicy::default().sensitive_categories_enabled;
        assert!(!enabled.contains(&C::Email));
        assert!(!enabled.contains(&C::Phone));
        let text = concat!("mail ", "someone", "@example.com", " call +84 912 345 678");
        assert_eq!(scan(text, &enabled), ScanResult::default());
        assert_eq!(scan(text, &all()).categories, vec![C::Email, C::Phone]);
    }

    #[test]
    fn large_input_bounded() {
        let big = "a".repeat(10 * 1024 * 1024);
        let start = std::time::Instant::now();
        let r = scan(&big, &all());
        let elapsed = start.elapsed();
        assert_eq!(r, ScanResult::default());
        let bound = if cfg!(debug_assertions) {
            std::time::Duration::from_secs(2)
        } else {
            std::time::Duration::from_millis(200)
        };
        assert!(elapsed < bound, "scan took {elapsed:?}");
        assert_eq!(scan_window(&big).len(), MAX_SCAN_BYTES);
    }

    #[test]
    fn only_first_256_kib_scanned() {
        let key = api_key();
        let inside = format!("{} {key}", "a".repeat(MAX_SCAN_BYTES - key.len() - 1));
        assert_eq!(inside.len(), MAX_SCAN_BYTES);
        assert_eq!(scan(&inside, &all()).categories, vec![C::ApiKey]);
        let outside = format!("{} {key}", "a".repeat(MAX_SCAN_BYTES));
        assert_eq!(scan(&outside, &all()), ScanResult::default());
    }

    #[test]
    fn scan_window_respects_char_boundary() {
        // 'ệ' is 3 bytes; MAX_SCAN_BYTES is not a multiple of 3.
        let text = "ệ".repeat(MAX_SCAN_BYTES);
        let w = scan_window(&text);
        assert!(w.len() <= MAX_SCAN_BYTES && w.len() > MAX_SCAN_BYTES - 3);
        assert_eq!(scan_window("short"), "short");
    }

    #[test]
    fn pathological_inputs_are_fast() {
        let bound = if cfg!(debug_assertions) {
            std::time::Duration::from_secs(2)
        } else {
            std::time::Duration::from_millis(200)
        };
        let inputs = [
            "a@".repeat(MAX_SCAN_BYTES / 2),
            "4 ".repeat(MAX_SCAN_BYTES / 2),
            "eyJ.".repeat(MAX_SCAN_BYTES / 4),
            "-----BEGIN ".repeat(MAX_SCAN_BYTES / 11),
            "password:".repeat(MAX_SCAN_BYTES / 9),
            "+1-".repeat(MAX_SCAN_BYTES / 3),
            "sk-".repeat(MAX_SCAN_BYTES / 3),
        ];
        for input in &inputs {
            let start = std::time::Instant::now();
            let _ = scan(input, &all());
            assert!(start.elapsed() < bound);
        }
    }

    #[test]
    fn i18n_label_for_every_category() {
        for cat in all() {
            let hint = MaskHint {
                visible_prefix: "abc".to_string(),
                category: cat,
            };
            for lang in [Lang::En, Lang::Vi] {
                let p = masked_preview(&hint, lang);
                if cat == C::Email {
                    assert!(p.starts_with("…•••• ("));
                } else {
                    assert!(p.starts_with("abc…•••• ("));
                }
                assert!(p.ends_with(')'));
            }
        }
    }
}
