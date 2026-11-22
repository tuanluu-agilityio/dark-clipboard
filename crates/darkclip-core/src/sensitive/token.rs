//! JSON Web Tokens in compact form:
//! - JWS `header.payload[.signature]`: the header base64url-decodes to a JSON object with an
//!   `"alg"` member and the payload decodes;
//! - JWE `header.key.iv.ciphertext.tag`: the header decodes to a JSON object with `"enc"`.
//!
//! A token may follow other dot-separated segments in the same run (`id.eyJ…`); the starts of
//! the last five segments are tried.
//!
//! Known misses: opaque bearer tokens, headers that do not start with `{"` (`eyJ`), tokens
//! glued to preceding characters inside a segment, tokens split across lines.

use super::{is_alnum, wipe_bytes, Detector, Hit};
use crate::model::SensitiveCategory;

pub(crate) struct JwtDetector;

/// Real JOSE headers are tiny; longer first segments are not decoded.
const MAX_HEADER_LEN: usize = 4096;
/// A JWE compact token has five segments, the most of any form.
const MAX_SEGMENTS: usize = 5;

fn is_b64url_byte(b: u8) -> bool {
    is_alnum(b) || b == b'_' || b == b'-'
}

fn is_token_byte(b: u8) -> bool {
    is_b64url_byte(b) || b == b'.'
}

/// `{` (after optional whitespace) and the quoted `member` name somewhere in the object.
fn is_json_object_with(decoded: &[u8], member: &[u8]) -> bool {
    let trimmed = decoded
        .iter()
        .position(|b| !b.is_ascii_whitespace())
        .and_then(|p| decoded.get(p..))
        .unwrap_or(&[]);
    trimmed.first() == Some(&b'{')
        && trimmed.windows(member.len() + 2).any(|w| {
            w.first() == Some(&b'"')
                && w.last() == Some(&b'"')
                && w.get(1..=member.len()) == Some(member)
        })
}

/// Decodes `header` and checks it for `member`; the decoded bytes are wiped.
fn header_has(header: &[u8], member: &[u8]) -> bool {
    if header.len() > MAX_HEADER_LEN || !header.starts_with(b"eyJ") {
        return false;
    }
    let Some(mut h) = decode_base64url(header) else {
        return false;
    };
    let ok = is_json_object_with(&h, member);
    wipe_bytes(&mut h);
    ok
}

/// Base64url segment of a possible length (`4n + 1` is impossible).
fn valid_len(seg: &[u8]) -> bool {
    seg.len() % 4 != 1
}

/// `core` is one candidate token without trailing dots.
fn is_compact_token(core: &[u8]) -> bool {
    let mut parts: [&[u8]; MAX_SEGMENTS] = [&[]; MAX_SEGMENTS];
    let mut count = 0;
    for seg in core.split(|&b| b == b'.') {
        match parts.get_mut(count) {
            Some(slot) => *slot = seg,
            None => return false,
        }
        count += 1;
    }
    match (count, parts) {
        (2 | 3, [header, payload, signature, _, _]) => {
            if payload.is_empty() || (count == 3 && signature.is_empty()) {
                return false;
            }
            if !header_has(header, b"alg") {
                return false;
            }
            match decode_base64url(payload) {
                Some(mut p) => {
                    wipe_bytes(&mut p);
                    true
                }
                None => false,
            }
        }
        (5, [header, key, iv, ciphertext, tag]) => {
            // The encrypted key is empty for direct encryption.
            valid_len(key)
                && [iv, ciphertext, tag]
                    .iter()
                    .all(|s| !s.is_empty() && valid_len(s))
                && header_has(header, b"enc")
        }
        _ => false,
    }
}

/// Byte offset (into `run`) of a compact token ending the run, if any. Tries the starts of
/// the last [`MAX_SEGMENTS`] segments, leftmost first: at most five linear checks.
fn token_start(run: &[u8]) -> Option<usize> {
    // A sentence may end right after the token; an unsigned JWT also ends in '.'.
    let end = run.iter().rposition(|&b| b != b'.').map_or(0, |p| p + 1);
    let core = run.get(..end)?;
    let mut starts = [0usize; MAX_SEGMENTS];
    let mut n = 0;
    let mut pos = end;
    while n < MAX_SEGMENTS {
        let seg_start = core
            .get(..pos)?
            .iter()
            .rposition(|&b| b == b'.')
            .map_or(0, |p| p + 1);
        if let Some(slot) = starts.get_mut(n) {
            *slot = seg_start;
        }
        n += 1;
        if seg_start == 0 {
            break;
        }
        pos = seg_start - 1;
    }
    starts.get(..n)?.iter().rev().copied().find(|&s| {
        core.get(s..)
            .is_some_and(|c| c.starts_with(b"eyJ") && is_compact_token(c))
    })
}

impl Detector for JwtDetector {
    /// One pass over maximal runs of `[A-Za-z0-9_.-]`; each run is checked a bounded number
    /// of times.
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
            if let Some(offset) = bytes.get(start..i).and_then(token_start) {
                out.push(Hit {
                    category: SensitiveCategory::Token,
                    start: start + offset,
                    end: i,
                });
            }
        }
    }
}

fn sextet(b: u8) -> Option<u32> {
    let v = match b {
        b'A'..=b'Z' => b - b'A',
        b'a'..=b'z' => b - b'a' + 26,
        b'0'..=b'9' => b - b'0' + 52,
        b'-' => 62,
        b'_' => 63,
        _ => return None,
    };
    Some(u32::from(v))
}

/// Decodes unpadded base64url (RFC 4648 §5, as used by RFC 7515).
/// `None` on any invalid character (including `=`) or a length of `4n + 1`.
pub(crate) fn decode_base64url(s: &[u8]) -> Option<Vec<u8>> {
    if s.len() % 4 == 1 {
        return None;
    }
    let mut out = Vec::with_capacity(s.len() / 4 * 3 + 2);
    let mut acc: u32 = 0;
    let mut bits = 0u32;
    for &b in s {
        let Some(v) = sextet(b) else {
            wipe_bytes(&mut out);
            return None;
        };
        acc = (acc << 6 | v) & 0xFFFF;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push(((acc >> bits) & 0xFF) as u8);
        }
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::SensitiveCategory as C;
    use crate::sensitive::testutil::*;

    fn hit(s: &str) -> bool {
        only(s, C::Token) == vec![C::Token]
    }

    fn jwt() -> String {
        format!(
            "{}.{}.{}",
            concat!("eyJhbGciOi", "JIUzI1NiJ9"),
            concat!("eyJzdWIiOiIxMjM0", "NTY3ODkwIn0"),
            body(43)
        )
    }

    #[test]
    fn jwt_requires_decodable_header() {
        let tok = jwt();
        assert!(hit(&tok));
        assert!(hit(&format!("Authorization: Bearer {tok}")));
        assert!(hit(&format!("token={tok}.")));
        // unsigned JWT
        assert!(hit(concat!(
            "eyJhbGciOi",
            "Jub25lIn0",
            ".",
            "eyJzdWIiOiIxIn0",
            "."
        )));

        let garbage = [
            format!("eyJ{}.{}.{}", "!!!!", body(10), body(10)),
            format!("eyJ{}.{}.{}", "zzzz", body(10), body(10)),
            format!("eyJ{}.{}.{}", body(17), body(10), body(10)),
            // decodes, but is not an object with "alg"
            format!("{}.{}.{}", "eyJmb28iOiJiYXIifQ", body(10), body(10)),
            format!("{}.", concat!("eyJhbGciOi", "JIUzI1NiJ9")),
            concat!("eyJhbGciOi", "JIUzI1NiJ9").to_string(),
            "eyJ.eyJ.eyJ".to_string(),
        ];
        for g in &garbage {
            assert!(!hit(g));
        }
    }

    #[test]
    fn jwt_after_other_segments() {
        assert!(hit(&format!("id.{}", jwt())));
        assert!(hit(&format!("a.b.c.{}", jwt())));
        let mut out = Vec::new();
        let text = format!("x id.{}", jwt());
        super::Detector::scan(&JwtDetector, &text, &mut out);
        assert_eq!(out.len(), 1);
        assert_eq!((out[0].start, out[0].end), (5, text.len()));
    }

    #[test]
    fn jwe_compact_with_enc_header() {
        let header = concat!("eyJhbGciOiJkaXIiLCJl", "bmMiOiJBMjU2R0NNIn0");
        assert!(hit(&format!(
            "{header}..{}.{}.{}",
            body(16),
            body(40),
            body(22)
        )));
        assert!(hit(&format!(
            "{header}.{}.{}.{}.{}",
            body(43),
            body(16),
            body(40),
            body(22)
        )));
        // 5 parts but no "enc" member
        assert!(!hit(&format!(
            "{}.{}.{}.{}.{}",
            "eyJ6aXAiOiJERUYifQ",
            body(43),
            body(16),
            body(40),
            body(22)
        )));
        // 4 parts
        assert!(!hit(&format!(
            "{header}.{}.{}.{}",
            body(16),
            body(40),
            body(22)
        )));
    }

    #[test]
    fn base64url_decoding() {
        assert_eq!(decode_base64url(b"").unwrap(), b"");
        assert_eq!(decode_base64url(b"Zg").unwrap(), b"f");
        assert_eq!(decode_base64url(b"Zm8").unwrap(), b"fo");
        assert_eq!(decode_base64url(b"Zm9v").unwrap(), b"foo");
        assert_eq!(decode_base64url(b"-_8").unwrap(), vec![0xfb, 0xff]);
        assert_eq!(
            decode_base64url(concat!("eyJhbGciOi", "JIUzI1NiJ9").as_bytes()).unwrap(),
            br#"{"alg":"HS256"}"#
        );
        assert!(decode_base64url(b"Z").is_none());
        assert!(decode_base64url(b"Zm9v=").is_none());
        assert!(decode_base64url(b"Zm+v").is_none());
    }
}
