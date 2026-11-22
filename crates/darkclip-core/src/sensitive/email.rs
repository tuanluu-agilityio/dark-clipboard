//! Email addresses (off by default).
//!
//! Known misses: quoted local parts, internationalized domains, IP-literal domains.

use super::{Detector, Hit};
use crate::model::SensitiveCategory;

pub(crate) struct EmailDetector;

const MAX_LOCAL: usize = 64;
const MAX_DOMAIN: usize = 253;
const MAX_LABEL: usize = 63;
/// File extensions that look like TLDs in names such as `icon@2x.png`.
const FILE_EXTENSIONS: &[&[u8]] = &[
    b"png", b"jpg", b"jpeg", b"gif", b"svg", b"webp", b"bmp", b"ico", b"tif", b"tiff", b"pdf",
    b"avif", b"heic",
];

fn is_local_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'%' | b'+' | b'-')
}

fn is_domain_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-')
}

/// `2x`, `3x`: retina scale suffixes.
fn is_scale_label(label: &[u8]) -> bool {
    label.len() >= 2
        && label.last() == Some(&b'x')
        && label
            .get(..label.len() - 1)
            .is_some_and(|d| d.iter().all(u8::is_ascii_digit))
}

fn valid_domain(domain: &[u8]) -> bool {
    let mut count = 0;
    let mut first: &[u8] = &[];
    let mut last: &[u8] = &[];
    for label in domain.split(|&b| b == b'.') {
        if label.is_empty()
            || label.len() > MAX_LABEL
            || label.first() == Some(&b'-')
            || label.last() == Some(&b'-')
        {
            return false;
        }
        if count == 0 {
            first = label;
        }
        last = label;
        count += 1;
    }
    let tld_lower: Vec<u8> = last.to_ascii_lowercase();
    count >= 2
        && last.len() >= 2
        && last.iter().all(u8::is_ascii_alphabetic)
        && !FILE_EXTENSIONS.contains(&tld_lower.as_slice())
        && !is_scale_label(first)
}

impl Detector for EmailDetector {
    /// For each `@`: a backward scan bounded by 65 bytes and a forward scan that stops at the
    /// next non-domain byte (so also at the next `@`). Total work is linear.
    fn scan(&self, text: &str, out: &mut Vec<Hit>) {
        let bytes = text.as_bytes();
        for (at, _) in text.match_indices('@') {
            let mut local_start = at;
            while local_start > 0
                && at - local_start <= MAX_LOCAL
                && bytes
                    .get(local_start - 1)
                    .is_some_and(|&b| is_local_byte(b))
            {
                local_start -= 1;
            }
            let Some(local) = bytes.get(local_start..at) else {
                continue;
            };
            if local.is_empty()
                || local.len() > MAX_LOCAL
                || local.first() == Some(&b'.')
                || local.last() == Some(&b'.')
            {
                continue;
            }
            let mut end = at + 1;
            while end - at <= MAX_DOMAIN + 1 && bytes.get(end).is_some_and(|&b| is_domain_byte(b)) {
                end += 1;
            }
            // A sentence may end right after the address.
            while end > at + 1 && bytes.get(end - 1) == Some(&b'.') {
                end -= 1;
            }
            let Some(domain) = bytes.get(at + 1..end) else {
                continue;
            };
            if domain.len() <= MAX_DOMAIN && valid_domain(domain) {
                out.push(Hit {
                    category: SensitiveCategory::Email,
                    start: local_start,
                    end,
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
        only(s, C::Email) == vec![C::Email]
    }

    #[test]
    fn email_positive() {
        assert!(hit(concat!("some", "one@example.com")));
        assert!(hit(concat!("mail: first.last+tag", "@mail.example.co.uk.")));
        assert!(hit(concat!("<a_b-c", "@sub-domain.example.org>")));
    }

    #[test]
    fn email_negative() {
        let texts = [
            "@mention",
            "x@2x.png",
            "icon@3x.jpg",
            "logo@retina.svg",
            "user@localhost",
            "a@b.c",
            ".a@example.com",
            "a.@example.com",
            "a@-example.com",
            "a@example.123",
            "npm i @types/node",
        ];
        for t in texts {
            assert!(!hit(t));
        }
    }
}
