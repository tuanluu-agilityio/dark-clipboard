//! Timestamps (Unix ms, UTC) and a hand-written RFC 3339 conversion.
//! The core never reads the OS clock; time arrives through [`Clock`].

use crate::error::CoreError;
use serde::{de, Deserialize, Deserializer, Serialize, Serializer};

const MS_PER_DAY: i64 = 86_400_000;

/// Unix milliseconds, UTC.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Timestamp(pub i64);

/// Source of the current time, injected so the core stays pure.
pub trait Clock: Send + Sync {
    fn now(&self) -> Timestamp;
}

/// Days since 1970-01-01 to (year, month, day). Hinnant's civil_from_days.
fn civil_from_days(days: i64) -> (i64, i64, i64) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (if m <= 2 { y + 1 } else { y }, m, d)
}

/// (year, month, day) to days since 1970-01-01. Hinnant's days_from_civil.
fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = if m > 2 { m - 3 } else { m + 9 };
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

fn is_leap(y: i64) -> bool {
    (y % 4 == 0 && y % 100 != 0) || y % 400 == 0
}

fn days_in_month(y: i64, m: i64) -> i64 {
    match m {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        _ if is_leap(y) => 29,
        _ => 28,
    }
}

/// Parses exactly `len` ASCII digits.
fn digits(s: &str, len: usize) -> Result<i64, CoreError> {
    if s.len() != len || !s.bytes().all(|b| b.is_ascii_digit()) {
        return Err(CoreError::InvalidTimestamp);
    }
    s.parse::<i64>().map_err(|_| CoreError::InvalidTimestamp)
}

impl Timestamp {
    /// Formats as `YYYY-MM-DDTHH:MM:SS.mmmZ`.
    pub fn to_rfc3339(self) -> String {
        let days = self.0.div_euclid(MS_PER_DAY);
        let ms_of_day = self.0.rem_euclid(MS_PER_DAY);
        let (y, mo, d) = civil_from_days(days);
        let h = ms_of_day / 3_600_000;
        let mi = ms_of_day % 3_600_000 / 60_000;
        let s = ms_of_day % 60_000 / 1000;
        let ms = ms_of_day % 1000;
        format!("{y:04}-{mo:02}-{d:02}T{h:02}:{mi:02}:{s:02}.{ms:03}Z")
    }

    /// Parses `YYYY-MM-DDTHH:MM:SS[.f{1,9}]Z` (UTC only). Extra fraction digits are truncated.
    pub fn parse_rfc3339(s: &str) -> Result<Self, CoreError> {
        let body = s.strip_suffix('Z').ok_or(CoreError::InvalidTimestamp)?;
        let (date, time) = body.split_once('T').ok_or(CoreError::InvalidTimestamp)?;
        let mut dp = date.split('-');
        let (Some(ys), Some(ms_), Some(ds), None) = (dp.next(), dp.next(), dp.next(), dp.next())
        else {
            return Err(CoreError::InvalidTimestamp);
        };
        let (hms, frac) = match time.split_once('.') {
            Some((a, b)) => (a, Some(b)),
            None => (time, None),
        };
        let mut tp = hms.split(':');
        let (Some(hs), Some(is), Some(ss), None) = (tp.next(), tp.next(), tp.next(), tp.next())
        else {
            return Err(CoreError::InvalidTimestamp);
        };
        let (y, mo, d) = (digits(ys, 4)?, digits(ms_, 2)?, digits(ds, 2)?);
        let (h, mi, sec) = (digits(hs, 2)?, digits(is, 2)?, digits(ss, 2)?);
        if !(1..=12).contains(&mo) || d < 1 || d > days_in_month(y, mo) {
            return Err(CoreError::InvalidTimestamp);
        }
        if h > 23 || mi > 59 || sec > 59 {
            return Err(CoreError::InvalidTimestamp);
        }
        let millis = match frac {
            None => 0,
            Some(f) => {
                if f.is_empty() || f.len() > 9 || !f.bytes().all(|b| b.is_ascii_digit()) {
                    return Err(CoreError::InvalidTimestamp);
                }
                let mut v = 0i64;
                for i in 0..3 {
                    let digit = f.as_bytes().get(i).map_or(0, |b| i64::from(b - b'0'));
                    v = v * 10 + digit;
                }
                v
            }
        };
        let days = days_from_civil(y, mo, d);
        Ok(Timestamp(
            days * MS_PER_DAY + ((h * 60 + mi) * 60 + sec) * 1000 + millis,
        ))
    }
}

impl Serialize for Timestamp {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&self.to_rfc3339())
    }
}

impl<'de> Deserialize<'de> for Timestamp {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let s = String::deserialize(d)?;
        Timestamp::parse_rfc3339(&s).map_err(de::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn timestamp_rfc3339_roundtrip() {
        let s = "2026-11-22T04:01:44.903Z";
        let t = Timestamp::parse_rfc3339(s).unwrap();
        assert_eq!(t.to_rfc3339(), s);
        assert_eq!(t, Timestamp(1_795_320_104_903));
    }

    #[test]
    fn known_values() {
        assert_eq!(Timestamp(0).to_rfc3339(), "1970-01-01T00:00:00.000Z");
        assert_eq!(
            Timestamp::parse_rfc3339("2000-02-29T23:59:59.999Z").unwrap(),
            Timestamp(951_868_799_999)
        );
        assert_eq!(Timestamp(-1).to_rfc3339(), "1969-12-31T23:59:59.999Z");
    }

    #[test]
    fn examples_roundtrip() {
        for s in [
            "2026-11-22T05:20:00.000Z",
            "2026-11-22T05:32:00.000Z",
            "2026-11-22T03:15:07.120Z",
            "2026-11-22T08:30:00.000Z",
            "2024-02-29T00:00:00.000Z",
        ] {
            assert_eq!(Timestamp::parse_rfc3339(s).unwrap().to_rfc3339(), s);
        }
    }

    #[test]
    fn civil_roundtrip_wide_range() {
        for days in (-800_000i64..800_000).step_by(37) {
            let (y, m, d) = civil_from_days(days);
            assert_eq!(days_from_civil(y, m, d), days);
        }
    }

    #[test]
    fn rejects_bad_input() {
        for s in [
            "",
            "2026-11-22",
            "2026-11-22T04:01:44.903",
            "2026-11-22T04:01:44.903+01:00",
            "2026-13-22T04:01:44Z",
            "2026-02-29T04:01:44Z",
            "2026-11-22T24:01:44Z",
            "2026-11-22T04:60:44Z",
            "2026-11-22T04:01:44.Z",
            "2026-11-2T04:01:44Z",
            "+026-11-22T04:01:44Z",
        ] {
            assert_eq!(
                Timestamp::parse_rfc3339(s),
                Err(CoreError::InvalidTimestamp),
                "{s}"
            );
        }
    }

    #[test]
    fn fraction_forms() {
        let base = Timestamp::parse_rfc3339("2026-11-22T04:01:44Z").unwrap();
        assert_eq!(
            Timestamp::parse_rfc3339("2026-11-22T04:01:44.5Z")
                .unwrap()
                .0,
            base.0 + 500
        );
        assert_eq!(
            Timestamp::parse_rfc3339("2026-11-22T04:01:44.123456Z")
                .unwrap()
                .0,
            base.0 + 123
        );
    }
}
