# Task 700: UI foundation kit (tokens, Lucide icons, `ui/kit/` components)

**Agent:** `builder` · **Model:** sonnet · **Branch:** `task/700-ui-foundation-kit` (from `main`) · **Review:** `reviewer` (sonnet)
**Global Constraints:** GC-08, GC-11, GC-12, GC-14, GC-15, GC-17, GC-18, **GC-19**. **Source:** `docs/plans/phase-7-ui-redesign.md` (Phase 7, D-078). **Review Focus:** 4 (no key map changes), 5 (P4/P6 baselines measured, not estimated), 6 (every new string in i18n EN + VI; glyph scan passes).
Foundation only: **no window layout is rebuilt** (701+ do that). Every user-visible string comes from i18n (EN + VI).

## Read first
`CLAUDE.md`, `AGENTS.md`, `docs/specs/ui-redesign/README.md` ("Builder check resolutions"), `02-design-system.md` (§2 tokens, §5 icons, §6 components), `05-acceptance.md` (A1, A9, P3, P4, P6), `01-audit.md` §1.2, §2, §7, §8, `docs/tasks/414-icon-glyphs.md`, `docs/tasks/212-renderer-memory.md` (measurement method), `apps/desktop/src/theme.rs`, `ui/theme.slint`, `ui/icons.slint`.

## Requirements
1. **Tokens.** In `apps/desktop/src/theme.rs` (single source of truth) and the `Theme` global in `ui/theme.slint` add: canvas, surface, raised, selected, border, border-strong, text, text-2, text-3, accent, on-accent, focus, success, warning, danger, with the hex values of `02` §2. Old names (bg, line, text-muted, ...) stay as **aliases** mapped to the new values so every existing window compiles and matches the palette; state in the report which alias maps to which token. No window markup is edited except for item 4.
2. **WCAG test.** Extend the test in `theme.rs` with every pair of `05` §1 A1 (text, text-2, text-3, accent, danger, success, warning, focus on canvas/surface/selected as listed there; on-accent on accent; `text-3` not asserted on raised; accent on selected asserted only as >= 3:1 because it is icon-only). Written first, seen failing.
3. **Lucide SVGs.** Vendor the icons of `02` §5 "Icon map" **unmodified** from one pinned Lucide release into `apps/desktop/ui/icons/` (official lucide GitHub release or npm `lucide-static` only). Add `ui/icons/README.md` (version, source URL, icon list) and `ui/icons/LICENSE` (ISC text). Add the Lucide attribution to the third-party notices: find where licences are listed today (`about.hbs`, `about.toml`, `scripts/release-notices.ps1`; check which exist) and extend it. If downloading is impossible, **stop and report**; never draw icons by hand.
4. **Kit** in `ui/kit/` per `02` §6: `Icon` (name -> `@image-url`, `size`, `tint` via `colorize`), `IconButton` (FocusScope, 28x28, icon 16, 2 px `focus` ring, hover/active/danger, tooltip, `accessible-label` + description), `Button` (primary/secondary/ghost/danger), `KeyCap`, `Tooltip` (500 ms hover and on keyboard focus; one at a time; text from i18n), `Switch` (32x18, checkbox role), `Menu`/`MenuItem` (keyboard navigable, in-window overlay, never an OS window). Replace the Task 414 `Path` icons (`ui/icons.slint`) where used today (popup gear and close, favorite star, settings lock) with the new `Icon`; delete `icons.slint` if nothing uses it. `tests/no_font_glyphs.rs` passes unchanged.
5. **Menu investigation.** Check whether Slint 1.19 `ContextMenuArea`/`Menu` renders under the software renderer and how `PopupWindow` clips at the window edge (throwaway program outside the repo, or a test on Slint's headless backend; no window shown, GC-19). Record the result and the choice for 701/704 in `memory-bank/decisions.md` (new D-number) and in the report.
6. **i18n.** Tooltip strings for the existing icon buttons (settings, close, favorite, ...) as new `Msg` keys, EN + VI, in `crates/darkclip-core/src/i18n.rs`.
7. **Baselines (P4, P6).** Before any change and after: private bytes after opening popup, history and settings once each, and release exe size. Follow the Task 212 method (release build, temp `--data-dir`, `--exit-after-ms`, no screenshots). Write exact commands and raw numbers to `docs/testing/ui-redesign-baseline.md`. Budgets: P4 <= baseline + 4 MiB, P6 <= + 300 KiB; report if exceeded.
8. **Checklist.** Owner rows in `docs/testing/platform-checklist.md`: icons crisp at 100/150/200 %; focus ring visible on every kit control; tooltips on hover and on keyboard focus.
9. **CHANGELOG.** `[Unreleased]` -> Added: new design tokens and icon set (no layout change yet).

## Files this task may edit
`apps/desktop/src/theme.rs`, `apps/desktop/ui/theme.slint`, `apps/desktop/ui/kit/*` (new), `apps/desktop/ui/icons/*` (new), `apps/desktop/ui/{app,popup,organize,settings}.slint` (icon swaps and alias fixes only), `apps/desktop/ui/icons.slint` (delete), `apps/desktop/tests/*` (new kit tests; `no_font_glyphs.rs` only to add `ui/kit` to its scanned files), `apps/desktop/src/ui/*` and `src/popup/row.rs` only if an icon swap needs it, `crates/darkclip-core/src/i18n.rs`, licence notice files, `docs/testing/*`, `memory-bank/{decisions,techContext}.md`, `CHANGELOG.md`. Nothing else.

## Tests (first, seen failing)
- WCAG pairs (item 2).
- Every `.svg` referenced from `ui/kit` exists, and every file in `ui/icons/` is referenced (no dead icons).
- Kit keyboard behaviour testable as pure Rust key maps (menu up/down/home/end/enter/esc; tooltip show rules as a pure function if extracted).
- i18n completeness holds; all existing tests pass unchanged. Never run `#[ignore]` tests.

## Acceptance
A1, A9; `Icon`/`IconButton`/`Button`/`KeyCap`/`Tooltip`/`Switch`/`Menu` exist and compile; existing windows compile with the new palette; P4/P6 recorded; `cargo fmt --all`, then `powershell -File scripts/check.ps1` prints `ALL CHECKS PASSED`.

## Known pitfalls (from `memory-bank/systemPatterns.md`)
- **UI icons are never symbol glyphs** (Task 414): the software renderer has no font fallback; `tests/no_font_glyphs.rs` scans `ui/*.slint`, `src/**/*.rs` and `i18n.rs` (allow-list: arrow right only). New `ui/kit/*.slint` files must be covered by the scan; do not weaken it.
- **Windows-only code must lint on macOS/Linux** (CI runs clippy `-D warnings` there): check every `cfg(windows)` you add for what stays unused elsewhere.
- **Footer width budget** (`tests/footer_budget.rs`) must keep passing: do not change popup padding, button padding or footer labels here.
- **Software renderer (D-055):** no blur, shadows, gradients or GPU features; no timer or animation running while a window is hidden (idle CPU stays 0 %). The tooltip delay timer runs only while hovered or focused.
- **Single colour source:** colours live in `theme.rs`; no new hex literals in `.slint` outside `Theme`.
- **Keep `Palette.color-scheme = dark`** in each window `init` while any std-widget remains.
- **No unwrap/expect** in non-test code (`panic = "abort"`); identical EN/VI strings go in `SAME_IN_BOTH`.
- **Run the app only with `--data-dir <temp>` and `--exit-after-ms`** (GC-19); visual checks are owner rows.

## Not doing
Window layouts, new keys, popup size change, app icon (Task 710), new crates.
