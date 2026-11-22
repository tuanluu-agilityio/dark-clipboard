//! Passwords: the value after a label such as `password:`, `pwd=` or `mật khẩu:`.
//!
//! Known misses: passwords without a label, labels in other languages, Vietnamese text
//! in decomposed (NFD) form, values on the line after the label.

use super::{Detector, Hit};
use crate::model::SensitiveCategory;

pub(crate) struct PasswordDetector;

/// Lowercase labels, compared case-insensitively.
const LABELS: &[&str] = &[
    "password",
    "passwd",
    "passphrase",
    "pwd",
    "mật khẩu",
    "mat khau",
];
/// Blanks allowed between label, separator and value.
const MAX_BLANKS: usize = 16;
/// Characters of a value examined; longer values still count.
const MAX_VALUE_CHARS: usize = 256;
const MIN_VALUE_CHARS: usize = 3;

/// Byte length of `label` at the start of `rest`, compared case-insensitively.
fn match_label(rest: &str, label: &str) -> Option<usize> {
    let mut it = rest.char_indices();
    for l in label.chars() {
        let (_, c) = it.next()?;
        if !c.to_lowercase().eq(std::iter::once(l)) {
            return None;
        }
    }
    Some(it.next().map_or(rest.len(), |(p, _)| p))
}

fn skip_blanks(s: &[u8], mut p: usize) -> usize {
    let limit = p + MAX_BLANKS;
    while p < limit && matches!(s.get(p), Some(b' ' | b'\t')) {
        p += 1;
    }
    p
}

/// Given the text right after a label, returns the byte offset (into `after`) where
/// the value ends, if a separator and a plausible value follow.
fn value_end(after: &str) -> Option<usize> {
    let s = after.as_bytes();
    // The label must end the word: rejects `password_hash`, `passwordless`.
    if s.first()
        .is_some_and(|&b| b.is_ascii_alphanumeric() || b == b'_')
        || after.chars().next().is_some_and(char::is_alphanumeric)
    {
        return None;
    }
    let mut p = 0;
    if matches!(s.first(), Some(b'"' | b'\'')) {
        p = 1;
    }
    p = skip_blanks(s, p);
    match s.get(p) {
        Some(b':') => p += 1,
        Some(b'=') if s.get(p + 1) != Some(&b'=') => p += 1,
        _ if s.get(p..).is_some_and(|r| r.starts_with("：".as_bytes())) => p += "：".len(),
        _ => return None,
    }
    p = skip_blanks(s, p);
    let quote = match s.get(p) {
        Some(&q @ (b'"' | b'\'')) => {
            p += 1;
            Some(char::from(q))
        }
        _ => None,
    };
    let value = after.get(p..)?;
    let mut end = 0;
    for (i, c) in value.char_indices().take(MAX_VALUE_CHARS) {
        let stop = match quote {
            Some(q) => c == q || c == '\n' || c == '\r',
            None => c.is_whitespace(),
        };
        if stop {
            break;
        }
        end = i + c.len_utf8();
    }
    let mut v = value.get(..end)?;
    if quote.is_none() {
        v = v.trim_end_matches([',', ';']);
    }
    let long_enough = v.chars().nth(MIN_VALUE_CHARS - 1).is_some();
    // Masks and rulers such as `********` or `---` are not passwords.
    let meaningful = v.chars().any(|c| !matches!(c, '*' | '•' | '.' | '-' | '_'));
    (long_enough && meaningful).then_some(p + v.len())
}

impl Detector for PasswordDetector {
    /// At each char boundary, a bounded label comparison; after a label, a bounded value scan.
    fn scan(&self, text: &str, out: &mut Vec<Hit>) {
        let mut prev: Option<char> = None;
        for (start, c) in text.char_indices() {
            let at_word_start = !prev.is_some_and(char::is_alphanumeric);
            prev = Some(c);
            if !at_word_start || !matches!(c, 'p' | 'P' | 'm' | 'M') {
                continue;
            }
            let Some(rest) = text.get(start..) else {
                continue;
            };
            for label in LABELS {
                let Some(n) = match_label(rest, label) else {
                    continue;
                };
                if let Some(end) = rest.get(n..).and_then(value_end) {
                    out.push(Hit {
                        category: SensitiveCategory::Password,
                        start,
                        end: start + n + end,
                    });
                    break;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::model::SensitiveCategory as C;
    use crate::sensitive::testutil::*;

    fn hit(s: &str) -> bool {
        only(s, C::Password) == vec![C::Password]
    }

    #[test]
    fn password_after_label() {
        assert!(hit(concat!("pass", "word: ", "hunter2")));
        assert!(hit(concat!("pw", "d=", "abc123")));
        assert!(hit(concat!("mật ", "khẩu: ", "abc")));
        assert!(hit(concat!("MẬT ", "KHẨU: ", "abc")));
        assert!(hit(concat!("mat ", "khau: ", "abc")));
        assert!(hit(concat!("Pass", "word = ", "s3cr3t!")));
        assert!(hit(concat!("DB_PASS", "WORD=", "hunter22")));
        assert!(hit(concat!("{\"pass", "word\": \"", "hunter2\"}")));
        assert!(hit(concat!("user: bob\npass", "wd: ", "hunter2\n")));
        assert!(hit(concat!("passph", "rase: ", "correct horse")));
    }

    #[test]
    fn password_label_alone_negative() {
        let texts = [
            "password",
            "password:",
            "password: ",
            "password:\nnext line",
            "pwd=",
            "Forgot your password?",
            "mật khẩu",
            "mật khẩu:  ",
            "password_hash: abc123",
            "passwordless: true",
            "if password == other {",
            "password: ********",
            "password: \"\"",
            "mypassword: abc123",
        ];
        for t in texts {
            assert!(!hit(t));
        }
    }
}
