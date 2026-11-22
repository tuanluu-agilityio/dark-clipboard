//! API keys with well-known vendor prefixes.
//!
//! Known misses: keys without a vendor prefix, prefixes not listed here, keys split across lines.

use super::{is_alnum, Detector, Hit};
use crate::model::SensitiveCategory;

pub(crate) struct ApiKeyDetector;

/// Bytes that may appear inside a key token.
fn is_token_byte(b: u8) -> bool {
    is_alnum(b) || b == b'_' || b == b'-'
}

struct Classes {
    upper: bool,
    lower: bool,
    digit: bool,
}

fn classes(s: &[u8]) -> Classes {
    Classes {
        upper: s.iter().any(u8::is_ascii_uppercase),
        lower: s.iter().any(u8::is_ascii_lowercase),
        digit: s.iter().any(u8::is_ascii_digit),
    }
}

/// Mixed-case alphanumeric body: real keys are random, kebab-case identifiers are not.
fn random_looking(body: &[u8], min_len: usize) -> bool {
    let c = classes(body);
    body.len() >= min_len && c.upper && c.lower && c.digit
}

fn is_key(tok: &[u8]) -> bool {
    if let Some(body) = tok.strip_prefix(b"sk-") {
        return random_looking(body, 20);
    }
    if let Some(body) = tok.strip_prefix(b"glpat-") {
        return random_looking(body, 20);
    }
    for p in [b"ghp_", b"gho_", b"ghu_", b"ghs_", b"ghr_"] {
        if let Some(body) = tok.strip_prefix(p) {
            return body.len() == 36 && body.iter().all(|&b| is_alnum(b));
        }
    }
    if let Some(body) = tok.strip_prefix(b"github_pat_") {
        return body.len() >= 40
            && body.iter().all(|&b| is_alnum(b) || b == b'_')
            && classes(body).digit;
    }
    for p in [b"xoxa-", b"xoxb-", b"xoxo-", b"xoxp-", b"xoxr-", b"xoxs-"] {
        if let Some(body) = tok.strip_prefix(p) {
            return body.len() >= 10 && classes(body).digit;
        }
    }
    for p in [b"AKIA", b"ASIA"] {
        if let Some(body) = tok.strip_prefix(p) {
            return body.len() == 16
                && body
                    .iter()
                    .all(|&b| b.is_ascii_uppercase() || b.is_ascii_digit());
        }
    }
    if tok.starts_with(b"AIza") {
        return tok.len() == 39;
    }
    false
}

impl Detector for ApiKeyDetector {
    /// One pass over maximal runs of `[A-Za-z0-9_-]`; each run is classified once.
    fn scan(&self, text: &str, out: &mut Vec<Hit>) {
        let bytes = text.as_bytes();
        let mut i = 0;
        while i < bytes.len() {
            if !bytes.get(i).is_some_and(|&b| is_token_byte(b)) {
                i += 1;
                continue;
            }
            let start = i;
            while bytes.get(i).is_some_and(|&b| is_token_byte(b)) {
                i += 1;
            }
            if bytes.get(start..i).is_some_and(is_key) {
                out.push(Hit {
                    category: SensitiveCategory::ApiKey,
                    start,
                    end: i,
                });
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::model::SensitiveCategory as C;
    use crate::sensitive::testutil::*;

    fn hit(s: &str) -> bool {
        only(s, C::ApiKey) == vec![C::ApiKey]
    }

    #[test]
    fn api_key_prefixes_positive() {
        let keys = [
            format!("{}{}", concat!("s", "k-"), body(48)),
            format!("{}{}", concat!("s", "k-proj-"), body(64)),
            format!("{}{}", concat!("gh", "p_"), body(36)),
            format!("{}{}_{}", concat!("github", "_pat_"), body(22), body(59)),
            format!(
                "{}{}-{}-{}",
                concat!("xo", "xb-"),
                "1234567890",
                "9876543210",
                body(24)
            ),
            format!("{}{}", concat!("AK", "IA"), upper_body(16)),
            format!("{}{}", concat!("AI", "za"), body(35)),
            format!("{}{}", concat!("gl", "pat-"), body(20)),
        ];
        for key in &keys {
            assert!(hit(key));
            assert!(hit(&format!("export KEY=\"{key}\"\n")));
            assert!(hit(&format!("Authorization: Bearer {key}")));
        }
    }

    #[test]
    fn api_key_negative() {
        let texts = [
            "risk-free sk-learn task-runner".to_string(),
            format!("{}{}", concat!("s", "k-"), "a".repeat(40)),
            format!("{}{}", concat!("gh", "p_"), body(20)),
            format!("{}{}", concat!("gh", "p_"), body(40)),
            format!("{}{}", concat!("AK", "IA"), upper_body(15)),
            format!("{}{}", concat!("AK", "IA"), body(16)),
            format!("{}{}", concat!("AK", "IA"), upper_body(17)),
            format!("{}{}", concat!("AI", "za"), body(34)),
            format!("{}{}", concat!("gl", "pat-"), body(10)),
            format!("{}{}", concat!("xo", "xb-"), "hello"),
            format!("mask{}{}", concat!("s", "k-"), body(48)),
            "github_pat_".to_string(),
        ];
        for t in &texts {
            assert!(!hit(t));
        }
    }

    #[test]
    fn api_key_span_is_token() {
        let key = format!("{}{}", concat!("gh", "p_"), body(36));
        let text = format!("a {key} b");
        let mut out = Vec::new();
        super::Detector::scan(&super::ApiKeyDetector, &text, &mut out);
        assert_eq!(out.len(), 1);
        assert_eq!((out[0].start, out[0].end), (2, 2 + key.len()));
    }
}
