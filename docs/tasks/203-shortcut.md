# Task 203: Global shortcut

**Agent:** `builder` · **Model:** sonnet · **Branch:** `task/203-shortcut` · **Review:** `reviewer` (sonnet)
**Global Constraints:** GC-06, GC-10, GC-14. **Plan:** Phase 3 Task 203; Review Focus 1.

## Requirements
1. `global-hotkey` (latest compatible). Default `Ctrl+Shift+V`. The shortcut string is stored in settings (`popup_shortcut`), parsed by a pure parser (`Ctrl|Alt|Shift|Win` + key; case-insensitive; platform-standard names) with clear errors.
2. Registration failure (taken by another app) is **not silent**: tray tooltip + a warning shown in settings (207 renders it; this task exposes it in status) with the reason "shortcut in use". The app keeps running; `--toggle` still works.
3. Pressing the shortcut toggles the popup (open ↔ close); two presses within 150 ms count once (debounce).
4. Changing the shortcut unregisters the old one first and rolls back on failure.

## Tests
Parser unit tests (valid, invalid, duplicates, unknown key, Vietnamese/English error keys). Toggle/debounce logic as a pure state machine test. No real key injection in tests.
