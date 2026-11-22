//! Phone numbers (off by default): international `+` numbers with 8 to 15 digits,
//! and Vietnamese mobile numbers `0[35789]` + 8 digits.
//!
//! Known misses: national formats of other countries, numbers grouped with dots.

use super::{boundary_after, boundary_before, Detector, Hit};
use crate::model::SensitiveCategory;

pub(crate) struct PhoneDetector;

const INTL_MIN_DIGITS: usize = 8;
const INTL_MAX_DIGITS: usize = 15; // E.164
const VN_DIGITS: usize = 10;
/// At most this many separator bytes between two digit groups, e.g. `") "`.
const MAX_SEP_RUN: usize = 2;

fn is_digit_at(bytes: &[u8], i: usize) -> bool {
    bytes.get(i).is_some_and(u8::is_ascii_digit)
}

struct Run {
    end: usize,
    count: usize,
    first_two: [u8; 2],
}

/// Reads digit groups from `i`, allowing up to [`MAX_SEP_RUN`] bytes accepted by `is_sep`
/// between groups. Each byte is visited a bounded number of times.
fn read_run(bytes: &[u8], mut i: usize, is_sep: fn(u8) -> bool) -> Run {
    let mut count = 0;
    let mut first_two = [0u8; 2];
    loop {
        while let Some(&b) = bytes.get(i).filter(|b| b.is_ascii_digit()) {
            if let Some(slot) = first_two.get_mut(count) {
                *slot = b - b'0';
            }
            count += 1;
            i += 1;
        }
        let mut j = i;
        while j - i < MAX_SEP_RUN && bytes.get(j).is_some_and(|&b| is_sep(b)) {
            j += 1;
        }
        if j > i && is_digit_at(bytes, j) {
            i = j;
        } else {
            break;
        }
    }
    Run {
        end: i,
        count,
        first_two,
    }
}

fn intl_sep(b: u8) -> bool {
    matches!(b, b' ' | b'-' | b'(' | b')')
}

fn local_sep(b: u8) -> bool {
    matches!(b, b' ' | b'-')
}

impl Detector for PhoneDetector {
    /// One pass; every digit run is read once and then skipped.
    fn scan(&self, text: &str, out: &mut Vec<Hit>) {
        let bytes = text.as_bytes();
        let mut i = 0;
        while i < bytes.len() {
            let b = bytes.get(i).copied().unwrap_or(0);
            if b == b'+' && is_digit_at(bytes, i + 1) {
                let run = read_run(bytes, i + 1, intl_sep);
                if boundary_before(bytes, i)
                    && boundary_after(bytes, run.end)
                    && (INTL_MIN_DIGITS..=INTL_MAX_DIGITS).contains(&run.count)
                {
                    out.push(Hit {
                        category: SensitiveCategory::Phone,
                        start: i,
                        end: run.end,
                    });
                }
                i = run.end;
            } else if b.is_ascii_digit() {
                let run = read_run(bytes, i, local_sep);
                if boundary_before(bytes, i)
                    && boundary_after(bytes, run.end)
                    && run.count == VN_DIGITS
                    && run.first_two[0] == 0
                    && matches!(run.first_two[1], 3 | 5 | 7 | 8 | 9)
                {
                    out.push(Hit {
                        category: SensitiveCategory::Phone,
                        start: i,
                        end: run.end,
                    });
                }
                i = run.end;
            } else {
                i += 1;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::model::SensitiveCategory as C;
    use crate::sensitive::testutil::*;

    fn hit(s: &str) -> bool {
        only(s, C::Phone) == vec![C::Phone]
    }

    #[test]
    fn phone_positive() {
        assert!(hit(concat!("+84 912 ", "345 678")));
        assert!(hit(concat!("call +1-555-", "123-4567")));
        assert!(hit(concat!("+44 (20) 7946 ", "0958")));
        assert!(hit(concat!("0912", "345678")));
        assert!(hit(concat!("0912 345", " 678")));
    }

    #[test]
    fn phone_negative() {
        let texts = [
            "+1",
            "+123",
            "1+2345678901",
            "0123456789",
            "09123456789",
            "2026-11-22",
            "192.168.100.100",
            "1,234,567.89",
            "+1234567890123456",
            "v0912345678",
        ];
        for t in texts {
            assert!(!hit(t));
        }
    }
}
