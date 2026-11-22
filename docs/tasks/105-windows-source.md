# Task 105: Windows `SourceResolver` (owner window → process image name)

**Agent:** `builder` · **Model:** sonnet · **Branch:** `task/105-windows-source` (from `main`) · **Review:** `security-reviewer` (opus)
**Global Constraints:** GC-06, GC-10, GC-13. **Plan:** `docs/plans/phase-2-windows-capture.md` Task 105. Runs right after 101 (D-031), because 101's monitor resolves the source at signal time.

## Read first
`docs/spikes/windows-clipboard-monitor.md` answer 7, `crates/darkclip-windows/src/{lib,monitor,win}.rs` (from Task 101), `docs/security/privacy-design.md` §6 (excluded apps match `app_id`, case-insensitive on Windows).

## Files
Create `crates/darkclip-windows/src/source.rs`; modify `src/{lib.rs, win.rs, monitor.rs}`, `scripts/check.ps1` and `scripts/check.sh` only if the window-title rule needs widening (it already bans `GetWindowText*`; add `GetWindowTextLength`/`InternalGetWindowText` if missing).

## Requirements
1. `WinSourceResolver` implements `SourceResolver`: `GetClipboardOwner()` → null → `None`; else `GetWindowThreadProcessId` → `OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION)` → `QueryFullProcessImageNameW` → file name only. Access denied (elevated or protected process) or any failure → `None`. Handle closed on every path.
2. Result: `ClipboardSource { app_id: <file name as reported, e.g. "KeePassXC.exe">, app_name: None, platform: Windows, confidence: Owner }`. The full path is never stored or returned (it can contain the user name).
3. The monitor (Task 101) uses this resolver on its thread at `WM_CLIPBOARDUPDATE`; `create()` returns it as `PlatformBundle.source`. `caps.source_app = Owner`.
4. Never call any window-title API (SR-11, check rule).

## Tests
- Unit, pure: `file_name_from_path` (`C:\\Program Files\\KeePassXC\\KeePassXC.exe` → `KeePassXC.exe`; a path without a separator; empty → `None`; non-UTF-16-valid → `None` or lossless handling, state which).
- Unit, real OS but read-only: `resolve_does_not_panic` (whatever owns the clipboard now; result is `None` or a name without `\\`).
- `#[ignore = "writes the real clipboard"] real_clipboard_write_owner_is_powershell` (owner after `Set-Clipboard` is `powershell.exe`). Do not run ignored tests (owner is using the machine).

## Not doing
Frontmost-window fallback (not needed on Windows), app display names (P1 UI).
