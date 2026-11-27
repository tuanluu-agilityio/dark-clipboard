# Technical context

Updated: 2026-11-22

## Development environment (Phase 0 machine)
- Windows 11 Home 10.0.26300 x64. Project folder `D:\2026 - Tuan\dark-clipboard` (the path contains spaces: always quote it).
- Rust 1.100.0 stable, `x86_64-pc-windows-msvc`, clippy and rustfmt installed. `cargo` lives in `%USERPROFILE%\.cargo\bin`.
- Node 24 (schema validation tooling only).
- No WSL, no macOS, no Linux machine available in Phase 0.
- Darkshot repo: `D:\2026 - Tuan\shotme` (read-only for this project).

## Commands
| Purpose | Command |
|---|---|
| Quality gate (Windows) | `powershell -File scripts/check.ps1`, which must print `ALL CHECKS PASSED` |
| Quality gate (macOS/Linux) | `bash scripts/check.sh` |
| Format | `cargo fmt --all` |
| Schema validation only | `node tools/schema-check/validate.mjs` |
| Regenerate schemas | `python scripts/gen_schemas.py` (then re-run validation) |
| Add a dependency | `cargo add -p <crate> <dep>` |
| Stable fuzz harness (Task 406) | see `docs/testing/fuzzing.md` (`cargo test --release ... -- --ignored`, env profile overrides) |
| Supply-chain check | `cargo deny check` (cargo-deny 0.22.0 installed locally; also runs in CI) |

## Planned dependencies (decided in ADRs; versions are pinned by Cargo.lock when added)
| Crate | Where | Why |
|---|---|---|
| `serde`, `serde_json`, `thiserror` | core and up | models, JSON, errors |
| `uuid` (v7) | core | sortable ids |
| `sha2` | core; darkclip-windows (copy-back block cache key, Task 110) | content hashing |
| `rusqlite` 0.43 (bundled SQLite 3.54.1) | storage | SQLite + FTS5 |
| `flate2` 1.1 (dev only) | darkclip-app tests (iCCP fixture) | test data |
| `png` 0.18 | storage, darkclip-windows (DIB→PNG, Task 103), app (103b) | thumbnails, DIB→PNG |
| `zeroize` 1.9 | darkclip-windows (Task 103), darkclip-app (103b) | wipe intermediate clipboard buffers |
| `windows` 0.62.2 (cfg(windows) only, default features off; added Task 101) | darkclip-windows | Win32 clipboard (spike-proven) |
| `objc2-app-kit` | darkclip-macos | NSPasteboard (Phase 4) |
| `x11rb`, `wayland-client`, `wayland-protocols(-wlr)` | darkclip-linux | X11/Wayland (Phase 4) |
| `slint` / `slint-build` **~1.19.0** (pinned: feature `unstable-winit-030` is outside semver, used only for focus loss, bring-to-front and popup timing in `popup_host.rs`); Windows default renderer: Slint **software** renderer on the winit backend (D-055, Task 212), `SLINT_BACKEND` overrides, `global-hotkey` 0.10, `tray-icon` 0.26 (Windows), `ksni` (Linux, later) | apps/desktop | UI shell (ADR-0001) |
| `embed-resource` 3.0.12, MIT (Task 710): **build-dependency**, `[target.'cfg(windows)'.build-dependencies]`, used in `apps/desktop/build.rs` to embed `res/app.rc` (the exe icon). Not compiled on macOS/Linux; `cargo deny check` passes with no `deny.toml` change. Pulls `cc`, `toml`, `vswhom`, `winreg` (build-time only) | apps/desktop (build) | Windows exe icon |
| Lucide icons `lucide-static` **1.79.0** (43 SVG files, vendored unmodified in `apps/desktop/ui/icons/`, ISC + MIT for Feather-derived icons; not a crate, embedded with `@image-url`; licence in `about.hbs`, Task 700) | apps/desktop | UI icons (D-078, D-079) |
| `unicode-normalization` 0.1.26 | core (Task 401) | NFC for tag names |
| `darkclip-storage` (workspace) | apps/desktop | demo seeding and the search cursor type (Task 204) |
| `darkclip-ai` (workspace, Task 601) | apps/desktop (Task 602+) | translate provider seam; deps darkclip-core, serde, serde_json only; network-free |
| `zip` **~9.0.5** (default features off; `deflate-flate2-zlib-rs` only; adds `typed-path`, reuses `flate2`/`zlib-rs`/`crc32fast` already in the tree) | storage (Task 405); app dev-dependency (test fixtures) | `.dcbackup` / `.dcexport` archives. Pinned below 9.1: zip 9.1+ depends on `windows-sys` on Windows, which GC-01 bans in the portable crates |
| `rusqlite` feature `backup` | storage (Task 405) | online backup API for the restore swap |
| `boon` 0.6 (dev only) | storage tests (Task 405) | validates `export.json` against `export.schema.json` (JSON Schema 2020-12; no network) |
| `windows` features `Win32_System_Com`, `Win32_UI_Shell`, `Win32_UI_Shell_Common` | apps/desktop (Task 405) | the shell's save and open file dialogs |
| `darkclip-fuzzgen` (workspace crate, dev-dependency only; depends on `png` already in the tree) | dev-dependency of core, app, storage, windows (Task 406) | seeded generator, mutator, heap-tracking allocator and runner of the stable `#[ignore]` fuzz harness; no product crate depends on it in a normal build |
| `ureq` 3.5.2 (default features off; feature `rustls` only: rustls 0.23 with the `ring` provider and `webpki-roots` 1.0; no OpenSSL, no native-tls, no gzip, no cookies, no json; adds `ureq-proto`, `rustls`, `rustls-webpki`, `rustls-pki-types`, `ring`, `webpki-roots`, `http`, `httparse`, `subtle`, `untrusted`, `utf8-zero`, `getrandom` 0.2) | apps/desktop only (Task 412, D-057) | opt-in update check in `src/update.rs`. `webpki-roots` is CDLA-Permissive-2.0: `deny.toml` has a per-crate licence exception. The gate fails if a crate under `crates/` depends on any HTTP or TLS client |
| `serde_json` 1.0.154 | apps/desktop (Task 412; already in the tree) | parse the release answer (`tag_name`, `html_url`) |
| `libfuzzer-sys` 0.4 (only in `fuzz/`, its own workspace, excluded from the product workspace and from `cargo deny`) | `fuzz/` cargo-fuzz targets (Task 406) | libFuzzer entry points; needs nightly + `cargo install cargo-fuzz`, not installed on this machine |

## Constraints
- Release profile: `opt-level="z"`, `lto=true`, `codegen-units=1`, `panic="abort"`, `strip=true` (Darkshot).
- No network crates in P0/P1. No async runtime.
- `cargo deny` policy in `deny.toml` (Task 001).

- Release tooling (Task 408): `cargo-about` 0.8.4 (installed in the release workflow, `--frozen`), `yaml` 2.9.1 (npm, `tools/schema-check`, workflow YAML gate).

- Website (Task 715, D-092): static `website/index.html` (inline CSS and JS, English and Vietnamese, no build step, no network). Geist and Geist Mono (SIL OFL 1.1) are self-hosted in `website/fonts/`. Published by `.github/workflows/pages.yml` (GitHub Pages, source GitHub Actions; a step fails the deploy if the page loads anything from another origin). Not part of the Rust workspace or the release.
