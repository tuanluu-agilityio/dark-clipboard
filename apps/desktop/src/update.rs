//! The opt-in update check (Task 412, D-057): the ONLY network code in the project.
//!
//! One HTTPS GET to the GitHub "latest release" endpoint, only after the user pressed "Check
//! for updates" or turned on "Automatically check weekly" (off by default). It sends no clip
//! data, settings or identifiers, only a fixed `User-Agent`. The answer is capped and only
//! `tag_name` and `html_url` are read; the page opens only when `html_url` passes the allow
//! check. Nothing is downloaded or installed. Errors carry no content (GC-06).
//!
//! The request sits behind [`Fetch`], so every rule here is tested with a fake that counts
//! calls; no test touches the network. `crates/` stay network-free (checked by the gate).

#![cfg_attr(not(windows), allow(dead_code))]

use darkclip_app::settings::UpdateSettings;
use darkclip_core::Timestamp;
use std::time::Duration;

pub const LATEST_URL: &str =
    "https://api.github.com/repos/tuanluu-agilityio/dark-clipboard/releases/latest";
pub const RELEASE_URL_PREFIX: &str = "https://github.com/tuanluu-agilityio/dark-clipboard/releases/";
/// The most response bytes read (256 KiB).
pub const MAX_BODY_BYTES: usize = 256 * 1024;
pub const TIMEOUT: Duration = Duration::from_secs(10);
/// The weekly interval, in milliseconds.
pub const AUTO_INTERVAL_MS: i64 = 7 * 24 * 60 * 60 * 1000;

/// Why a check failed. No variant carries text from the network or the system.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UpdateError {
    Network,
    TooLarge,
    Malformed,
    BadUrl,
}

/// A release page address that passed the allow check.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReleaseUrl(String);

impl ReleaseUrl {
    pub fn parse(_s: &str) -> Option<ReleaseUrl> {
        unimplemented!()
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// A semantic version (`major.minor.patch` with an optional pre-release; build metadata is
/// ignored).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Version {
    major: u64,
}

impl Version {
    pub fn parse(_s: &str) -> Option<Version> {
        unimplemented!()
    }
}

impl PartialOrd for Version {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Version {
    fn cmp(&self, _other: &Self) -> std::cmp::Ordering {
        unimplemented!()
    }
}

/// What the response said: the newest release and where to read about it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Release {
    pub version: Version,
    /// The tag as shown to the user (`X.Y.Z`, no `v`).
    pub shown: String,
    pub url: ReleaseUrl,
}

pub fn parse_response(_body: &[u8]) -> Result<Release, UpdateError> {
    unimplemented!()
}

/// The result shown in About.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    UpToDate,
    Available { shown: String, url: ReleaseUrl },
    Failed,
}

/// The request. The real one is `UreqFetch`; tests use a fake.
pub trait Fetch {
    /// One GET of [`LATEST_URL`]; at most [`MAX_BODY_BYTES`] come back.
    fn get_latest(&self) -> Result<Vec<u8>, UpdateError>;
}

/// Always asks (the user pressed the button).
pub fn check_now(_fetch: &dyn Fetch, _current: &str) -> Outcome {
    unimplemented!()
}

/// Whether the weekly check is due: the toggle is on and the last check is a week old or
/// missing.
pub fn auto_due(_u: &UpdateSettings, _now: Timestamp) -> bool {
    unimplemented!()
}

/// The weekly check: asks only when [`auto_due`]; otherwise makes no request.
pub fn check_if_due(
    _u: &UpdateSettings,
    _now: Timestamp,
    _fetch: &dyn Fetch,
    _current: &str,
) -> Option<Outcome> {
    unimplemented!()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    struct Fake {
        calls: Cell<u32>,
        answer: Result<Vec<u8>, UpdateError>,
    }

    impl Fake {
        fn new(answer: Result<Vec<u8>, UpdateError>) -> Fake {
            Fake {
                calls: Cell::new(0),
                answer,
            }
        }
        fn release(tag: &str) -> Fake {
            Fake::new(Ok(body(tag, &format!("{RELEASE_URL_PREFIX}tag/{tag}"))))
        }
    }

    impl Fetch for Fake {
        fn get_latest(&self) -> Result<Vec<u8>, UpdateError> {
            self.calls.set(self.calls.get() + 1);
            self.answer.clone()
        }
    }

    fn body(tag: &str, url: &str) -> Vec<u8> {
        format!(
            r#"{{"id":1,"tag_name":"{tag}","name":"x","html_url":"{url}","assets":[{{"browser_download_url":"https://evil.example/a.exe"}}]}}"#
        )
        .into_bytes()
    }

    fn v(s: &str) -> Version {
        Version::parse(s).unwrap()
    }

    #[test]
    fn version_compare_table() {
        use std::cmp::Ordering::{Equal, Greater, Less};
        let table = [
            ("1.2.3", "1.2.3", Equal),
            ("v1.2.3", "1.2.3", Equal),
            ("0.1.0", "0.1.1", Less),
            ("0.1.1", "0.1.0", Greater),
            ("0.9.0", "0.10.0", Less),
            ("1.0.0", "0.99.99", Greater),
            ("2.0.0", "10.0.0", Less),
            ("1.0.0-rc.1", "1.0.0", Less),
            ("1.0.0", "1.0.0-rc.1", Greater),
            ("1.0.0-alpha", "1.0.0-beta", Less),
            ("1.0.0-alpha.2", "1.0.0-alpha.10", Less),
            ("1.0.0-alpha", "1.0.0-alpha.1", Less),
            ("1.0.0-1", "1.0.0-alpha", Less),
            ("1.0.0+build5", "1.0.0", Equal),
            ("1.0.0+a", "1.0.0+b", Equal),
        ];
        for (a, b, want) in table {
            assert_eq!(v(a).cmp(&v(b)), want, "{a} vs {b}");
        }
    }

    #[test]
    fn invalid_versions_are_refused() {
        for s in [
            "",
            "v",
            "1",
            "1.2",
            "1.2.3.4",
            "a.b.c",
            "01.2.3",
            "1.2.-3",
            " 1.2.3",
            "1.2.3 ",
            "1.2.3-",
            "1.2.3-a..b",
            "99999999999999999999.0.0",
            "vv1.2.3",
            "1.2.3-\u{e9}",
        ] {
            assert!(Version::parse(s).is_none(), "{s:?} must be refused");
        }
    }

    #[test]
    fn release_urls_are_allow_listed() {
        let ok = [
            "https://github.com/tuanluu-agilityio/dark-clipboard/releases/tag/v0.1.0",
            "https://github.com/tuanluu-agilityio/dark-clipboard/releases/latest",
            "https://github.com/tuanluu-agilityio/dark-clipboard/releases/tag/v0.1.0-rc.1",
        ];
        for u in ok {
            assert_eq!(ReleaseUrl::parse(u).unwrap().as_str(), u);
        }
        let bad = [
            "",
            "https://github.com/tuanluu-agilityio/dark-clipboard/releases/",
            "https://github.com/tuanluu-agilityio/dark-clipboard/releases",
            "http://github.com/tuanluu-agilityio/dark-clipboard/releases/tag/v1.0.0",
            "https://github.com.evil.example/tuanluu-agilityio/dark-clipboard/releases/x",
            "https://github.com/tuanluu-agilityio/dark-clipboard-evil/releases/tag/v1",
            "https://github.com/tuanluu-agilityio/dark-clipboard/releases/../../evil",
            "https://github.com/tuanluu-agilityio/dark-clipboard/releases/tag/v1?x=1",
            "https://github.com/tuanluu-agilityio/dark-clipboard/releases/tag/v1#frag",
            "https://github.com/tuanluu-agilityio/dark-clipboard/releases/tag/v1 calc",
            "https://github.com/tuanluu-agilityio/dark-clipboard/releases/tag/v1\n",
            "https://github.com/tuanluu-agilityio/dark-clipboard/releases/a\\b",
            "https://github.com/tuanluu-agilityio/dark-clipboard/releases/a%2e%2e",
            "https://github.com/tuanluu-agilityio/dark-clipboard/releases/a\"b",
            "https://github.com/tuanluu-agilityio/dark-clipboard/releases/\u{e9}",
            "HTTPS://github.com/tuanluu-agilityio/dark-clipboard/releases/tag/v1",
            "file:///c:/windows/system32/calc.exe",
        ];
        for u in bad {
            assert!(ReleaseUrl::parse(u).is_none(), "{u:?} must be refused");
        }
    }

    #[test]
    fn a_good_response_parses_and_ignores_every_other_field() {
        let r = parse_response(&body(
            "v0.2.0",
            "https://github.com/tuanluu-agilityio/dark-clipboard/releases/tag/v0.2.0",
        ))
        .unwrap();
        assert_eq!(r.version, v("0.2.0"));
        assert_eq!(r.shown, "0.2.0");
        assert_eq!(
            r.url.as_str(),
            "https://github.com/tuanluu-agilityio/dark-clipboard/releases/tag/v0.2.0"
        );
    }

    #[test]
    fn the_response_parser_refuses_bad_input_with_content_free_errors() {
        let good_url = format!("{RELEASE_URL_PREFIX}tag/v1.0.0");
        // Oversize: one byte past the cap, even if it would be valid JSON.
        let mut big = body("v1.0.0", &good_url);
        big.resize(MAX_BODY_BYTES + 1, b' ');
        assert_eq!(parse_response(&big), Err(UpdateError::TooLarge));
        // Exactly the cap is fine.
        big.truncate(MAX_BODY_BYTES);
        assert!(parse_response(&big).is_ok());
        // Garbage.
        for junk in [
            &b""[..],
            b"not json",
            b"\xff\xfe\x00",
            b"[]",
            b"null",
            b"\"x\"",
            b"{",
        ] {
            assert_eq!(
                parse_response(junk),
                Err(UpdateError::Malformed),
                "{junk:?}"
            );
        }
        // Missing or wrongly typed fields.
        for j in [
            r#"{}"#.to_string(),
            r#"{"tag_name":"v1.0.0"}"#.to_string(),
            format!(r#"{{"html_url":"{good_url}"}}"#),
            format!(r#"{{"tag_name":5,"html_url":"{good_url}"}}"#),
            r#"{"tag_name":"v1.0.0","html_url":null}"#.to_string(),
            format!(r#"{{"tag_name":"latest","html_url":"{good_url}"}}"#),
        ] {
            assert_eq!(
                parse_response(j.as_bytes()),
                Err(UpdateError::Malformed),
                "{j}"
            );
        }
        // A good tag with an address outside the allow list.
        let evil = body("v1.0.0", "https://evil.example/releases/tag/v1.0.0");
        assert_eq!(parse_response(&evil), Err(UpdateError::BadUrl));
        // Deep nesting does not overflow the stack.
        let deep = format!("{}{}", "[".repeat(100_000), "]".repeat(100_000));
        assert_eq!(parse_response(deep.as_bytes()), Err(UpdateError::Malformed));
    }

    #[test]
    fn a_newer_release_is_available_and_carries_the_validated_address() {
        let f = Fake::release("v0.2.0");
        match check_now(&f, "0.1.0") {
            Outcome::Available { shown, url } => {
                assert_eq!(shown, "0.2.0");
                assert!(url.as_str().starts_with(RELEASE_URL_PREFIX));
            }
            other => panic!("unexpected {other:?}"),
        }
        assert_eq!(f.calls.get(), 1);
    }

    #[test]
    fn the_same_or_an_older_release_is_up_to_date() {
        assert_eq!(
            check_now(&Fake::release("v0.1.0"), "0.1.0"),
            Outcome::UpToDate
        );
        assert_eq!(
            check_now(&Fake::release("v0.0.9"), "0.1.0"),
            Outcome::UpToDate
        );
        // The running build is the final version of a published pre-release.
        assert_eq!(
            check_now(&Fake::release("v1.0.0-rc.1"), "1.0.0"),
            Outcome::UpToDate
        );
    }

    #[test]
    fn every_failure_is_one_failed_outcome() {
        for e in [
            UpdateError::Network,
            UpdateError::TooLarge,
            UpdateError::Malformed,
        ] {
            assert_eq!(check_now(&Fake::new(Err(e)), "0.1.0"), Outcome::Failed);
        }
        assert_eq!(
            check_now(&Fake::new(Ok(b"junk".to_vec())), "0.1.0"),
            Outcome::Failed
        );
        // An unreadable running version is a failure, not an update.
        assert_eq!(
            check_now(&Fake::release("v9.0.0"), "garbage"),
            Outcome::Failed
        );
    }

    fn on(last: Option<i64>) -> UpdateSettings {
        UpdateSettings {
            auto_check: true,
            last_check_ms: last,
        }
    }

    #[test]
    fn no_request_is_made_when_the_toggle_is_off() {
        let f = Fake::release("v9.9.9");
        let off = UpdateSettings::default();
        assert!(!off.auto_check);
        // Never checked, and a year later: still nothing.
        for now in [0, AUTO_INTERVAL_MS * 52, i64::MAX] {
            assert!(!auto_due(&off, Timestamp(now)));
            assert_eq!(check_if_due(&off, Timestamp(now), &f, "0.1.0"), None);
        }
        let old = UpdateSettings {
            auto_check: false,
            last_check_ms: Some(1),
        };
        assert_eq!(
            check_if_due(&old, Timestamp(AUTO_INTERVAL_MS * 9), &f, "0.1.0"),
            None
        );
        assert_eq!(f.calls.get(), 0, "the fake must never have been called");
    }

    #[test]
    fn the_weekly_check_runs_only_when_a_week_has_passed() {
        let now = Timestamp(AUTO_INTERVAL_MS * 10);
        // Never checked: due.
        let f = Fake::release("v0.2.0");
        assert!(matches!(
            check_if_due(&on(None), now, &f, "0.1.0"),
            Some(Outcome::Available { .. })
        ));
        assert_eq!(f.calls.get(), 1);
        // Checked just under a week ago: not due.
        let f = Fake::release("v0.2.0");
        let recent = on(Some(now.0 - AUTO_INTERVAL_MS + 1));
        assert!(!auto_due(&recent, now));
        assert_eq!(check_if_due(&recent, now, &f, "0.1.0"), None);
        assert_eq!(f.calls.get(), 0);
        // Exactly a week ago: due.
        let f = Fake::release("v0.1.0");
        let week = on(Some(now.0 - AUTO_INTERVAL_MS));
        assert!(auto_due(&week, now));
        assert_eq!(
            check_if_due(&week, now, &f, "0.1.0"),
            Some(Outcome::UpToDate)
        );
        assert_eq!(f.calls.get(), 1);
        // The clock went backwards (last check far in the future): due, so it repairs itself.
        assert!(auto_due(&on(Some(now.0 + AUTO_INTERVAL_MS * 3)), now));
    }

    #[test]
    fn the_request_constants_match_the_decision() {
        assert_eq!(
            LATEST_URL,
            "https://api.github.com/repos/tuanluu-agilityio/dark-clipboard/releases/latest"
        );
        assert_eq!(MAX_BODY_BYTES, 262_144);
        assert_eq!(TIMEOUT, Duration::from_secs(10));
        assert_eq!(AUTO_INTERVAL_MS, 604_800_000);
    }
}
