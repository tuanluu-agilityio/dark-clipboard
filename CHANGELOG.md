# Changelog

All notable changes to Dark Clipboard, newest first. Versions follow [SemVer](https://semver.org/). Sections per release: Added, Fixed, Known issues.

## [Unreleased]

### Added (Phase 3 desktop shell)
- Task 212: on Windows the UI uses Slint's software renderer by default (measured about 12 MiB private bytes after use and no idle CPU, against about 72 MiB and 0.2 % of a core with the GPU renderer). A user-set `SLINT_BACKEND` still wins. If selecting the software renderer fails the app logs `renderer_fallback`, but Slint may then fail to start too (on Windows the failure comes from creating the event loop, which cannot be retried); a software renderer failure when a window is created is handled inside Slint. The start log records `renderer=software|default|env`. Popup open time (`perf what=popup_shown`) is now timed from the first redraw after the show request, which works with every renderer; a new content-free `perf what=search_done ms=<n>` times a typed search from its debounced start to the results on screen.
- Task 205: copy back and paste. Enter puts the clip back on the clipboard, closes the popup and returns the focus to the window you were in. Ctrl+Enter also pastes it there (Ctrl+V through `SendInput`); a new setting `ui.enter_pastes` ("Enter also pastes", off by default) makes Enter paste too. The paste is never blind: it happens only if the window recorded when the popup opened still exists, belongs to the same process, is a normal visible top-level window (never the taskbar, the desktop or a tool window), takes the focus, and Shift/Alt/Win/Enter are released; otherwise the clip stays copied and a toast says "Copied — paste manually". A target running as administrator (higher integrity level, UIPI) gets no input and its own toast. The popup shows the Ctrl+Enter hint only where pasting works. Capability `auto_paste` is now native on Windows. The log records only a coarse `paste` outcome (ok, refused, elevated, failed).
- Task 204: the popup. A frameless, always-on-top dark window opens near the cursor on the monitor under it (per-monitor DPI aware) with the search box focused and the recent clips listed, pinned first. Typing searches as you type (accent-insensitive, matched words highlighted); filter chips for kind, pinned, today, 7 days and source app; the list loads more clips as you scroll. Keys: arrows, Page Up/Down, Home/End, Enter copies the clip back, Shift+Enter copies plain text, Ctrl+Enter copies (paste comes with Task 205), Delete removes with an Undo toast, Ctrl+P pins, Alt+1..9 copies the n-th result, Esc or clicking elsewhere closes; every action also has a button. Developer flag `--seed-demo-clips N --data-dir <dir>` writes N synthetic clips (never into the default data folder). Popup open time is logged content-free (`perf` event); `scripts/measure-popup.ps1` reports p50/p95.
- Task 201: `dark-clipboard` with no flag now starts the Slint app hidden (placeholder window, always dark, English and Vietnamese; the OS language is used on first run). Single instance per user: a second start forwards its command over a named pipe restricted to the current user and exits 0. New `--toggle` (show or hide the popup) and `--quit` ask the running instance; `--help` prints usage. `--headless` is unchanged. No tray, shortcut or popup content yet (Tasks 202-204); the console window of this developer build stays visible.

### Added
- Phase 0 blueprint: product vision, feature matrix, MVP scope, user journeys, system architecture, platform capability matrix, data model with JSON Schemas and examples, threat model, privacy design, AI/MCP interface draft, ADR-0001 to ADR-0006, roadmap and Phase 1–5 plans, Phase 1 task packages.
- Feasibility spikes on Windows (clipboard monitoring; Tauri 2 vs Slint shell). These are throwaway code under `spikes/`.
- Agent workflow (`CLAUDE.md`, `AGENTS.md`, `.claude/`), memory-bank, quality gate scripts, CI workflow.

### Added (Phase 2 Windows capture)
- Developer binary `dark-clipboard --headless` (Windows): runs the capture actor on the real Windows clipboard until Ctrl+C, with a content-free size-capped log. No UI yet.
- `App::capabilities()`; the Windows adapter reports read, write and privacy-marker support as native (real-clipboard tests ran in Task 107, see `docs/testing/platform-checklist.md`).
- The headless log records, on stop, one line of skip counts per reason (static reason names and numbers only).
- When storage is full, the capture actor runs retention once and retries the insert once before reporting the failure.
- Check rule S-10: log and print lines in the headless binary may not mention content-like words.

### Added (Phase 1 Foundation)
- Portable Rust workspace: `darkclip-core` (models, capture policy and privacy markers, classification and hashing, sensitive-data detection, i18n en/vi), `darkclip-platform` (adapter traits, capabilities, fake platform), `darkclip-storage` (SQLite + FTS5 with accent-insensitive search, content-addressed blobs, retention, migrations), `darkclip-app` (single capture actor, Application API, settings). Headless only: no OS adapter or UI yet.
- Quality gates: banned-crate, privacy-boundary and logging checks; cargo-deny.

### Fixed
- A `settings.json` that starts with a UTF-8 byte order mark (written by Notepad or PowerShell 5.1) is no longer treated as corrupt (it was renamed and all settings were reset); saving never writes a BOM.
- Windows: clipboard writes through OLE with a flush (for example Notepad) keep their source app, and an excluded app without privacy markers that writes this way is now skipped (F-14). Fixed in the capture actor; real-clipboard recheck pending.
- Windows: a screenshot-sized bitmap copy (for example 4K, offered only as a DIB) is stored when its PNG fits the image size limit; the limit applies to the stored PNG, and bitmap input is bounded separately (64 megapixels, 256 MiB decoded). Real-clipboard recheck pending.
- Windows: copying back a stored image too large to decode safely now puts the PNG alone on the clipboard instead of decoding it; a copy-back retried while the clipboard is busy converts the image once, and its converted data is dropped when the copy-back fails. Real-clipboard recheck pending.
- A clipboard change that replaced another before capture looked at it is no longer stored with the earlier app as its source.

### Known issues
- Windows build uses the GUI subsystem (no console window). Interactive cmd and PowerShell do not wait for `dark-clipboard --toggle`, `--quit` or `--help`: output may appear after the prompt and `%ERRORLEVEL%` / `$LASTEXITCODE` are not the real exit code (use `start /wait` or `Start-Process -Wait`). `--headless` started from Explorer has no console or tray and stops only via Task Manager: `--quit` does not reach a headless run (it has no instance pipe).
- Windows verification covered KeePassXC 2.8.0 and Darkshot; Bitwarden, 1Password, Office and RDP are untested.
- The popup is Windows only and was measured only through `--toggle` (IPC); the real-shortcut timing, Vietnamese IME, mixed-DPI monitors and 10,000-clip scrolling are owner manual checks. Image thumbnails and the detail view arrive in Task 206. Paste into the previous app (Task 205) is untested on a real desktop: Notepad, VS Code, Edge, Word, an elevated Notepad and a tray-opened popup are owner manual checks. A popup opened from the tray records the taskbar as the previous window, so there Ctrl+Enter only copies. "Enter also pastes" has no settings screen yet (Task 207); set `ui.enter_pastes` in `settings.json`. The popup may show a taskbar button while open.
- macOS and Linux behaviour is untested.
