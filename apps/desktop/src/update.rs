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
pub const RELEASE_URL_PREFIX: &str =
    "https://github.com/tuanluu-agilityio/dark-clipboard/releases/";
/// The most response bytes read (256 KiB).
pub const MAX_BODY_BYTES: usize = 256 * 1024;
pub const TIMEOUT: Duration = Duration::from_secs(10);
/// The longest tag accepted, in bytes.
pub const MAX_TAG_BYTES: usize = 64;
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

/// A release page address that passed the allow check: the fixed prefix and then only
/// letters, digits and `. _ ~ / + -`; no `..`, query, fragment, escape or space. It is the only
/// address the app ever opens.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReleaseUrl(String);

impl ReleaseUrl {
    pub fn parse(s: &str) -> Option<ReleaseUrl> {
        let rest = s.strip_prefix(RELEASE_URL_PREFIX)?;
        let clean = !rest.is_empty()
            && !rest.contains("..")
            && rest
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"._~/+-".contains(&b));
        clean.then(|| ReleaseUrl(s.to_string()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// One dot-separated part of a pre-release; numbers sort before names (semver 2.0.0).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
enum PreId {
    Num(u64),
    Name(String),
}

/// A semantic version (`major.minor.patch` with an optional pre-release; build metadata is
/// ignored).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Version {
    major: u64,
    minor: u64,
    patch: u64,
    pre: Vec<PreId>,
}

/// A number without a leading zero, as semver requires.
fn plain_number(s: &str) -> Option<u64> {
    if s.is_empty() || !s.bytes().all(|b| b.is_ascii_digit()) || (s.len() > 1 && s.starts_with('0'))
    {
        return None;
    }
    s.parse().ok()
}

fn pre_id(id: &str) -> Option<PreId> {
    if id.is_empty() || !id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-') {
        None
    } else if id.bytes().all(|b| b.is_ascii_digit()) {
        plain_number(id).map(PreId::Num)
    } else {
        Some(PreId::Name(id.to_string()))
    }
}

impl Version {
    /// `1.2.3`, `v1.2.3`, `1.2.3-rc.1`, `1.2.3+build`. Anything else is `None`.
    pub fn parse(s: &str) -> Option<Version> {
        let s = s.strip_prefix('v').unwrap_or(s);
        let s = s.split_once('+').map_or(s, |(core, _)| core);
        let (core, pre) = match s.split_once('-') {
            Some((core, pre)) => (core, Some(pre)),
            None => (s, None),
        };
        let mut nums = core.split('.');
        let major = plain_number(nums.next()?)?;
        let minor = plain_number(nums.next()?)?;
        let patch = plain_number(nums.next()?)?;
        if nums.next().is_some() {
            return None;
        }
        let pre = match pre {
            None => Vec::new(),
            Some(p) => p.split('.').map(pre_id).collect::<Option<Vec<_>>>()?,
        };
        Some(Version {
            major,
            minor,
            patch,
            pre,
        })
    }
}

impl PartialOrd for Version {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Version {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        use std::cmp::Ordering::{Equal, Greater, Less};
        let mine = (self.major, self.minor, self.patch);
        let theirs = (other.major, other.minor, other.patch);
        let core = mine.cmp(&theirs);
        if core != Equal {
            return core;
        }
        // A pre-release is older than the release it leads to.
        match (self.pre.is_empty(), other.pre.is_empty()) {
            (true, true) => Equal,
            (true, false) => Greater,
            (false, true) => Less,
            (false, false) => self.pre.cmp(&other.pre),
        }
    }
}

/// What the response said: the newest release and where to read about it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Release {
    pub version: Version,
    /// The tag as shown to the user (`X.Y.Z`, no `v`); it passed `Version::parse`, so it only
    /// holds letters, digits and `. + -`.
    pub shown: String,
    pub url: ReleaseUrl,
}

/// Reads `tag_name` and `html_url` from the response and nothing else. The errors name the
/// kind of problem only, never the text.
pub fn parse_response(body: &[u8]) -> Result<Release, UpdateError> {
    if body.len() > MAX_BODY_BYTES {
        return Err(UpdateError::TooLarge);
    }
    let value: serde_json::Value =
        serde_json::from_slice(body).map_err(|_| UpdateError::Malformed)?;
    let field = |name: &str| value.get(name).and_then(|v| v.as_str());
    let tag = field("tag_name").ok_or(UpdateError::Malformed)?;
    let url = field("html_url").ok_or(UpdateError::Malformed)?;
    let version = Version::parse(tag).ok_or(UpdateError::Malformed)?;
    let url = ReleaseUrl::parse(url).ok_or(UpdateError::BadUrl)?;
    Ok(Release {
        version,
        shown: tag.strip_prefix('v').unwrap_or(tag).to_string(),
        url,
    })
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
pub fn check_now(fetch: &dyn Fetch, current: &str) -> Outcome {
    let Some(current) = Version::parse(current) else {
        return Outcome::Failed;
    };
    let release = fetch.get_latest().and_then(|body| parse_response(&body));
    match release {
        Ok(r) if r.version > current => Outcome::Available {
            shown: r.shown,
            url: r.url,
        },
        Ok(_) => Outcome::UpToDate,
        Err(_) => Outcome::Failed,
    }
}

/// Whether the weekly check is due: the toggle is on and the last check is a week old, missing
/// or in the future (the clock moved back).
pub fn auto_due(u: &UpdateSettings, now: Timestamp) -> bool {
    if !u.auto_check {
        return false;
    }
    match u.last_check_ms {
        None => true,
        Some(last) => last > now.0 || now.0.saturating_sub(last) >= AUTO_INTERVAL_MS,
    }
}

/// The weekly check: asks only when [`auto_due`]; otherwise makes no request.
pub fn check_if_due(
    u: &UpdateSettings,
    now: Timestamp,
    fetch: &dyn Fetch,
    current: &str,
) -> Option<Outcome> {
    auto_due(u, now).then(|| check_now(fetch, current))
}

/// Allows one check at a time (the button and the weekly path share it).
pub struct InFlight(std::sync::atomic::AtomicBool);

impl InFlight {
    pub const fn new() -> InFlight {
        InFlight(std::sync::atomic::AtomicBool::new(false))
    }

    /// True when the caller may start a check; it must call [`InFlight::end`] afterwards.
    pub fn try_begin(&self) -> bool {
        unimplemented!()
    }

    pub fn end(&self) {
        unimplemented!()
    }
}

/// The real request: HTTPS only, no redirects, no cookies, a 10 s limit for the whole call,
/// the body capped, and a fixed `User-Agent`. It sends nothing else of ours.
pub struct UreqFetch;

impl Fetch for UreqFetch {
    fn get_latest(&self) -> Result<Vec<u8>, UpdateError> {
        let config = ureq::Agent::config_builder()
            .timeout_global(Some(TIMEOUT))
            .https_only(true)
            .max_redirects(0)
            .user_agent(format!("DarkClipboard/{}", env!("CARGO_PKG_VERSION")))
            .build();
        let agent = ureq::Agent::new_with_config(config);
        let mut response = agent
            .get(LATEST_URL)
            .header("Accept", "application/vnd.github+json")
            .call()
            .map_err(|_| UpdateError::Network)?;
        let limit = u64::try_from(MAX_BODY_BYTES).unwrap_or(u64::MAX);
        response
            .body_mut()
            .with_config()
            .limit(limit)
            .read_to_vec()
            .map_err(|e| match e {
                ureq::Error::BodyExceedsLimit(_) => UpdateError::TooLarge,
                _ => UpdateError::Network,
            })
    }
}

/// Opens a release page in the browser. Only a validated [`ReleaseUrl`] is accepted. True when
/// the opener was started. Nothing is downloaded or run by this app.
pub fn open_release(url: &ReleaseUrl) -> bool {
    // The system opener by absolute path where there is one (no search path on Windows).
    #[cfg(windows)]
    let Some(program) = crate::winsys::windows_directory().map(|d| d.join("explorer.exe")) else {
        return false;
    };
    #[cfg(target_os = "macos")]
    let program = std::path::PathBuf::from("/usr/bin/open");
    #[cfg(not(any(windows, target_os = "macos")))]
    let program = std::path::PathBuf::from("xdg-open");
    std::process::Command::new(program)
        .arg(url.as_str())
        .spawn()
        .map(|mut child| {
            // Reap it off the UI thread so it never lingers.
            let _ = std::thread::Builder::new()
                .name("darkclip-open-release".into())
                .spawn(move || {
                    let _ = child.wait();
                });
        })
        .is_ok()
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
            "https://github.com/tuanluu-agilityio/dark-clipboard/releases/tag/v0.1.0-rc.1",
        ];
        for u in ok {
            assert_eq!(ReleaseUrl::parse(u).unwrap().as_str(), u);
        }
        let bad = [
            "",
            // Only release pages: never a download or the bare list.
            "https://github.com/tuanluu-agilityio/dark-clipboard/releases/latest",
            "https://github.com/tuanluu-agilityio/dark-clipboard/releases/download/v1.0.0/a.exe",
            "https://github.com/tuanluu-agilityio/dark-clipboard/releases/tag/",
            "https://github.com/tuanluu-agilityio/dark-clipboard/releases/tagx/v1",
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
    fn build_metadata_is_validated_and_never_shown() {
        assert!(Version::parse("1.0.0+build.5").is_some());
        for bad in [
            "1.2.3+",
            "1.2.3+a..b",
            "1.2.3+Security notice: download from evil.example",
            "1.2.3+a b",
            "1.2.3+\u{e9}",
            "1.2.3+a+b",
        ] {
            assert!(Version::parse(bad).is_none(), "{bad:?} must be refused");
        }
        let url = format!("{RELEASE_URL_PREFIX}tag/v9.9.9");
        let r = parse_response(&body("v9.9.9+meta.5", &url)).unwrap();
        assert_eq!(r.shown, "9.9.9");
        let r = parse_response(&body("v1.0.0-rc.1+x", &url)).unwrap();
        assert_eq!(r.shown, "1.0.0-rc.1");
        let r = parse_response(&body("2.0.0", &url)).unwrap();
        assert_eq!(r.shown, "2.0.0");
        // Text in the tag that is not a version never reaches `shown`.
        let evil = body("v9.9.9+Security notice: download from evil.example", &url);
        assert_eq!(parse_response(&evil), Err(UpdateError::Malformed));
    }

    #[test]
    fn an_overlong_tag_is_refused() {
        let ok = format!("1.0.0-{}", "a".repeat(MAX_TAG_BYTES - 6));
        assert_eq!(ok.len(), MAX_TAG_BYTES);
        assert!(Version::parse(&ok).is_some());
        let long = format!("{ok}a");
        assert!(Version::parse(&long).is_none());
        let url = format!("{RELEASE_URL_PREFIX}tag/v1");
        assert_eq!(
            parse_response(&body(&long, &url)),
            Err(UpdateError::Malformed)
        );
        assert!(parse_response(&body(&ok, &url)).is_ok());
    }

    #[test]
    fn only_one_check_runs_at_a_time() {
        let g = InFlight::new();
        assert!(g.try_begin());
        assert!(!g.try_begin(), "a second check must not start");
        g.end();
        assert!(g.try_begin());
        g.end();
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
