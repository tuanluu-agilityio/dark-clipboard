# Phase 3: Desktop Shell (Slint) and P0 UX on Windows

> Task packages `docs/tasks/2NN-*.md` are written at phase start. Interfaces: `ClipApi` (Task 011), `CaptureHandle`/`AppEvent` (Task 010), `Capabilities` (Task 006).

**Goal:** A usable Windows 0.1.0-alpha: tray, global shortcut, popup search with keyboard navigation, copy and paste back, pin/delete, pause/private, settings, retention UI, autostart, English/Vietnamese, always-dark theme.
**Spec:** `docs/product/mvp-scope.md` §2, §5; `docs/product/user-journeys.md` J1–J6; ADR-0001.
**Global Constraints:** GC-06, GC-10, GC-14, GC-18.

## Review Focus
1. The shortcut pressed while the popup is open (toggle closes), and pressed twice quickly.
2. A popup on a secondary monitor with a different DPI (Darkshot's DPI lessons: per-monitor v2 awareness).
3. Focus return: after Enter, the previously focused window gets focus back. After Esc, too. With an elevated target app, paste falls back to copy with a toast.
4. Vietnamese IME (Telex/VNI via Windows IME or Unikey) typing in the search box.
5. 10,000 clips: the list scrolls without jank (virtualized model), and the search stays responsive.

## Tasks

| Task | Title | Depends | Agent / model | Reviewer | Risk | Acceptance criteria | Tests |
|---|---|---|---|---|---|---|---|
| 200 | Split `crates/darkclip-app/src/capture.rs` (> 2,000 lines) into modules (actor loop, pending/debounce, pipeline/store, copy-back) with no behaviour change; GC-03/GC-06 check paths updated to the new files (D-039) | P2 | specialist / opus | security-reviewer / opus | Medium (privacy gate moves) | All existing app tests pass unchanged; check scripts still enforce GC-03/GC-06 on the new files | Existing suite |
| 201 | Desktop app skeleton: single instance (named mutex), Slint event loop, wiring `App`; theme tokens from Darkshot's dark palette (colours copied as values, not code) | P2 | builder / sonnet | reviewer / sonnet | Low | Starts hidden; second instance forwards `--toggle` over IPC stub | Unit tests for arg parsing; manual |
| 202 | Tray icon and menu (open, pause 5 min / 1 h / until resumed, private mode, settings, quit); state icons | 201 | builder / sonnet | reviewer / sonnet | Low | States visible; menu strings from i18n | Manual checklist |
| 203 | Global shortcut (`global-hotkey`), conflict detection (registration failure → settings warning), configurable | 201 | builder / sonnet | reviewer / sonnet | Medium | Default `Ctrl+Shift+V`; failure shown, not silent | Unit test of shortcut parsing |
| 204 | Popup: pre-created hidden window, position near cursor/center on active monitor, search box, virtualized list (`VecModel` paging via `search`), keyboard map from MVP §2 | 201, 203 | specialist / opus | reviewer / opus | **High** | **NFR-01 measured**: ≤ 100 ms p95 shortcut → visible (log timestamps, 50 trials, in `docs/testing/perf-windows.md`); IME works | Measurement script; manual IME check |
| 205 | Copy back and paste: focus restore (`SetForegroundWindow` rules), `SendInput` Ctrl+V, UIPI fallback | 204, P2/104 | specialist / opus | security-reviewer / opus | High | Works in Notepad, VS Code, Edge, Word; elevated target → copy only + toast | Manual matrix |
| 206 | Previews: text (monospace for `code`), image thumbnails, file list; masked clips show masked preview and badge | 204 | builder / sonnet | reviewer / sonnet | Medium | No full image decode on the UI thread | Unit tests for view models |
| 211 | History window (owner UX prompt §3, D-036/D-044): filters, virtualized list, detail pane, multi-select delete, context menu; reuses 204/206 models | 204, 206 | builder / sonnet | reviewer / opus | Medium | Same APIs as popup; no business logic in UI | View-model tests; manual |
| 207 | Settings window: general (shortcut, language, autostart, popup position), privacy (pause, private, excluded apps with "pick running app", sensitive handling, capture kinds; OS markers shown as locked "always on"), history (retention, clear, storage stats), about (capabilities report, versions, licences) | 202 | builder / sonnet | security-reviewer / opus (privacy UI copy) | Medium | Saves atomically; invalid input rejected with message | Unit tests for settings view model |
| 208 | Autostart (`HKCU\...\Run`), first-run welcome (J6) | 207 | builder / sonnet | reviewer / sonnet | Low | Toggle works; welcome shows once | Manual |
| 209 | ADR-0001 revisit check: idle CPU 10 min, memory hidden/visible, IME, screen reader (Narrator) smoke | 204–208 | platform-verifier / sonnet | reviewer / opus | Medium | Numbers recorded; any trigger hit → stop and escalate to owner | Measurement |
| 210 | README (Windows alpha), CHANGELOG, user docs page, manual checklist; phase review | all | scaffolder / sonnet, reviewer / opus | — | — | 0.1.0-alpha build artifact with SHA-256 | `check.ps1` |

## Execution order and owner-prompt items (D-044)

Packages written 2026-11-23. Order: 200 → 201 → 202 → 203 → 204 → 205 → 206 → 211 → 207 → 208 → 209 → 210. Folded from the owner's prompts (D-036): single instance + IPC base (201), capture-state indicator (202), shortcut conflict explained (203), number shortcuts, filters, highlight, debounce, states, demo seeding (204), restore vs opt-in auto-paste with never-blind paste (205), history window (211), first-run consent with capture off until Start (208). Favorites stay in Phase 5. Dark theme only (D-037). First owner demo after 204.
