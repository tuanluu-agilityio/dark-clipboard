# Phase 7: UI redesign (modern, minimal, keyboard-first)

> **Status: COMPLETE 2026-11-25** (phase review 709 APPROVED; budgets per D-086). Approved by the owner 2026-11-24 (D-078) (design: `docs/specs/ui-redesign/README.md`).
> Task packages `docs/tasks/7NN-*.md` are written just before each task starts (700 and 710 first),
> because later packages depend on what Task 700 finds (menus under the software renderer). Owner prompt
> `prompts/ui-redesign.md` is requirements input (D-036). Numbered 7 because Phase 6 is
> reserved for CLI/MCP/AI (D-036); proposed to run **before** 0.2 Paste Stack so new UI builds on
> the kit.

**Goal:** Rebuild the popup, history and settings windows on one shared Slint component library
with SVG icons, new tokens and a complete keyboard model, keeping every feature, privacy rule and
the Rust hosts. Windows first (the only platform with adapters); the UI code is platform-neutral.
**Global Constraints:** all; GC-03/06/07 (privacy), GC-14 (i18n), GC-17 (no P2), GC-18, GC-19.

## Review Focus

1. Masked clip: no content in row, preview pane, detail panel, tooltip or accessible text.
2. Paste only on Ctrl+Enter (or the setting): no new path (row menu "Paste into app", mouse) skips
   the T-19 checks; Enter/double-click/buttons only copy.
3. GC-07: the "Always protected" rows have no control and no code path that changes markers.
4. Every existing key in `popup/keys.rs` and `history/keys.rs` keeps its action (tests unchanged
   or only extended).
5. Performance budgets in `05-acceptance.md` §2 (P1, P3, P5) re-measured, not estimated.
6. Every new string in i18n EN + VI; glyph-scan and width-budget tests pass.

## Tasks

| Task | Title | Depends | Agent / model | Reviewer | Risk | Acceptance criteria |
|---|---|---|---|---|---|---|
| 700 | Foundation kit: tokens in `theme.rs` (+ WCAG test pairs), Lucide SVGs in `ui/icons/` + licence notice, `ui/kit/` (`Icon`, `IconButton` focusable with tooltip, `Button`, `KeyCap`, `Tooltip`, `Switch`, `Menu`), check `ContextMenuArea` under the software renderer; baseline P4/P6 measured | Phase 5 | builder / sonnet | reviewer / sonnet | Low | A1, A9; kit unit-tested where testable; existing windows still compile and use new tokens |
| 710 | App icon, option A (D-078): generator script for an 8-size `.ico` (hand-tuned 16/20/24), exe resource via `embed-resource` (build-dependency, as Darkshot), window icon on every Slint window, tray icon from pre-rendered state images (badge per state) replacing the code-drawn glyph | 700 | builder / sonnet | reviewer / sonnet | Low | `cargo deny` passes; tray view-model tests keep state → icon mapping; owner rows: exe, taskbar, Alt+Tab, tray on light/dark taskbar, 100/150 % |
| 701 | Popup layout: header (search + 4 icon buttons), filter segment + More filters menu, 48 px rows with kind icons and row actions, status line, floating toast, row menu; new keys (Ctrl+K, Ctrl+,, Shift+F10, F1 stub, Ctrl+1..9, Ctrl+Shift+F) | 700 | builder / sonnet | reviewer / opus | Medium | Review Focus 1, 2, 4; width budget (EN/VI) for header + status |
| 702 | Popup preview side pane: 760 px width, placement keeps the window on one monitor (flip), image/text/files/masked states | 701 | builder / sonnet | reviewer / sonnet | Low | Placement unit tests (`popup/place.rs`) incl. right-edge and mixed DPI |
| 703 | History layout: sidebar (grouped `FilterItem`s with counts), toolbar, 56 px rows (no Alt+N), detail panel with icon toolbar + tags/note/collections sections; footer, org row and hint lines removed | 700 | builder / sonnet | reviewer / opus | Medium | Review Focus 1, 4; every removed button's action reachable (table in package) |
| 704 | History interaction: keyboard context menu (Menu/Shift+F10, submenus), bulk bar, select mode, responsive rail (Ctrl+B) and drawer, F6 regions, min width 640 | 703 | builder / sonnet | reviewer / sonnet | Medium | Key-map tests; width budget for toolbar and bulk bar |
| 705 | Settings: five sections, cards/rows, `Switch`/`Select`/`Field`, one `ConfirmDialog` replacing five overlays, dirty save bar + Ctrl+S, Ctrl+1..5; "Always protected" card | 700 | builder / sonnet | security-reviewer / opus | Medium | Review Focus 3; every setting present exactly once (test over the section model) |
| 706 | Secondary surfaces: welcome, Copy as / Edit panels, org dialogs, empty/error states, shortcuts sheet (F1) content in popup and history | 701, 703 | builder / sonnet | reviewer / sonnet | Low | Welcome still capture-off until Start; F1 lists match key maps (test) |
| 707 | Accessibility pass: label/role scan test (A3), hit-target scan (A6), focus order and tooltip-on-focus, Narrator descriptions, remaining width budgets (A7) | 701–706 | builder / sonnet | reviewer / sonnet | Low | A3–A7 automated parts green |
| 708 | Verification: P1–P6 measurements, checklist owner rows added (per window: DPI, Tab walk, keys, Narrator, Vietnamese) | 707 | platform-verifier / sonnet | reviewer / sonnet | Low | Numbers recorded with commands in `docs/testing/` |
| 709 | Phase review | all | specialist / opus | — | — | Review Focus 1–6 proven |

## Why these models

- **sonnet builders** for every UI task: Slint layout and Rust host wiring are well specified by
  the design; no FFI, storage or capture code is touched. **haiku is not used**: Slint markup errors
  are costly to iterate and the popup paste path is privacy-adjacent.
- **opus reviewers** where a task touches the paste path or masked content (701, 703) and
  **security-reviewer / opus** for settings (705: privacy presentation, GC-07). Others sonnet (D-077:
  UI layout is low risk).
- **specialist / opus** for the phase review only.
- Never `fable` (CLAUDE.md).

## Order and stopping points

700 → 710 → 701 → 702 → 703 → 704 → 705 → 706 → 707 → 708 → 709. Each task merges via PR on its own;
after 702 the popup is complete and shippable alone; after 704 history; after 705 settings. Any
release (0.2.0) only when the owner asks and after `/phase-review` APPROVED (D-077).

## Estimate

Similar Phase 3/5 UI tasks cost 0.2–0.5 M tokens each (implementation + review + fixes,
`memory-bank/progress.md`). Eleven tasks: roughly 3.5–4.5 M tokens.
