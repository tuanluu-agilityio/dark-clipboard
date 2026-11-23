# Task 412: Opt-in update check

**Agent:** `builder` · **Model:** sonnet · **Branch:** `task/412-update-check` · **Review:** `security-reviewer` (opus; first network code)
**Global Constraints:** GC-01, GC-06, GC-08 (amended by D-057: the only network code allowed before 0.1.0 is this opt-in check in `apps/desktop`), GC-10, GC-14, GC-18. **Plan:** Phase 5 Task 412; D-037.

## Requirements
1. Settings > About: a "Check for updates" button and an "Automatically check weekly" toggle, **off by default**; no request is ever made unless the user pressed the button or turned the toggle on.
2. One HTTPS GET to `https://api.github.com/repos/tuanluu-agilityio/dark-clipboard/releases/latest`; it sends no clip data, settings, identifiers or cookies (fixed User-Agent `DarkClipboard/<version>`); response capped at 256 KiB, timeout 10 s; only `tag_name` and `html_url` are read; `html_url` must start with `https://github.com/tuanluu-agilityio/dark-clipboard/releases/`. Semver compare with the running version.
3. Result: "up to date", or "version X available" with a button that opens the release page in the browser. **Never** downloads or installs anything.
4. Network code lives only in `apps/desktop` (one module); crates under `crates/` stay network-free (extend the check script to verify no HTTP client crate is a dependency of them). HTTP client: `ureq` with rustls (no OpenSSL), `cargo add`, `cargo deny check`.
5. Runs on a background thread; the UI never blocks; errors are content-free and shown as "could not check".
6. Docs: README privacy section and `docs/security/privacy-design.md` (dated "Changed by D-057" note): what is sent, when, how to keep it off.

## Tests (first, failing)
Version compare table; URL allow-check; response parser with oversize, garbage and missing fields; no request when off (the request function sits behind a trait; a fake counts calls); defaults have the toggle off. No test touches the real network.
