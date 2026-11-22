//! PEM private keys: a `-----BEGIN <LABEL>-----` line whose label ends in `PRIVATE KEY`
//! (or `PRIVATE KEY BLOCK` for PGP).
//!
//! Known misses: raw key material without armor (e.g. bare base64 or JWK JSON), PuTTY `.ppk`.

use super::{Detector, Hit};
use crate::model::SensitiveCategory;

pub(crate) struct PrivateKeyDetector;

const BEGIN: &str = "-----BEGIN ";
const DASHES: &[u8] = b"-----";
/// Longest armor label considered (real ones are under 30 bytes).
const MAX_LABEL: usize = 64;

/// Length of the label after `BEGIN ` if it is followed by `-----` and names a private key.
fn private_label_len(rest: &[u8]) -> Option<usize> {
    let len = rest
        .iter()
        .take(MAX_LABEL + 1)
        .position(|&b| !(b.is_ascii_uppercase() || b == b' '))?;
    let label = rest.get(..len)?;
    let names_private_key =
        label.ends_with(b"PRIVATE KEY") || label.ends_with(b"PRIVATE KEY BLOCK");
    (names_private_key && rest.get(len..)?.starts_with(DASHES)).then_some(len)
}

impl Detector for PrivateKeyDetector {
    /// Non-overlapping substring search for `-----BEGIN ` (linear), then a bounded label check.
    /// The hit covers the BEGIN line only, so no unbounded search for the END line is needed.
    fn scan(&self, text: &str, out: &mut Vec<Hit>) {
        let bytes = text.as_bytes();
        for (start, _) in text.match_indices(BEGIN) {
            let label_start = start + BEGIN.len();
            let Some(rest) = bytes.get(label_start..) else {
                continue;
            };
            if let Some(len) = private_label_len(rest) {
                out.push(Hit {
                    category: SensitiveCategory::PrivateKey,
                    start,
                    end: label_start + len + DASHES.len(),
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
