# Task 711: Menu anchors at the window origin; capture-mode cards fill the row (owner report 2026-11-25)

**Agent:** `builder` · **Model:** sonnet (UI layout, low risk, two-line root causes known) · **Branch:** `task/711-popup-anchor-modecard` (from `main` 28902d5) · **Review:** `reviewer` (sonnet) · **Risk:** Low
**Global Constraints:** GC-03, GC-06, GC-19. **Source:** owner bug report 2026-11-25 with three screenshots (Phase 7 build).
Scope: the two layout bugs below and a regression scan. **No behaviour change, no key change, no new feature.**

## Owner report
1. Settings → Capture and Privacy → the three capture-mode cards (Recording / Private / Paused) are drawn narrow and content-sized (~80-90 px each, left-aligned); titles elide ("Đa..."), the line text wraps one word per line and overflows the card and the "current mode" note below.
2. Popup: clicking a row's "..." (more) button opens the row menu near the bottom-right of the window, not under the row.
3. History: right-clicking a row (or Menu key) opens the row menu at the right edge, far from the pointer.

## Root causes (found by the orchestrator; confirm, do not re-investigate from scratch)
- **Menus (2, 3).** `ui/kit/menu.slint` `Menu` is a 0×0 anchor; its `PopupWindow` uses `x: root.px; y: root.py`, i.e. coordinates relative to the anchor. Every host instance (`ui/app.slint` `menu := Menu`, `ui/history.slint` `date-menu` and `row-menu`, `ui/kit/gallery.slint` `menu`) is a direct child of the `Window` **without `x`/`y`**, and Slint centres such a child in its parent. The anchor therefore sits at the window centre, and every `open(x, y)` (computed as window positions: `absolute-position`, pointer position) is shifted by half the window size; Slint then clamps the popup back inside the window, so it lands at the bottom/right edge. The date menu has the same offset.
- **Mode cards (1).** `ui/settings_parts.slint` `ModeCard`: the `Icon` (fixed `width: 18px`) is a direct child of the card's `VerticalLayout`; a fixed-width child caps the layout's cross-axis max width (≈ 18 + 24 padding), so the card's max width collapses to its minimum and `horizontal-stretch: 1` cannot widen it.

## Requirements
1. Put the menu anchor at the window origin: add `x: 0px; y: 0px;` to every `Menu` instance (app, history ×2, gallery), or set it once in the kit `Menu` if Slint 1.19 accepts `x`/`y` on a component root (check; prefer the kit if it compiles and is honoured). Update the `Menu` header comment: positions are window positions **because** the anchor is at (0,0).
2. Fix `ModeCard` so the three cards share the row equally: wrap the `Icon` so its fixed width no longer caps the layout (e.g. `HorizontalLayout { alignment: start; Icon {...} }` or a `Rectangle { height: 18px; Icon { x: 0; y: 0; } }`; pick one that Slint actually lays out with unbounded max width). Check the welcome window's mode cards (`ui/welcome.slint`) and any other card/row that has the same fixed-size-child-in-VerticalLayout pattern **where it visibly matters** (a stretched container that renders content-sized); fix only real cases, list them in the PR.
3. **Regression test** (new or in `tests/kit.rs`, same simple text-scan style; failure names file and line): (a) every `Menu {` instance in `ui/**/*.slint` (or the kit definition) sets `x: 0px` and `y: 0px`; (b) in `ModeCard`, no `Icon` is a direct child of a `VerticalLayout`. If a geometry test through Slint's testing backend is cheap (instantiate `SettingsWindow`, read a card's width), prefer it for (b), else the scan.
4. `docs/testing/platform-checklist.md`: add owner rows (**untested**): popup row "..." menu opens under the row; popup right-click opens at the pointer; history right-click / Menu key / Shift+F10 open at the pointer / under the row; history date menu under the date button; settings mode cards fill the row in EN and VI at 100/150 %.
5. `CHANGELOG.md` `[Unreleased]` → Fixed: two lines (menus open at the wrong place; capture-mode cards too narrow).

## Files this task may edit
`apps/desktop/ui/kit/menu.slint`, `ui/app.slint`, `ui/history.slint`, `ui/kit/gallery.slint`, `ui/settings_parts.slint`, `ui/welcome.slint` (only if requirement 2 finds the same bug), `apps/desktop/tests/kit.rs` or a new `apps/desktop/tests/layout_scan.rs`, `docs/testing/platform-checklist.md`, `CHANGELOG.md`, `memory-bank/systemPatterns.md` (one line each: "Slint centres a child of Window without x/y: set x/y 0 on 0×0 anchors"; "a fixed-size child caps a layout's cross-axis max").

## Known pitfalls
- Do not touch the Rust host code (`src/ui/*_host.rs`) positions: they are already window positions and become right once the anchor is at (0,0). If any host computes a position relative to the anchor's old place, report it rather than adding offsets.
- `history.slint` subtracts `self.absolute-position` (the window root, 0): leave it.
- No `unwrap`/`expect`; never run `#[ignore]` tests; no screenshots or input simulation (GC-19).

## Acceptance
Scan/geometry tests red before the fix, green after; `cargo fmt --all`, then `powershell -File scripts/check.ps1` prints `ALL CHECKS PASSED`.

## Not doing
Menu restyling, edge logic in Rust, new i18n strings (mode card texts stay as they are).
