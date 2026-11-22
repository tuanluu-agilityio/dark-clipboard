//! JSON Web Tokens: `header.payload[.signature]` where the header base64url-decodes
//! to a JSON object with an `"alg"` member.
//!
//! Known misses: opaque bearer tokens, JWE with unusual headers, tokens split across lines.

use super::{is_alnum, wipe_bytes, Detector, Hit};
use crate::model::SensitiveCategory;

pub(crate) struct JwtDetector;

/// Real JOSE headers are tiny; longer first segments are not decoded.
const MAX_HEADER_LEN: usize = 4096;

fn is_b64url_byte(b: u8) -> bool {
    is_alnum(b) || b == b'_' || b == b'-'
}

fn is_token_byte(b: u8) -> bool {
    is_b64url_byte(b) || b == b'.'
}

/// `{` (after optional whitespace) and an `"alg"` member name somewhere in the object.
fn is_jose_header(decoded: &[u8]) -> bool {
    let trimmed = decoded
        .iter()
        .position(|b| !b.is_ascii_whitespace())
        .and_then(|p| decoded.get(p..))
        .unwrap_or(&[]);
    trimmed.first() == Some(&b'{') && trimmed.windows(5).any(|w| w == b"\"alg\"")
}

fn is_jwt(run: &[u8]) -> bool {
    // A sentence may end right after the token; an unsigned JWT also ends in '.'.
    let end = run.iter().rposition(|&b| b != b'.').map_or(0, |p| p + 1);
    let core = run.get(..end).unwrap_or(&[]);
    let mut parts = core.split(|&b| b == b'.');
    let (Some(header), Some(payload)) = (parts.next(), parts.next()) else {
        return false;
    };
    let signature = parts.next();
    if parts.next().is_some()
        || header.len() > MAX_HEADER_LEN
        || !header.starts_with(b"eyJ")
        || payload.is_empty()
        || signature.is_some_and(<[u8]>::is_empty)
    {
        return false;
    }
    let Some(mut h) = decode_base64url(header) else {
        return false;
    };
    let header_ok = is_jose_header(&h);
    wipe_bytes(&mut h);
    if !header_ok {
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

impl Detector for JwtDetector {
    /// One pass over maximal runs of `[A-Za-z0-9_.-]`; each run is checked once.
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
            if bytes.get(start..i).is_some_and(is_jwt) {
                out.push(Hit {
                    category: SensitiveCategory::Token,
                    start,
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
