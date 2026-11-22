# Task 101: `darkclip-windows` crate and clipboard monitor thread

**Agent:** `specialist` · **Model:** opus · **Branch:** `task/101-windows-monitor` (from `main`) · **Review:** `reviewer` (opus)
**Global Constraints:** `docs/plans/global-constraints.md`, especially GC-01, GC-03, GC-04, GC-05, GC-06, GC-10, GC-13.
**Plan:** `docs/plans/phase-2-windows-capture.md` (Task 101 row, amendments F-01/S-4, S-5, F-03; Review Focus 4).

## Read first
`docs/spikes/windows-clipboard-monitor.md` (answers 1, 4, 5, 7, 8), `crates/darkclip-platform/src/{traits,fake,caps}.rs`, `crates/darkclip-app/src/capture.rs` (`on_signal`, source handling, `decide_all_sources`). The spike code in `spikes/` may be **read** for API names; never copy it (CLAUDE.md).

## Files
- Create `crates/darkclip-windows/{Cargo.toml, src/lib.rs, src/monitor.rs, src/win.rs, src/access.rs}`.
- Modify `crates/darkclip-platform/src/{traits.rs, fake.rs}` (ChangeSignal), `crates/darkclip-app/src/capture.rs` (use the signal's source), tests that construct `ChangeSignal`.

## Requirements
1. **Crate layout.** `windows` (0.62, `cargo add` with only the features needed) is a `[target.'cfg(windows)'.dependencies]` entry. On other OSes the crate compiles to an empty library (so the 3-OS workspace build stays green). Crate root has the GC-10 deny attributes. All `unsafe` lives in `win.rs` (thin wrappers, each block with `// SAFETY:`); other modules call safe wrappers. Every handle (window, class, listener, clipboard open) is released on every path (RAII guards).
2. **`pub fn create() -> Result<PlatformBundle, PlatformError>`** in `lib.rs`. In this task `access` is a `WinAccess` whose `sequence()` works and whose `snapshot_meta`/`read`/`write` return `PlatformError::Unsupported("not yet")` (Tasks 102–104 fill them). `source` is a resolver returning `None` (Task 105). `caps`: `monitor: Native`, `backend: "win32"`, others honest for what exists now.
3. **Monitor.** `WinMonitor::start(tx)` spawns one thread that registers a window class, creates a message-only window (`HWND_MESSAGE`), calls `AddClipboardFormatListener`, and runs `GetMessageW`. On `WM_CLIPBOARDUPDATE` it sends `ChangeSignal { seq: GetClipboardSequenceNumber(), at, source }`. `start` returns only after the window exists (or with its error). `stop()` posts a private quit message, joins the thread, and the thread calls `RemoveClipboardFormatListener` + `DestroyWindow` + `UnregisterClassW`. `stop` is idempotent and `Drop` calls it. A send error (receiver dropped) ends the loop cleanly.
4. **F-01 / S-4: `ChangeSignal` carries `source: Option<ClipboardSource>`** (drop `Copy`; keep `Clone`, a manual or content-free `Debug` is fine since sources are app ids). The Windows monitor fills it from a resolver closure called on the monitor thread at `WM_CLIPBOARDUPDATE` time; in this task that closure is the `None` resolver (Task 105 plugs in the real one; design the monitor so it takes a `Box<dyn Fn() -> Option<ClipboardSource> + Send>` or the `SourceResolver` trait). The fake's `copy()` puts the copy's source into the signal.
5. **Actor:** for each signal use `signal.source`; fall back to `SourceResolver::resolve()` at dequeue only when the signal has none (backends that cannot resolve at signal time). Every source seen for a change is still checked against excluded apps (`decide_all_sources` unchanged); the first non-None source stays primary. `PlatformBundle.source` stays (fallback; decision D-031 records this).
6. **F-03:** `WinAccess::sequence()` always returns `Ok(GetClipboardSequenceNumber() as u64)`.
7. **S-5 (applies from here on):** no retry loop in the adapter. A failed `OpenClipboard` returns `PlatformError::Busy` once; the actor owns retries.
8. No window titles, no content, no logging macros in the crate (it is an adapter; GC-06).

## Tests
Write first, see them fail (GC-11).
- Platform/app: existing tests updated for the new field; new `signal_source_is_primary_over_dequeue_resolve` (fake: copy with source A, then change the resolver to B before the actor dequeues → stored source A, B still checked for exclusion) and `excluded_source_in_signal_skips`.
- Windows unit (run normally, they do **not** touch clipboard content): `monitor_starts_and_stops_cleanly` (start, stop within 2 s, thread joined; start/stop twice in a row works), `drop_stops_monitor`, `sequence_is_ok`.
- Windows integration that **write** the clipboard: name them `real_clipboard_write_*`, mark `#[ignore = "writes the real clipboard"]`. `real_clipboard_write_same_process_signals` (write via a small test helper using `SetClipboardData`, expect a signal whose seq equals `GetClipboardSequenceNumber` after the write within 500 ms) and `real_clipboard_write_other_process_signals` (`powershell -NoProfile -Command Set-Clipboard -Value x`). **Do not run ignored tests**: the owner is using this machine and they overwrite the clipboard. Report them as "written, not run".
- No test simulates keyboard or mouse input.

## Not doing
`snapshot_meta`, `read`, `write` (102–104), source resolution (105), the binary (106).
