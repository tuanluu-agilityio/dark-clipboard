# Task 211: History window

**Agent:** `builder` · **Model:** sonnet · **Branch:** `task/211-history-window` · **Review:** `reviewer` (opus)
**Global Constraints:** GC-06, GC-10, GC-14. **Source:** owner UX prompt §3 (D-036): a main history window besides the popup. Favorites stay P1 (Phase 5).

## Requirements
1. A resizable window (remembers size/position per monitor) opened from the tray ("History…") and from the popup (Ctrl+H): left pane filters (All, Pinned, kinds, date ranges, source apps), centre virtualized list (reuses the 204 model), right detail pane (reuses 206).
2. Actions: copy/restore, copy as plain text, pin/unpin, delete (single and multi-select with confirmation), context menu with the same actions, keyboard equivalents.
3. Uses the same `App` APIs as the popup on the worker thread; no business logic in the UI (owner prompt rule).

## Tests
View-model tests for multi-select and confirmation flows. Manual checklist rows.
