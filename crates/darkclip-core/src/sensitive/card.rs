//! Payment cards: 13 to 19 digits, optionally grouped by single spaces or by single dashes,
//! passing the Luhn check, first digit 2 to 6.
//!
//! Known misses: cards starting with other digits, numbers with mixed or doubled separators.

use super::{boundary_after, boundary_before, wipe_slice, Detector, Hit};
use crate::model::SensitiveCategory;

pub(crate) struct CardDetector;

const MIN_DIGITS: usize = 13;
const MAX_DIGITS: usize = 19;

fn is_digit_at(bytes: &[u8], i: usize) -> bool {
    bytes.get(i).is_some_and(u8::is_ascii_digit)
}

/// A decimal or thousands mark glued to a digit (`1.4111…`, `…1111,5`) means a larger number.
fn number_continues_before(bytes: &[u8], start: usize) -> bool {
    start >= 2 && matches!(bytes.get(start - 1), Some(b'.' | b',')) && is_digit_at(bytes, start - 2)
}

fn number_continues_after(bytes: &[u8], end: usize) -> bool {
    matches!(bytes.get(end), Some(b'.' | b',')) && is_digit_at(bytes, end + 1)
}

fn plausible(digits: &[u8]) -> bool {
    let Some(&first) = digits.first() else {
        return false;
    };
    (MIN_DIGITS..=MAX_DIGITS).contains(&digits.len())
        && (2..=6).contains(&first)
        && digits.iter().any(|&d| d != first)
        && luhn(digits)
}

impl Detector for CardDetector {
    /// One pass: each maximal digit group run (single `' '` or single `'-'` between groups,
    /// the same separator throughout) is examined once, then skipped.
    fn scan(&self, text: &str, out: &mut Vec<Hit>) {
        let bytes = text.as_bytes();
        let mut digits = [0u8; MAX_DIGITS];
        let mut i = 0;
        while i < bytes.len() {
            if !is_digit_at(bytes, i) {
                i += 1;
                continue;
            }
            let start = i;
            let mut count = 0usize;
            let mut sep: Option<u8> = None;
            loop {
                while let Some(&b) = bytes.get(i).filter(|b| b.is_ascii_digit()) {
                    if let Some(slot) = digits.get_mut(count) {
                        *slot = b - b'0';
                    }
                    count += 1;
                    i += 1;
                }
                match bytes.get(i) {
                    Some(&s @ (b' ' | b'-'))
                        if sep.is_none_or(|x| x == s) && is_digit_at(bytes, i + 1) =>
                    {
                        sep = Some(s);
                        i += 1;
                    }
                    _ => break,
                }
            }
            let end = i;
            let found = count <= MAX_DIGITS
                && boundary_before(bytes, start)
                && boundary_after(bytes, end)
                && !number_continues_before(bytes, start)
                && !number_continues_after(bytes, end)
                && digits.get(..count).is_some_and(plausible);
            if found {
                out.push(Hit {
                    category: SensitiveCategory::PaymentCard,
                    start,
                    end,
                });
            }
        }
        wipe_slice(&mut digits);
    }
}

/// Luhn checksum over digit values 0..=9.
pub(crate) fn luhn(digits: &[u8]) -> bool {
    let sum: u32 = digits
        .iter()
        .rev()
        .enumerate()
        .map(|(i, &d)| {
            let d = u32::from(d);
            if i % 2 == 1 {
                let x = d * 2;
                if x > 9 {
                    x - 9
                } else {
                    x
                }
            } else {
                d
            }
        })
        .sum();
    sum.is_multiple_of(10)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::SensitiveCategory as C;
    use crate::sensitive::testutil::*;

    fn hit(s: &str) -> bool {
        only(s, C::PaymentCard) == vec![C::PaymentCard]
    }

    #[test]
    fn luhn_card() {
        assert!(hit(concat!("4111 1111", " 1111 1111")));
        assert!(hit(concat!("3782 822463", " 10005")));
        assert!(hit(concat!("41111111", "11111111")));
        assert!(hit(concat!("4111-1111", "-1111-1111")));
        assert!(hit(concat!("card: 5555 5555", " 5555 4444, exp 12/30")));
    }

    #[test]
    fn non_luhn_16_digits_negative() {
        let texts = [
            "4111 1111 1111 1112",
            "4111111111111112",
            "411111111111",
            "41111111111111111111",
            "4111 1111-1111 1111",
            "4111  1111 1111 1111",
            "x4111111111111111",
            "4111111111111111x",
            "0000000000000000",
            "4111.1111.1111.1111",
        ];
        for t in texts {
            assert!(!hit(t));
        }
    }

    #[test]
    fn luhn_values() {
        let d = |s: &str| s.bytes().map(|b| b - b'0').collect::<Vec<u8>>();
        assert!(luhn(&d("79927398713")));
        assert!(!luhn(&d("79927398710")));
        assert!(luhn(&d("4111111111111111")));
    }
}
