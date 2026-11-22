# Task 202: Tray icon and menu

**Agent:** `builder` · **Model:** sonnet · **Branch:** `task/202-tray` · **Review:** `reviewer` (sonnet)
**Global Constraints:** GC-06, GC-10, GC-14. **Plan:** Phase 3 Task 202; MVP §2 item 4; `prompts`-derived items (D-036): capture enabled/disabled indicator, basic status.

## Read first
`apps/desktop/src/` (Task 201), `crates/darkclip-app/src/api.rs` (capture mode, status), `docs/spikes/ui-shell-comparison.md` (tray-icon 0.26 worked with Slint).

## Requirements
1. `tray-icon` (latest compatible) on the Slint main thread. Icons for states: normal, paused, private, unavailable (capture failed / disabled before first-run Start). Icons are simple monochrome glyphs generated or drawn as PNG assets in `apps/desktop/assets/` (original artwork; no third-party icons).
2. Menu: Open (toggle popup), Pause ▸ 5 minutes / 1 hour / Until resumed, Resume (when paused), Private mode (check item), Settings…, Quit. Tooltip shows the state ("Dark Clipboard: capturing / paused until 14:05 / private"). All strings via i18n.
3. Mode changes go through `App` (actor commands) on the worker thread; the tray updates from `AppEvent::ModeChanged` and status.
4. Quit performs the same orderly shutdown as Task 106 (≤ 2 s) and removes the tray icon.

5. **No console window for the UI app** (Task 201 open issue): build with `#![windows_subsystem = "windows"]`; for `--headless`, `--help`, `--toggle`, `--quit` and errors, attach to the parent console (`AttachConsole(ATTACH_PARENT_PROCESS)`) so CLI output and Ctrl+C still work from a terminal. Integration tests that read stdout/exit codes must keep passing.

## Tests
View-model unit tests: state → icon id + tooltip key; menu item enabled/checked states per mode. Manual checklist rows added to `docs/testing/platform-checklist.md` (Windows, untested until the owner runs them).

## Not doing
Settings window content (207).
