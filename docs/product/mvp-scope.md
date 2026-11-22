# MVP Scope (P0) and Requirements

Status: Phase 0, 2026-11-22.

## 1. Release strategy

| Release | Content | Platforms | Gate |
|---|---|---|---|
| 0.1.0-alpha (internal) | P0 on Windows | Windows 10 22H2+, Windows 11 | Phase 3 exit criteria |
| 0.1.0 (first public) | P0 | Windows (supported); macOS 13+ and Linux X11 / KDE Wayland / wlroots Wayland (preview) | Phase 5 exit criteria, platform matrix rows verified on real machines |
| 0.2.x | P1 | Same | |
| 0.3.x | P2 (CLI, MCP) | Same | Security review of the MCP surface |

"Preview" means it ships, it is labelled preview in the README and in About, and its matrix rows are marked verified or not.

GNOME Wayland is **not** promised for 0.1.0. See `docs/architecture/platform-compatibility.md`.

## 2. In scope (P0)

1. Background capture of text, rich text (as extra representation), images, file lists.
2. Popup: global shortcut (default `Ctrl+Shift+V` on Windows/Linux, `Cmd+Shift+V` on macOS; configurable), type to search, arrow keys, Enter = copy, Shift+Enter = copy as plain text, Ctrl/Cmd+Enter = copy and paste (where supported), Delete = remove, Ctrl/Cmd+P = pin, Esc = close.
3. Pin, delete, clear history (with confirmation; pinned kept unless chosen).
4. Pause (5 min, 1 h, until resumed) and private mode. Tray icon reflects state.
5. Excluded apps list with built-in defaults (password managers), editable.
6. Mandatory exclusion of OS-concealed content and Darkshot-excluded content.
7. Sensitive-data detection as a secondary safeguard (API keys, tokens, private keys, card numbers with Luhn, passwords after labels).
8. Retention: max items, max age, max storage; pinned exempt.
9. Settings window; start at login; English and Vietnamese.
10. Local storage only: SQLite + blob directory in the per-user data dir.
11. Content-free diagnostics log (rotation, 1 MiB × 3).

## 3. Explicitly not in P0 (non-goals)

Collections, tags, notes, favorites UI (P1), transforms (P1), CLI and MCP (P2), any AI provider (P2), encryption at rest beyond OS facilities (P2), sync (P3), plugin or scripting engine (never planned), window-title capture (never by default), OCR of images (Darkshot owns OCR), telemetry (never by default; if ever added it is opt-in and documented).

## 4. Functional requirements

| ID | Requirement |
|---|---|
| FR-01 | The app records a new history entry within 500 ms of an eligible clipboard change. |
| FR-02 | Identical content copied again does not create a duplicate. It moves the existing clip to the top, increments `copy_count`, and updates `last_copied_at`. |
| FR-03 | Writes made by Dark Clipboard itself (copy back) do not create entries. |
| FR-04 | Content carrying any mandatory exclusion marker is never read beyond format enumeration, never stored, never logged. |
| FR-05 | Content from an excluded app is not stored. If the source app cannot be determined, the clip is stored (the policy is documented), unless private mode or pause is active. |
| FR-06 | Pause and private mode stop all reads of clipboard data. Change notifications may still arrive and are dropped. |
| FR-07 | Search returns results for prefix and substring queries over text previews, notes and tags (P1), ranked by match, pin, recency. |
| FR-08 | Selecting a clip puts all its stored representations back on the clipboard; "plain" puts only text. |
| FR-09 | Retention runs at startup and every 10 minutes, and after inserts that exceed a limit. |
| FR-10 | Deleting a clip removes its row, FTS row and unreferenced blobs. The UI states that the OS clipboard itself is not cleared unless the user picks "also clear clipboard". |
| FR-11 | All user-visible strings come from the i18n table (Darkshot convention). |
| FR-12 | Settings are stored as versioned JSON; a corrupt file falls back to defaults and is preserved as `.bak`. |

## 5. Non-functional requirements and budgets

Budgets are targets. Each has a measurement method; Phase 0 measured only items marked (measured).

| ID | Budget | How measured |
|---|---|---|
| NFR-01 | Popup visible in ≤ 100 ms (p95) after shortcut on a warm app | Timestamp at hotkey event and at first frame/`DOMContentLoaded` of the pre-created hidden window |
| NFR-02 | Search over 10,000 clips: ≤ 30 ms p95 per keystroke | Criterion-style bench in `darkclip-storage` |
| NFR-03 | Capture path (notify → stored) ≤ 50 ms p95 for text < 64 KiB, excluding OS delays | Bench with fake platform plus Windows integration test |
| NFR-04 | Idle CPU ≈ 0% on Windows (event-driven). macOS ≤ 0.2% with 250–500 ms changeCount poll. | Process CPU time over 10 min idle |
| NFR-05 | Memory: core process ≤ 40 MiB private bytes idle with UI hidden; the webview adds to this. Total budget on Windows to be confirmed from the Phase 0 spike (`docs/spikes/tauri-shell.md`) | Task Manager / `Get-Process` private bytes |
| NFR-06 | Installer/bundle ≤ 15 MiB on Windows (excluding WebView2, which ships with Windows 11) | Release artifact size |
| NFR-07 | Database size cap respected within 1% after retention | Integration test |
| NFR-08 | No panic in release paths (`panic = "abort"` in release, as in Darkshot) | Clippy lints `unwrap_used`, `expect_used` in non-test code of core crates |
| NFR-09 | Crash-safe storage: SQLite WAL, atomic settings writes (temp file and rename) | Kill-during-write test |

## 6. Privacy requirements (summary; detail in `docs/security/privacy-design.md`)

PR-01 local-first, no account; PR-02 no telemetry by default; PR-03 no clipboard content in logs, errors, `Debug`, crash reports; PR-04 mandatory exclusion markers; PR-05 excluded apps; PR-06 private mode and pause; PR-07 retention; PR-08 sensitive-data detection as secondary; PR-09 AI access off by default and read-only when enabled; PR-10 no implicit upload to AI; PR-11 MCP has no filesystem access; PR-12 never bypass Darkshot exclusions.

## 7. Cross-platform limitations accepted for P0

- Linux GNOME Wayland: capture may not work without a compositor that offers a data-control protocol. The app detects this and shows a clear "capture unavailable on this session" state, never silent failure.
- Linux Wayland: no auto-paste, global shortcut only via the xdg-desktop-portal GlobalShortcuts portal where available, otherwise the user binds a desktop shortcut to `dark-clipboard --toggle`.
- macOS: auto-paste needs Accessibility permission; reading the pasteboard may trigger the system's pasteboard privacy prompt on macOS 15.4+/26 if enabled by the user or the OS. See risk R-03.
