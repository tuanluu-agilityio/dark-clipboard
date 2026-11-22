//! Payment cards: 13 to 19 digits, optionally grouped by one kind of single separator
//! (space, dash, no-break space or another Unicode space) used throughout, passing the Luhn
//! check, first digit 2 to 6. Group-aligned prefixes of a longer run are also tried, so a
//! card followed by an expiry date or CVV is found.
//!
//! Known misses: cards starting with other digits, numbers with mixed or doubled separators,
//! groups split at positions that do not align with the card number.

use super::{boundary_after, boundary_before, wipe_slice, Detector, Hit};
use crate::model::SensitiveCategory;

pub(crate) struct CardDetector;

const MIN_DIGITS: usize = 13;
const MAX_DIGITS: usize = 19;

fn is_digit_at(bytes: &[u8], i: usize) -> bool {
    bytes.get(i).is_some_and(u8::is_ascii_digit)
}

/// Group separator starting at byte `i`, if any.
fn separator_at(text: &str, i: usize) -> Option<char> {
    let c = text.get(i..)?.chars().next()?;
    matches!(
        c,
        ' ' | '-' | '\u{a0}' | '\u{2000}'..='\u{200a}' | '\u{202f}'
    )
    .then_some(c)
}

/// Exactly three digits at `i`, not followed by a fourth: a thousands group.
fn thousands_group_at(bytes: &[u8], i: usize) -> bool {
    (i..i + 3).all(|k| is_digit_at(bytes, k)) && !is_digit_at(bytes, i + 3)
}

/// A decimal point glued to digits, or a thousands comma, means a larger number.
fn number_continues_before(bytes: &[u8], start: usize, first_group_len: usize) -> bool {
    start >= 2
        && is_digit_at(bytes, start - 2)
        && match bytes.get(start - 1) {
            Some(b'.') => true,
            Some(b',') => first_group_len == 3,
            _ => false,
        }
}

fn number_continues_after(bytes: &[u8], end: usize) -> bool {
    match bytes.get(end) {
        Some(b'.') => is_digit_at(bytes, end + 1),
        Some(b',') => thousands_group_at(bytes, end + 1),
        _ => false,
    }
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
    /// One pass: each maximal digit-group run is read once, then skipped. Prefixes ending at a
    /// group boundary with 13 to 19 digits are checked; there are at most 7 per run.
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
            let mut first_group_len = None;
            let mut sep: Option<char> = None;
            // Latest passing prefix that is followed by a separator and more digits.
            let mut inner_end: Option<usize> = None;
            let final_passes = loop {
                while let Some(&b) = bytes.get(i).filter(|b| b.is_ascii_digit()) {
                    if let Some(slot) = digits.get_mut(count) {
                        *slot = b - b'0';
                    }
                    count += 1;
                    i += 1;
                }
                first_group_len.get_or_insert(count);
                let passes = count <= MAX_DIGITS && digits.get(..count).is_some_and(plausible);
                match separator_at(text, i) {
                    Some(c)
                        if sep.is_none_or(|x| x == c) && is_digit_at(bytes, i + c.len_utf8()) =>
                    {
                        if passes {
                            inner_end = Some(i);
                        }
                        sep = Some(c);
                        i += c.len_utf8();
                    }
                    _ => break passes,
                }
            };
            let end = i;
            let start_ok = boundary_before(bytes, start)
                && !number_continues_before(bytes, start, first_group_len.unwrap_or(0));
            let hit_end = if final_passes
                && boundary_after(bytes, end)
                && !number_continues_after(bytes, end)
            {
                Some(end)
            } else {
                inner_end
            };
            if let (true, Some(end)) = (start_ok, hit_end) {
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
    fn card_followed_by_expiry_or_cvv() {
        assert!(hit(concat!("4111 1111 1111", " 1111 12/30 123")));
        assert!(hit(concat!("4111 1111 1111", " 1111 123")));
        assert!(hit(concat!("41111111", "11111111,12/30,123")));
        assert!(!hit("1,234,567.89"));
        // a comma followed by exactly three digits continues a number
        assert!(!hit(concat!("41111111", "11111111,123")));
    }

    #[test]
    fn card_unicode_space_separators() {
        assert!(hit(concat!("4111\u{a0}1111", "\u{a0}1111\u{a0}1111")));
        assert!(hit(concat!("4111\u{2009}1111", "\u{2009}1111\u{2009}1111")));
        assert!(hit(concat!("4111\u{202f}1111", "\u{202f}1111\u{202f}1111")));
        assert!(!hit(concat!("4111\u{a0}1111", " 1111 1111")));
        assert!(!hit(concat!(
            "4111\u{2009}1111",
            "\u{2008}1111\u{2009}1111"
        )));
    }

    #[test]
    fn luhn_values() {
        let d = |s: &str| s.bytes().map(|b| b - b'0').collect::<Vec<u8>>();
        assert!(luhn(&d("79927398713")));
        assert!(!luhn(&d("79927398710")));
        assert!(luhn(&d("4111111111111111")));
    }
}
