//! Armored private keys:
//! - PEM / OpenSSH / PGP: a `-----BEGIN <LABEL>-----` line whose label ends in `PRIVATE KEY`
//!   (or `PRIVATE KEY BLOCK` for PGP);
//! - RFC 4716 (SSH2): `---- BEGIN <LABEL> ----`, e.g. `SSH2 ENCRYPTED PRIVATE KEY`.
//!
//! Known misses: raw key material without armor (e.g. bare base64 or JWK JSON), PuTTY `.ppk`,
//! armor lines with lowercase or extra punctuation.

use super::{Detector, Hit};
use crate::model::SensitiveCategory;

pub(crate) struct PrivateKeyDetector;

/// Longest armor label considered (real ones are under 30 bytes).
const MAX_LABEL: usize = 64;

struct Armor {
    begin: &'static str,
    /// Bytes that must follow the label.
    close: &'static [u8],
}

const ARMORS: [Armor; 2] = [
    Armor {
        begin: "-----BEGIN ",
        close: b"-----",
    },
    Armor {
        begin: "---- BEGIN ",
        close: b" ----",
    },
];

/// Length of the label after `BEGIN ` if `close` follows it and it names a private key.
fn private_label_len(rest: &[u8], close: &[u8]) -> Option<usize> {
    let run = rest
        .iter()
        .take(MAX_LABEL + 1)
        .position(|&b| !(b.is_ascii_uppercase() || b.is_ascii_digit() || b == b' '))?;
    // `close` may start with a space that the label run swallowed.
    let len = (0..=run)
        .rev()
        .take(2)
        .find(|&l| rest.get(l..).is_some_and(|r| r.starts_with(close)))?;
    let label = rest.get(..len)?;
    (label.ends_with(b"PRIVATE KEY") || label.ends_with(b"PRIVATE KEY BLOCK")).then_some(len)
}

impl Detector for PrivateKeyDetector {
    /// Non-overlapping substring search for each armor's BEGIN marker (linear), then a bounded
    /// label check. The hit covers the BEGIN line only, so no unbounded search for the END
    /// line is needed.
    fn scan(&self, text: &str, out: &mut Vec<Hit>) {
        let bytes = text.as_bytes();
        for armor in &ARMORS {
            for (start, _) in text.match_indices(armor.begin) {
                let label_start = start + armor.begin.len();
                let Some(rest) = bytes.get(label_start..) else {
                    continue;
                };
                if let Some(len) = private_label_len(rest, armor.close) {
                    out.push(Hit {
                        category: SensitiveCategory::PrivateKey,
                        start,
                        end: label_start + len + armor.close.len(),
                    });
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
        only(s, C::PrivateKey) == vec![C::PrivateKey]
    }

    fn armor(label: &str) -> String {
        format!(
            "{d}BEGIN {label}{d}\n{}\n{d}END {label}{d}\n",
            body(64),
            d = "-----"
        )
    }

    #[test]
    fn pem_private_key() {
        assert!(hit(&armor(concat!("OPENSSH ", "PRIVATE KEY"))));
        assert!(hit(&armor(concat!("RSA ", "PRIVATE KEY"))));
        assert!(hit(&armor(concat!("EC ", "PRIVATE KEY"))));
        assert!(hit(&armor(concat!("ENCRYPTED ", "PRIVATE KEY"))));
        assert!(hit(&armor(concat!("", "PRIVATE KEY"))));
        assert!(hit(&armor(concat!("PGP ", "PRIVATE KEY BLOCK"))));
        assert!(hit(&format!(
            "key:\n  {}",
            armor(concat!("OPENSSH ", "PRIVATE KEY"))
        )));
    }

    #[test]
    fn rfc4716_ssh2_private_key() {
        assert!(hit(concat!(
            "---- BEGIN SSH2 ENCRYPTED ",
            "PRIVATE KEY ----\nabc\n"
        )));
        assert!(!hit("---- BEGIN SSH2 PUBLIC KEY ----\nabc\n"));
        assert!(!hit("---- BEGIN SSH2 ENCRYPTED PRIVATE KEY"));
    }

    #[test]
    fn pem_public_key_is_not_sensitive() {
        assert!(!hit(&armor("PUBLIC KEY")));
        assert!(!hit(&armor("RSA PUBLIC KEY")));
        assert!(!hit(&armor("PGP PUBLIC KEY BLOCK")));
        assert!(!hit(&armor("CERTIFICATE")));
        assert!(!hit("-----BEGIN PRIVATE KEY"));
        assert!(!hit("the words private key in a sentence"));
        assert!(!hit("-----BEGIN NOT A PRIVATE KEY HOLDER-----"));
    }
}
