# Task 206: Previews and item detail

**Agent:** `builder` · **Model:** sonnet · **Branch:** `task/206-previews` · **Review:** `reviewer` (sonnet)
**Global Constraints:** GC-06, GC-10, GC-14. **Plan:** Phase 3 Task 206.

## Requirements
1. List rows: kind icon, preview (2 lines; monospace for `code`), source app name when known, relative time, pin badge, masked badge for masked clips (masked preview only, never content).
2. Detail pane (toggle with → or Space): text up to the stored preview plus "load full text" on demand from storage on the worker thread, capped at 100 KiB for display; image thumbnail (256 px) with dimensions, full image loaded only on demand and downscaled off the UI thread; file list with names (paths shown, since the user copied them; never logged).
3. No full image decode on the UI thread; decoded preview images cached (LRU, ≤ 32 MiB).

## Tests
View-model tests (row formatting en/vi, masked rows never request content, LRU bound, display cap).
