# Task 207: Settings window

**Agent:** `builder` · **Model:** sonnet · **Branch:** `task/207-settings` · **Review:** `security-reviewer` (opus; privacy UI copy)
**Global Constraints:** GC-06, GC-07 (blocking: OS/Darkshot markers shown as locked "always on"), GC-10, GC-14. **Plan:** Phase 3 Task 207.

## Requirements
1. Sections: **General** (shortcut with conflict warning from 203, language en/vi, start at login (208 wires it), popup position near cursor/center, close on focus loss, "Enter also pastes" from 205), **Privacy** (pause/private, excluded apps list with built-in defaults marked `builtin` + "pick running app" (process names only, no window titles), sensitive handling drop/store masked/store, capture kinds, OS and Darkshot markers shown as a locked row "always respected"), **History** (max items, max age, max storage, clear history with confirmation and "keep pinned", storage stats incl. per-skip-reason counters), **About** (version, capabilities report from `App::capabilities()`, licences incl. GPLv3 and third-party list, data folder path with "open folder").
2. Saves atomically via the Task 011 settings API; invalid input rejected with an i18n message; read-only settings state (D-028 `DefaultedReadOnly`) shown as a banner.
3. Opened from tray, popup gear, and IPC `ShowSettings`.

## Tests
Settings view-model tests: validation; built-in password-manager entries are removable only after an explicit confirmation; the markers row has no toggle at all; read-only banner state.
