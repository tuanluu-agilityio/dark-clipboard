# Task 204: Popup (search, list, keyboard)

**Agent:** `specialist` · **Model:** opus · **Branch:** `task/204-popup` · **Review:** `reviewer` (opus)
**Global Constraints:** GC-06, GC-10, GC-14. **Plan:** Phase 3 Task 204; Review Focus 1, 2, 4, 5; MVP §2 item 2; NFR-01, NFR-02; requirements folded from the owner's UX prompt (D-036).

## Read first
`crates/darkclip-app/src/api.rs` (search, list, stats), `crates/darkclip-storage/src/search.rs` (limits, cursor, filters), Tasks 201–203 code, `docs/product/user-journeys.md` J1–J3.

## Requirements
1. **Pre-created hidden window** (undecorated, always on top, ~420×560 logical px, rounded dark panel). Shown on the monitor under the cursor (position near cursor, clamped to the work area; setting "center" later in 207). Per-monitor DPI v2 awareness (manifest or API) so it renders crisp on mixed-DPI monitors.
2. **Search box** auto-focused on show; input debounced (~60 ms); empty query lists recent clips (pinned first). Results come from `App` search on the worker thread; stale responses (older query) are discarded. Highlight matched terms in previews (diacritic-insensitive, like FTS).
3. **Filter chips:** kind (text, rich text, image, files, url, code…), pinned only, today / 7 days, source app (from results). Favorites are P1 (Phase 5).
4. **Virtualized list:** a Slint `ListView` over a model that pages via the search cursor (load next page near the end); never loads full content or full images into the UI (previews and thumbnails only).
5. **Keyboard (MVP §2):** ↑/↓ (and PgUp/PgDn/Home/End), Enter = copy, Shift+Enter = copy as plain text, Ctrl+Enter = copy and paste (Task 205 wires paste; until then it copies), Delete = remove (with undo toast instead of a dialog), Ctrl+P = pin/unpin, Alt+1…9 = copy the n-th result, Esc = close, Tab moves between search, chips and list. Every action also reachable by mouse; accessible names on controls.
6. **States:** empty history (onboarding hint), no results, loading (only if > 150 ms), error (content-free message).
7. **Close behaviour:** losing focus closes the popup (setting later); the previously focused window is recorded at show time for Task 205.
8. **NFR-01 measurement:** timestamps at hotkey event and first frame after show (Slint rendering notifier), logged content-free with a `perf` event; a script `scripts/measure-popup.ps1` that parses 50 trials from the log and prints p50/p95. The agent runs it only with `--toggle` driven by the IPC (no key simulation) and records results in `docs/testing/perf-windows.md`; the real-shortcut measurement is an owner manual row.
9. 10,000-clip smoke: a dev flag `--seed-demo-clips N` (synthetic text only, written to `--data-dir`, refuses the default data dir) to test scrolling and search latency.

## Tests
View-model unit tests: query → request, stale-response discard, paging, keyboard map → actions, highlight ranges with Vietnamese diacritics. Manual rows added to the checklist: IME (Telex/VNI, Unikey), mixed-DPI monitor, shortcut double-press, 10k scroll.
