# Dark Clipboard UI redesign: design

Date: 2026-11-24 · Status: **approved by the owner 2026-11-24 (D-078)** · Owner prompt: local
`prompts/ui-redesign.md` (requirements input, D-036) · Plan: `docs/plans/phase-7-ui-redesign.md`

| Part | File |
|---|---|
| 1. UI audit (current state, contracts, keys, problems, invariants) | `01-audit.md` |
| 2. Design system (tokens, type, spacing, icons, components) | `02-design-system.md` |
| 3. Wireframes (popup, history, settings) | `03-wireframes.md` |
| 4. Interaction and keyboard specification | `04-interaction-keyboard.md` |
| 5. Accessibility and performance acceptance criteria | `05-acceptance.md` |
| 6. Phased implementation plan | `docs/plans/phase-7-ui-redesign.md` |
| 7. Visual mockups (static HTML) | `mockups/index.html` |
| 8. App icon options (owner chose **A**) | `mockups/app-icon.html` |

## Intent

The owner (2026-11-24): the current UI is "too basic, too much text, no icons; it does not show a
modern, minimal product". 0.1.x works but looks like a form: the popup footer is five text buttons,
history has 13 text buttons and three hint lines under the list, settings is a long stock-widget form,
clip kinds are letter codes ("Aa", "Img"), and selection is nearly invisible (1.11:1). The goal is a
modern, minimal, keyboard-first look that makes Dark Clipboard feel like a premium product while
staying as light as today.

## Goals

- Icons instead of words for frequent actions; words for content and explanations.
- One consistent component library and token set for every window.
- Clear selection and focus; every action reachable by keyboard and mouse.
- History with a sidebar, list, detail panel, context menu and bulk bar; works down to 640 px.
- Settings in five sections (General, Capture & Privacy, History & Storage, Data & Transfer, About)
  where privacy protections are visible and understandable.
- Vietnamese and English fit everywhere.

## Non-goals

- No new product features (no Paste Stack, snippets engine, CLI/MCP/AI, sync, light theme).
- No change to capture, storage, privacy rules, clipboard or paste behaviour, data model or Rust API.
- No new framework, no web view, no GPU renderer requirement, no new crates.

## Anti-goals (failure even if it "works")

- Weaker or hidden privacy: masked content shown, exclusion markers made to look optional.
- Slower or heavier: popup p95 or idle memory regresses past `05-acceptance.md` budgets.
- Icon-only mystery meat: an icon without tooltip, accessible name and a key.
- Animations, blur, shadows or glass that the software renderer pays for.

## Constraints

GC-03/06/07 privacy; i18n for every string (EN + VI); GC-19 (agents cannot see or drive the UI:
visual checks are owner rows); dark only (D-037); software renderer default (Task 212); glyph-scan
test (`tests/no_font_glyphs.rs`); phase discipline (this is a new phase, Phase 7); one task, one
branch, one agent; independent review (D-077).

## Approaches (decided by the owner: B)

| | Approach | Trade-off | Verdict |
|---|---|---|---|
| A | Re-skin: new tokens + icons, same layouts | Cheapest (3–4 tasks) but the crowded text footers and three-column history stay; does not answer "too basic" | Rejected |
| **B** | **Rebuild layouts on a shared component library, keep the Rust hosts and models** | 9–10 tasks, each window on its own branch, reviewable and stoppable; small Rust additions (menus, sidebar grouping, new keys) | **Chosen by the owner, 2026-11-24** |
| C | Rewrite the UI layer or change framework | Loses the 7 MiB / 0 % idle footprint that chose Slint (ADR-0001); nothing visual gained | Rejected |

## Decisions made in this design (orchestrator calls; owner may override)

1. **Icons = Lucide SVG files** embedded with `@image-url` and tinted with `colorize`. Verified on
   2026-11-24 with a throwaway Slint 1.19 program on the software renderer: a stroked SVG drew
   correctly tinted (284 tint pixels, 0 off-colour). The Task 414 `Path` icons remain as a fallback.
   Lucide is ISC-licensed (GPLv3-compatible); licence text goes into third-party notices.
2. **Tokens** from the owner's suggestion, plus `raised`, `selected`, `border-strong`, `text-3`,
   `focus`, `warning`, `danger`, `on-accent` (full table and contrast in `02`/`05`). `text-3`
   tuned to `#8F98A8` so it passes 4.5:1 on every surface it is used on.
3. **Popup** 440 × 520 (was 420 × 560): denser 48 px rows (was 68), so ~8 rows visible instead of 6.
   The preview opens to the side (760 px wide) instead of eating 220 px of list height.
4. **No text footer** anywhere: actions live on the selected row, in the detail toolbar, in menus,
   and as keys; a single status line shows capture state and at most three key hints.
5. **F1 shortcuts sheet** replaces the hidden/elided hint lines.
6. **Settings keeps the explicit Save** (validation per field exists); the save bar appears only when
   something changed, with Ctrl+S.
7. **History minimum width 640 px** (was 760) with rail + drawer breakpoints.
8. **Keys only added, never rebound** (`04` lists every new key).
9. Search highlight on the selected row uses text colour + accent underline (accent text on
   `selected` would be 4.39:1).

## Left to the builder (per task package)

Exact Slint structure of each component; whether Slint 1.19 `ContextMenuArea`/`MenuItem` render
under the software renderer (Task 700 checks; fallback `PopupWindow` menu); exact i18n wording of
new tooltip strings (EN + VI, reviewed); Rust changes needed to group `FilterItem`s for the sidebar.

## Expected performance impact

Flat CPU; memory within +2 MiB of baseline (≈ 160 KiB of rasterised icons in the worst case, one
extra pane); binary ≤ +300 KiB. Measured, not assumed: `05-acceptance.md` §2.

## Builder check (2026-11-24) and resolutions

An independent reviewer (fresh agent, sonnet) read this package as a builder. Nothing needed the
owner; all points are settled here as orchestrator calls:

1. **Popup widening.** The list stays where it is; the pane grows to the right. If the widened
   window would cross the work-area edge of the monitor the popup is on, the window moves left just
   enough (the list moves once, only when needed); if the monitor is narrower than 760 px the pane
   opens as an overlay over the list instead. Size/position computed in `popup/place.rs` (pure,
   unit-tested: right edge, left edge, mixed DPI per monitor scale). Both `POPUP_LOGICAL`
   (`place.rs:5`) and the Slint size in `app.slint` change in Task 701/702.
2. **Menus, tooltips and the F1 sheet are in-window overlays** (`PopupWindow` or in-tree layers),
   never separate OS windows, so focus never leaves the popup and "close on focus loss" and the
   paste target are unaffected. Menus are positioned to stay inside the window (open upward /
   leftward near edges). Task 700 checks how Slint clips `PopupWindow` at the window edge.
3. **Keys.** New `Key` variants (F1, F6, Comma, Slash, Digit with Ctrl) are added to the existing
   parser in each key map; the popup and history maps keep their own files as today. Review Focus 4
   means: every existing test keeps its expected action; tests may be added, and the one test that
   asserts `F1 → Key::Other` (`popup/keys.rs:293`) changes by design. F1/F6/Ctrl+, work with the
   search box focused (they type nothing). The existing Ctrl+Backspace-as-Ctrl+H alias stays.
   Settings gets a new pure `settings` key map file with tests (Ctrl+S, Ctrl+1..5, nav arrows,
   Esc). Ctrl+S with a validation error behaves exactly like clicking Save today (errors shown,
   nothing saved).
4. **History sidebar = today's filters regrouped.** Apps (sources), collections, tags, kinds,
   Pinned, Favorites and date filters all exist today (`chips::history_filters`,
   `nav::history_nav_chips`, `history_host.rs:186-203`). Which filters combine stays exactly as
   today (`Filters` model unchanged). Only group collapse (UI state, not persisted) is new.
5. **Paste.** No new paste path: "Paste into app" is **not** in the row menu (mockup updated); the
   status-line hints follow the same rule as today's key hints (with "Enter pastes" on, the Enter
   hint says Paste and the Ctrl+Enter hint is omitted).
6. **Toast placement** (one rule everywhere): floats above the status line, inside the list area.
7. **F6 at narrow widths** cycles only visible regions: rail → list (→ drawer when open).
8. **Popup "new" Menu key**: `Key::Menu` is parsed today with no action; 701 gives it the row menu.
9. **History minimum size**: the remembered per-monitor size (`history/place.rs`) gets the new
   640 × 460 lower bound in Task 704.
10. **Icons**: the exact list is fixed in Task 700's package (the design names 42; budget P6
    assumes up to 50 files). SVGs are vendored unmodified from a pinned Lucide release with its
    LICENSE.
11. **Sequencing**: Phase 5 is complete (D-074); Phase 7 starts after the in-flight Task 417.
