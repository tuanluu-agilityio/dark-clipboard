# Task 106: Headless dev binary `apps/desktop --headless`

**Agent:** `builder` · **Model:** sonnet · **Branch:** `task/106-headless-binary` (from `main`) · **Review:** `reviewer` (sonnet; upgrade to opus if logging or retention logic changes in app)
**Global Constraints:** GC-01 (the binary may depend on `darkclip-windows`; portable crates still may not), GC-05, GC-06 (blocking), GC-08, GC-10, GC-14. **Plan:** Task 106 row, amendments S-6, F-02, S-10.

## Read first
`crates/darkclip-app/src/{api.rs, capture.rs, settings.rs}`, `crates/darkclip-windows/src/lib.rs`, `docs/architecture/system-overview.md` §3 (StorageFull rule), `docs/security/privacy-design.md` §7.

## Files
Create `apps/desktop/{Cargo.toml, src/main.rs, src/log.rs}`, `docs/testing/perf-windows.md`. Modify root `Cargo.toml` (members += `apps/*`), `crates/darkclip-app/src/{api.rs, capture.rs}` (S-6, F-02), `scripts/check.ps1` + `scripts/check.sh` (S-10), `README.md` (dev run line), `CHANGELOG.md`.

## Requirements
1. Binary `dark-clipboard`. `--headless` runs: data dir (`%LOCALAPPDATA%\DarkClipboard` on Windows; `--data-dir <path>` override for tests), open `App` with the platform bundle from `darkclip_windows::create()` (on non-Windows: print an i18n "not supported yet" message and exit 2), run until Ctrl+C (`SetConsoleCtrlHandler` via a tiny safe wrapper in `darkclip-windows`, or the `ctrlc` crate; state which), then shut down within 2 s. Without `--headless`: print usage (UI is Phase 3).
2. **Content-free logging** to `<data dir>/logs/dark-clipboard.log` (size-capped 1 MiB, one rotation): start/stop, capability summary, counts, error **codes**. Only `&'static str`, numbers, enum names via explicit mapping. No `{:?}` of any model type.
3. **S-10 check rule** in both check scripts: in `apps/desktop/src/**`, a log/print macro line must not contain `preview`, `content`, `bytes`, `text`, `query`, `{:?}`, `{:#?}`. Fail with file:line.
4. **S-6:** `App::capabilities() -> Capabilities` (stored from `PlatformBundle.caps` at open); the binary logs it at start.
5. **F-02:** on `StorageError::StorageFull` from an insert, the actor runs retention once and retries the insert once; second failure → `Unavailable` event + counter, actor alive. Test with a storage fake or an injected error hook (state which).
6. `--headless` idle: no busy loops (actor and monitor block on channels/messages).

## Tests
- App: `capabilities_exposed`, `storage_full_runs_retention_then_retries_once`, `storage_full_twice_reports_unavailable`.
- Binary: `cli_without_flag_prints_usage` (assert on exit code, not on text), `headless_with_data_dir_starts_and_stops` (spawn with `--data-dir <tempdir> --headless --exit-after-ms 500`, a hidden test flag; exit 0; log file exists and contains no clipboard text: copy nothing, just check it has only known lines). This test opens a clipboard listener but does not write the clipboard.
- Manual (the agent runs these, read-only for the clipboard): 10 min idle CPU and private memory of `dark-clipboard --headless` (`Get-Process` samples every 60 s), Ctrl+C equivalent shutdown time (send the console a close via the hidden `--exit-after-ms`; real Ctrl+C left to the owner). Record command lines and raw numbers in `docs/testing/perf-windows.md`, with date and machine.

## Not doing
UI, tray, hotkeys (Phase 3).
