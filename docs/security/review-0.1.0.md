# Security re-review before 0.1.0 (Windows)

Task 407. Date: 2026-11-24. Reviewer: specialist agent (opus), read-only on product code. Scope: the threat model (`docs/security/threat-model.md`, T-01 to T-20) against the system as built on Windows at commit 9e60487 (`main` at bda38a5: Phases 1 to 3 and Phase 5 Tasks 401 to 406, 411, 412). macOS and Linux are out of scope (no hardware, Q-04).

Method: code reading and `rg` searches over `crates/` and `apps/`, the quality gate, `cargo deny`, `cargo tree`, an unsafe inventory, the search bench, and two headless start-up runs against damaged databases in temporary folders. Earlier task reviews (security-reviewer on 011, 401, 404, 405, 406, 411, 412) were not repeated line by line; where this review relies on them it says so. GC-19 was kept: the UI was never launched, no screenshot, no input simulation, the real clipboard was never written, and the binary ran only with a temporary `--data-dir` that was deleted afterwards.

## Verdict

**0.1.0 for Windows is releasable once the one high item (F407-01) is fixed**, provided the release notes keep the "alpha, untested by hand" wording of `docs/user/windows-alpha.md` and the untested list below is published with it. No critical item was found. The three medium items (F407-02, F407-03, F407-04) should be fixed in the same fix task if possible; if not, they must be listed as known issues in `CHANGELOG.md`.

High items that need a fix task before release:

| ID | Title | Threat |
|---|---|---|
| F407-01 | Windows Error Reporting is not disabled for the app, although T-03 lists it as a mitigation and the welcome screen says "Nothing leaves this computer" | T-03 |

## Findings by severity

### High

#### F407-01 (high, T-03): crash reports can carry clip memory to Windows Error Reporting

- **Where:** no call anywhere in `apps/desktop/src/` or `crates/darkclip-windows/src/` (`rg -n "WerAddExcludedApplication|WerSetFlags|set_hook" crates apps` finds nothing); release profile `Cargo.toml:22` (`panic = "abort"`); claim in `docs/security/threat-model.md:39` (T-03: "Windows WER local dumps disabled for our process (Darkshot `wer.rs` pattern)"); user-facing promise `crates/darkclip-core/src/i18n.rs:552` ("Nothing leaves this computer").
- **What:** a panic in a dependency (Task 406 already found one in `png`), an arithmetic or slice bug in a dependency, or a native fault ends the process through `abort`, which Windows hands to WER. WER queues a report with a minidump of the process, and depending on the user's diagnostic-data settings may upload it or later request a larger dump. The process holds clip text, previews and decoded images in memory, so a dump can contain clipboard content. The mitigation the threat model names was never implemented, and the app has no panic hook.
- **Fix:** port the Darkshot pattern (`crates/darkshot-windows/src/wer.rs` in the Darkshot repo, read only): at start-up, before the platform bundle exists, call `WerAddExcludedApplication(<exe file name>, FALSE)` (per user, HKCU, no admin) and `WerSetFlags(WER_FAULT_REPORTING_FLAG_NOHEAP)`, ignoring errors, in `apps/desktop/src/winsys.rs` (already the desktop's `unsafe` Win32 module) or in `darkclip-windows`. Check both calls against the Microsoft documentation (`werapi.h`) when implementing. Add a unit test that the function runs without error and, if possible, a registry read-back test under HKCU. Keep the threat-model text as is once implemented; if the owner decides against it, change T-03 by a dated note and drop "Nothing leaves this computer" instead.

### Medium

#### F407-02 (medium, T-04): the data folder relies on inherited ACLs; on this machine a third principal can read it

- **Where:** `crates/darkclip-storage/src/blobs.rs:48` (`ensure_private_dir`) and `:80` (`restrict_file`) set 0700/0600 on Unix and do nothing on Windows; `apps/desktop/src/main.rs:207` (`%LOCALAPPDATA%\DarkClipboard`).
- **What:** on Windows the data folder (`history.db`, WAL, blobs, logs, pre-restore copies, staging folders) gets whatever ACL `%LOCALAPPDATA%` passes down. That is usually user + SYSTEM + Administrators, but not always. Observed on the review machine (`icacls %LOCALAPPDATA%`, ACLs only, no data folder opened): an extra inherited entry `<machine>\<developer-sandbox group>:(I)(OI)(CI)(RX)` added by a third-party developer tool. Every member of that group can read the whole clipboard history. A `--data-dir` in a shared folder inherits that folder's ACL in the same way. Unix gets an explicit 0700, so Windows is the weaker platform here. The pipe already uses an explicit protected DACL (`apps/desktop/src/single_instance.rs:303`).
- **Fix:** when the app creates or opens the data folder on Windows, set a protected DACL with inheritance on it: the current user's SID and SYSTEM, full control, object and container inherit (for example SDDL `D:P(A;OICI;FA;;;<user SID>)(A;OICI;FA;;;SY)`; decide whether to keep BUILTIN\Administrators). The code must live in `apps/desktop` or `darkclip-windows`, not in `darkclip-storage` (GC-01). Apply it to the existing folder on start, so current installs are fixed too. Update T-04 ("explicit protected DACL") and add a manual row (`icacls` shows exactly the expected entries).

#### F407-03 (medium, reliability, DB corruption and restore): a database that cannot be opened makes the app quit without a word, and restore is unreachable

- **Where:** `apps/desktop/src/ui/mod.rs:340` to `:353` (`App::start` error → `start_failed` log line → `quit_soon()`); `crates/darkclip-storage/src/db.rs:141` (open has no `quick_check`); spec `docs/architecture/data-model.md:175` ("the desktop shows a message and does not start capture"); unused string `ErrSchemaTooNew` (`crates/darkclip-core/src/i18n.rs:61`).
- **Measured** (headless, temporary folders, synthetic data, then deleted; `target/release/dark-clipboard.exe` built from 9e60487):
  - A: `history.db` replaced by 63 bytes of text. `dark-clipboard.exe --data-dir <tmp>\a --headless --exit-after-ms 3000` → exit code 1, log `start version=0.0.1` / `start_failed code=STORAGE_ERROR`. The UI path takes the same `App::start` branch and quits with no window and no message (read from code; the UI was not launched).
  - B: `--seed-demo-clips 2000` into `<tmp>\b` (1,527,808-byte database), then 4,096 bytes at offset 761,856 overwritten with `0xA5`. `--headless --exit-after-ms 3000` → exit code 0; log `capabilities ...`, `running`, `capture_off`, `unavailable what=retention`, `stop ms=3`. The app runs on a damaged database; the only signal is the tray's "unavailable" state, and retention (deletion of expired clips) stops working until the database is repaired.
- **Why it matters:** the Task 405 recovery path (Settings > History > Restore) needs the app running, so it cannot help exactly when the database is damaged or newer. A user sees the app "not start" and has no hint to move `history.db` aside. In case B, clips the user expects to expire stay on disk.
- **Fix:** on a start failure in the UI, show a native message box with an i18n text (newer version: `ErrSchemaTooNew`; damaged: a new string) and the data-folder path, and offer "Move the damaged history aside and start empty" (rename to `history.db.damaged-<ms>` with its WAL and SHM, then start; the user can then restore a backup). Consider `PRAGMA quick_check` at open (measure its cost on a 10,000-clip database first) and a `SQLITE_CORRUPT` code mapped to that same path at run time. Owner manual rows for both cases.

#### F407-04 (medium, T-12, docs and DoS): the threat model claims a peer check that does not exist, and another local user can block the app from starting

- **Where:** `docs/security/threat-model.md:48` (claims `GetNamedPipeClientProcessId` + token user); `apps/desktop/src/single_instance.rs:282` to `:303` (pipe name `dark-clipboard-<hash of SID>-s<session>`), `:316` (`FILE_FLAG_FIRST_PIPE_INSTANCE`), `apps/desktop/src/ui/mod.rs:521` (IPC failure → `start_failed stage=ipc`, app quits).
- **What:** (a) the DACL (`D:P(A;;GA;;;<user SID>)`) does keep other users out, so the missing peer check is a documentation error, not a hole. (b) Named-pipe names are global, SIDs are not secret and the hash is deterministic, so another local account can create that pipe name first. Our first-instance create then fails and the app quits silently (no message, same as F407-03). A victim's `--toggle` client could also connect to the squatter's pipe; it sends only the 2-byte command and uses `SECURITY_IDENTIFICATION`, so nothing else leaks. Needs a second local account on the same machine; result is denial of service only, failing closed.
- **Fix:** correct T-12 by a dated note (DACL + `PIPE_REJECT_REMOTE_CLIENTS` + identification-level client; no peer check), or add the peer check. On a first-instance failure while the mutex is ours, check the existing pipe's server process (`GetNamedPipeServerProcessId` + token user) and show a message instead of quitting silently (shares the message path of F407-03).

### Low

#### F407-05 (low, sensitive data in backups): backups are plain zip files and keep "do not sync" clips

- **Where:** `crates/darkclip-storage/src/backup.rs:5` to `:10`; README "Backup, restore, export and import".
- **What:** a `.dcbackup` is an unencrypted zip of the whole history (clips, images, notes, tags), and by design (D-064) it keeps clips that the source app marked `CanUploadToCloudClipboard = 0`. Masked and expired clips are purged and the snapshot vacuumed (verified in code; covered by Task 405's security review). If the user saves it into a cloud-synced folder (OneDrive Documents or Desktop is common), the whole history, including "do not sync" clips, is uploaded by the sync client. The README does not say the file is unencrypted. Exports are fine: masked, sensitive and NoCloud clips are left out.
- **Fix:** one sentence in the backup dialog and README: the file is not encrypted and contains your full history, including clips apps asked not to sync; keep it out of synced folders. Optional: warn when the chosen path is under the OneDrive folder (`%OneDrive%`).

#### F407-06 (low, docs): the alpha guide says the app has no network access

- **Where:** `docs/user/windows-alpha.md:80` ("The app has no network access and sends nothing anywhere").
- **What:** wrong since Task 412 (opt-in update check). README is already correct.
- **Fix:** in Task 410 (0.1.0 docs), replace with the README wording on the opt-in update check.

#### F407-07 (low, licences): the release must ship notices for all bundled crates

- **Where:** `apps/desktop/src/prefs/about.rs:13` (`THIRD_PARTY`: direct dependencies only, by design); `deny.toml`.
- **What:** `cargo deny check` passes (licences ok), and every licence is GPL-3.0-compatible. But the binary statically links hundreds of transitive crates under MIT, BSD, Apache-2.0, ISC, Zlib, Unicode-3.0, MPL-2.0, BSL-1.0 and CDLA-Permissive-2.0, most of which require their notice to go with binary copies. The About list covers direct dependencies only.
- **Fix:** in Task 408/409, generate a full third-party notice file at release time (for example `cargo about generate`) and ship it next to the binary and in the release assets. Also remove the 2 licence exceptions `cargo deny` reports as never encountered (`deny.toml:37` `i-slint-backend-linuxkms` and `:45` `i-slint-backend-testing`) or keep them on purpose for other targets.

#### F407-08 (low, T-01): KeePass 2 (classic) is not a built-in exclusion; Bitwarden and 1Password are untested

- **Where:** `crates/darkclip-core/src/model.rs:653` to `:657` (Windows defaults: `keepassxc.exe`, `1password.exe`, `bitwarden.exe`, `darkshot.exe`).
- **What:** only KeePassXC 2.8.0 and Darkshot 0.6.0 were verified on a real clipboard (platform checklist, Task 107). KeePass 2 (`KeePass.exe`) is a common Windows password manager and is not on the list, so it is protected only if it sets a marker (not verified here). Bitwarden and 1Password are on the list but were never tested (R-02 open). The excluded-app match also relies on a cooperative owner window (F-09).
- **Fix:** add `keepass.exe` as a built-in Windows exclusion (new decision; markers stay the primary defence), and keep the three password-manager rows as owner manual rows before claiming them.

#### F407-09 (low, T-06, memory): the peak memory of a very large image capture is not measured

- **Where:** `crates/darkclip-core/src/limits.rs:9` to `:13` (64 Mpx, 256 MiB RGBA, 32,768 px side); `crates/darkclip-core/src/model.rs:516` (`image_max_mb: 20` for the stored PNG).
- **What:** caps are checked from headers before allocation (fuzzed in Task 406), so memory is bounded, but the bound is large: a 64-Mpx DIB on the clipboard can hold its read copy, a 256 MiB RGBA buffer and the encoder's buffers at once, several hundred MiB for a moment, against 7 to 12 MiB normal use (Task 212). Not measured, and the 4K DIB real-clipboard row (Task 110) is still an owner row.
- **Fix:** none needed for release; add an owner manual row (copy an 8K screenshot, watch private bytes in Task Manager) and record the peak.

### Info

- **F407-10 (info, NFR-02, not security):** search bench under load, 5 runs: worst case p95 29.1 to 32.0 ms (3 of 5 over 30 ms), realistic p95 22.2 to 30.4 ms (1 of 5 over). The machine was not idle. Details: `docs/testing/perf-windows.md`, section "Search bench re-run, 5 times (Task 407, F-21)". F-21 stays open.
- **F407-11 (info, unsafe):** portable crates `darkclip-platform`, `darkclip-storage` and `darkclip-app` have no `unsafe` but do not forbid it either (only `apps/desktop/src/main.rs:10` has `#![deny(unsafe_code)]`). Adding `#![forbid(unsafe_code)]` to those three crate roots would lock the inventory below in place.
- **F407-12 (info, carried over):** F-09 (excluded-app `app_id` not authenticated), F-23 (unbounded post-message drain in the actor; restored `sqlite_stat1`), F-24 (libFuzzer targets never run), F-17 (tray icon not re-added after an Explorer restart). Still valid, none blocks a release.

## Areas checked without a finding

| Area | Threat | Evidence |
|---|---|---|
| Content in logs | T-03 | The file log (`apps/desktop/src/log.rs:16` to `:20`) only takes `&'static str` events, keys and names, and numbers; no `Box::leak`/`String::leak` in product code (`rg` hits only a test). Gate rules GC-06 and S-10 pass. `eprintln!`/`println!` in `apps/desktop/src/main.rs` print i18n strings or a count only. |
| Content in errors and Debug | T-03 | `StorageError` maps rusqlite errors to code names, dropping messages and parameters (`crates/darkclip-storage/src/error.rs:77`); `PlatformError` carries static text, a format name and numbers; `ApiError` is codes only. Every type that holds content has a manual `Debug` (`Clip`, `ClipContent`, `ClipDetailView`, `NewClip`, `SearchQuery`, `ClipSummary`, `RawRepresentation`, `MaskHint`, `DecodedImage`, the UI row/item/filter types and others, 30 impls). `Representation`, `Sensitivity`, `ClipMetadata` derive `Debug` but hold no content. `{:?}` in non-test format strings appears only in `StorageError`'s `Display` (an `io::ErrorKind` and an archive reason code). |
| Panics in product code | T-03 | All `panic!`/`unreachable!` hits are in tests; GC-10 lints active. The known `png` palette panic is pre-checked (`crates/darkclip-app/src/image.rs:51`, and the copy-back decoder, Task 406). Remaining risk is F407-01. |
| Temp files | T-03, T-05 | Product code uses no system temp folder (`std::env::temp_dir` only in a fuzz entry). Partial files: blob `.tmp` (swept at open), backup/export `<dest>.dark-clipboard-partial` (removed on error, `crates/darkclip-storage/src/archive.rs:399`), staging folders inside the data folder (removed at next open). Masked clips are purged from the pre-restore copy and from backups. |
| Password-manager markers | T-01, T-02 | Windows marker table fixed in code (`crates/darkclip-core/src/markers.rs:33`): `ExcludeClipboardContentFromMonitorProcessing`, `CanIncludeInClipboardHistory = 0` (a non-DWORD value fails closed), `Clipboard Viewer Ignore` always skip; Darkshot's set reported as `DarkshotExcluded`; `CanUploadToCloudClipboard = 0` alone stores with `no_export`. Policy order: settings, self-write, markers, transient, excluded app (`crates/darkclip-core/src/policy.rs:109`). Gate asserts the marker names. Darkshot is also a built-in exclusion; removing it needs a confirmation that says its markers still exclude it. |
| Network | T-20, GC-08 | Only `apps/desktop/src/update.rs` uses `ureq` (gate rule passes); HTTPS only, no redirects, 10 s, 256 KiB, fixed headers (`update.rs:292`); only a validated release-tag URL is opened, with `explorer.exe` by absolute path. `crates/` have no HTTP/TLS crate on any target (gate). Off by default; owner rows untested. |
| IPC authorisation | T-12 | Protected DACL for the user's SID only, `PIPE_REJECT_REMOTE_CLIENTS`, message mode, 16-byte buffers, exact 2-byte versioned protocol, at most 4 handler threads, mutex and pipe scoped per session (`Local\`, `-s<session>`), client opens with `SECURITY_IDENTIFICATION`. Only toggle, settings and quit cross the pipe; no clip data. Low-integrity and AppContainer processes are refused by the default mandatory label and the DACL. See F407-04. |
| Single instance | T-12 | Per-user, per-session mutex holding a SID hash; a `--data-dir` gets its own scope. Two sessions of the same user would run two instances on one data folder; SQLite locking keeps that safe but it is not the designed single writer (rare on client Windows). |
| Restore of a hostile backup | T-07, Task 405 | Read in code and covered by Task 405's three security-review rounds: archive limits, manifest, per-file SHA-256, defensive raw open, exact schema match including `sqlite_*`, `integrity_check` and `foreign_key_check` before the live database is touched, rollback on failure. Not re-tested here. |
| Exports and imports | T-05, T-07 | Export leaves out masked, sensitive and no-export clips; import re-derives kind, preview, hash and sensitivity through capture's `prepare`, drops the source, never sets `no_export`, clamps timestamps (`crates/darkclip-app/src/capture/transfer.rs:1` to `:13`). |
| Large images | T-06, T-07 | Header checks before allocation, side cap, `Zeroizing` buffers, metadata chunks ignored; fuzzed (Task 406, stable harness). See F407-09. |
| Copy-back | T-18, T-19 | One `OpenClipboard` attempt per call, retries owned and bounded by the actor, prepared blocks kept and freed for `Busy` retries (`crates/darkclip-windows/src/access.rs:758`, `:951`); no marker formats written (D-012). Real-clipboard copy-back passed in Task 107 (10/10 ignored tests, run then with owner consent); the UI paths are owner rows. |
| Window titles | T-17 | Gate rule SR-11 passes; the model has no title field. |
| Memory and idle cost | NFR-04, NFR-05 | Not re-measured. Task 212 (software renderer, `docs/testing/perf-windows.md` "Software renderer by default"): 6.8 to 7.1 MiB private hidden, 11.7 MiB after 20 shows, 0 s CPU over 291 s idle. |
| Popup latency | NFR-01 | Not re-measured. Task 212 R2: 50 shows over IPC, p50 12.1 ms, p95 18.4 ms, max 36.4 ms (first show). The real-shortcut row is an owner row. |

## Dependency audit

`cargo deny 0.22.0`, `cargo deny check` (advisory database fetched by the tool):

    advisories ok, bans ok, licenses ok, sources ok

Warnings only: 34 `duplicate` warnings (all targets, mostly Linux/macOS UI stacks: `windows-sys`, `syn`, `hashbrown`, `getrandom`, `toml*`, `rustix`, `nix`, `calloop`, `smithay-client-toolkit` and others) and 2 `license-exception-not-encountered` (`deny.toml:37`, `:45`).

Duplicates in the Windows build graph, `cargo tree -d -e normal --target x86_64-pc-windows-msvc`:

| Crate | Versions |
|---|---|
| getrandom | 0.2.17, 0.4.3 |
| hashbrown | 0.14.5, 0.16.1, 0.17.2 |
| miniz_oxide | 0.8.9, 0.9.1 |
| smol_str | 0.2.2, 0.3.7 |
| syn | 2.0.119, 3.0.10 |
| windows-sys | 0.52.0, 0.59.0, 0.61.2 |

All come in through Slint, winit and their dependencies; no action for 0.1.0 (binary size only). Licences: allowed list in `deny.toml` (MIT, Apache-2.0, Apache-2.0 WITH LLVM-exception, BSD-2/3-Clause, ISC, Unicode-3.0, Zlib, MPL-2.0, BSL-1.0, Unlicense) plus GPL-3.0-only for the Slint crates and CDLA-Permissive-2.0 for `webpki-roots`; the app itself is GPL-3.0-only. See F407-07 for notices.

## Unsafe and FFI inventory

Counted with `rg` (comment lines excluded); a `// SAFETY:` comment was checked for each site.

| File | `unsafe` blocks | `unsafe impl` | `unsafe fn` | SAFETY comments | Notes |
|---|---|---|---|---|---|
| `crates/darkclip-windows/src/win.rs` | 69 (1 in tests) | 1 (`Send for GlobalBlock`) | 0 | 71 | All Win32 clipboard, monitor, paste, autostart FFI of the adapter (`lib.rs` states all `unsafe` lives here) |
| `apps/desktop/src/winsys.rs` | 20 | 0 | 0 | 20 | Desktop Win32 helpers (`#![allow(unsafe_code)]` at the module) |
| `apps/desktop/src/single_instance.rs` | 15 | 0 | 0 | 16 | Mutex, named pipe, SID, session id (`#[allow(unsafe_code)]` on the `win` module) |
| `crates/darkclip-core/src/sensitive/wipe.rs` | 3 (1 in tests) | 0 | 0 | 3 | `write_volatile` zeroing |
| `crates/darkclip-fuzzgen/src/alloc.rs` | 4 | 1 (`GlobalAlloc`) | 4 (trait methods) | 5 | Test-only crate (dev-dependency), tracking allocator for the fuzz harness |
| everything else | 0 | 0 | 0 | — | `apps/desktop/src/main.rs:10` has `#![deny(unsafe_code)]` |

Every `unsafe` block and impl has a `// SAFETY:` comment; the four `unsafe fn` lines in `alloc.rs` are the `GlobalAlloc` trait signatures, whose bodies carry their own comments. Handle release on every path was reviewed in the owning tasks (101 to 110, 202, 205) and was not re-audited line by line here.

## Untested areas (honest list)

- **macOS and Linux: nothing tested** (Q-04, no hardware). Every macOS and Linux row in `docs/testing/platform-checklist.md` is untested; the macOS and Linux adapters do not exist yet (Phase 4 deferred). No claim beyond Windows may be made for 0.1.0.
- **Windows owner-only manual rows** (GC-19: agents cannot click, type, look at the screen or write the clipboard). Still `untested` in `docs/testing/platform-checklist.md`, by section: tray and console (Task 202), popup (204), copy back and paste (205), history window (211), welcome and start at login (208), Phase 3 review fixes (213), text transforms and edited copies (404), data management (411), backup, restore, export and import (405; the file dialogs are compiled but were never run), collections, tags, notes and favorites (403), opt-in update check (412, including "no connection with the toggle off"), and in the Task 107 section: elevated source app, copy-back then immediate copy in another app, Win+V after a copy-back, real Ctrl+C in a console. Not installed, so never tested: Bitwarden, 1Password, Word/Excel, RDP clipboard.
- **Owner performance rows** (`docs/testing/perf-windows.md`): software-renderer look at 100/150/200 % scaling, UI search timing (`perf what=search_done`), the real-shortcut NFR-01 run, IME and Narrator rows.
- **New manual rows proposed by this review:** data-folder ACL after F407-02 (`icacls` shows only the user and SYSTEM); start-up with a damaged and with a newer database after F407-03 (a message appears, "move aside" works, restore then works); WER exclusion after F407-01 (the HKCU `ExcludedApplications` value exists after first start); large-image peak memory (F407-09).
- **Not run:** libFuzzer targets (F-24); `#[ignore]` real-clipboard tests (GC-19); an idle-machine search bench (F-21).

## Commands run for this review

| Command | Result |
|---|---|
| `cargo test -p darkclip-storage --release --lib -- --ignored --test-threads=1 --nocapture bench_search` (5 times) | see F407-10 and `docs/testing/perf-windows.md` |
| `cargo deny check` | `advisories ok, bans ok, licenses ok, sources ok` (34 duplicate warnings, 2 unused licence exceptions) |
| `cargo tree -d -e normal --target x86_64-pc-windows-msvc` | 6 duplicated crates (table above) |
| `cargo build --release -p dark-clipboard` | ok, 13,078,528 bytes |
| `dark-clipboard.exe --data-dir <tmp>\a --headless --exit-after-ms 3000` (garbage `history.db`) | exit 1, `start_failed code=STORAGE_ERROR` |
| `dark-clipboard.exe --seed-demo-clips 2000 --data-dir <tmp>\b`, one page overwritten, then `--data-dir <tmp>\b --headless --exit-after-ms 3000` | exit 0, `unavailable what=retention` |
| `icacls %LOCALAPPDATA%`, `icacls %TEMP%` | ACLs only; see F407-02 |
| `powershell -File scripts/check.ps1` | `ALL CHECKS PASSED` (run after writing this report) |
