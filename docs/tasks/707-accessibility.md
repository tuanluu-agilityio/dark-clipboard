# Task 707: Accessibility pass (label/role scan, hit targets, focus return, tooltip on focus, Narrator text, remaining width budgets)

**Agent:** `builder` · **Model:** sonnet · **Branch:** `task/707-accessibility` (from `main`, after 701-706) · **Review:** `reviewer` (sonnet) · **Risk:** Low
**Global Constraints:** GC-03, GC-06, GC-14, GC-18, **GC-19**. **Source:** `docs/plans/phase-7-ui-redesign.md` (row 707, Review Focus 1, 6), `docs/specs/ui-redesign/05-acceptance.md` A3-A7, D-082.
Scope: tests plus the smallest fixes they force. **No behaviour change, no key change, no new feature.**

## Read first
`CLAUDE.md`, `05-acceptance.md` (A3, A6, A7, section 4), `04-interaction-keyboard.md` section 1 (rules 2, 3, 7) and section 5, D-079, D-082, D-083 in `memory-bank/decisions.md`, then `apps/desktop/tests/{kit,popup_budget,history_budget,settings_budget,settings_sections,no_font_glyphs}.rs` (scan style, helpers to reuse), `ui/kit/*.slint`, `ui/{popup,history,settings,welcome,shortcuts}.slint`, `ui/settings_parts.slint`.

## Requirements
1. **A3 scan** (new `apps/desktop/tests/a11y_scan.rs`): parse every `ui/**/*.slint` (not `kit/gallery.slint`); for each instance of `IconButton`, `Button`, `NavItem`, `Switch`, `Select`, `Field`, menu item rows, `ModeCard` (capture-mode cards) and checkbox-role elements, assert it binds a non-empty `accessible-label` (a property or `Strings`/`SText` binding, never a literal) and the component definition has `accessible-role`. Allowlist = explicit, each entry with a reason (decoration, always-protected rows: GC-07, not focusable). Fix every miss in `.slint`; add `Msg` keys EN + VI only where a label truly is missing.
2. **A6 scan**: every `IconButton` and row-action instance is >= 28 x 28 px (read the size constants and instance overrides); nothing interactive anywhere is < 24 px in either dimension. Fix by changing the size or hit area, not the icon.
3. **Focus return** (D-082): after a settings `ConfirmDialog`/running-app picker or the history/popup `Menu` closes, focus returns to the opener. Where openers sit in `for` loops and cannot be referenced, record the opener index in a property and focus through an index property in the loop; if Slint 1.19 cannot do it, leave focus on the window's root scope and write the limit in `docs/testing/platform-checklist.md` ("known limit", row below). Do not change which keys do what.
4. **Tooltip on keyboard focus**: every focusable `IconButton` shows its tooltip when focused (same `TooltipHub` path as hover, rule 3). Test: scan that the kit `IconButton` raises the hub on `has-focus` and every instance has a non-empty tooltip text; every window has one `TooltipLayer` as the last root child.
5. **Narrator text**: unit tests in the popup and history row modules (pure Rust that builds `label`/`description`): the description contains kind, age, source and badge names (pin, star, lock, masked) in EN and VI; **for a masked clip it never contains any character sequence of the content or stored preview** (Review Focus 1; use a distinctive secret string). Do not change the builders unless a test fails.
6. **Remaining width budgets (A7)**: new `apps/desktop/tests/secondary_budget.rs` (style of `popup_budget.rs`, EN and VI, 7 px/char, nothing loosened) for welcome (buttons, mode cards), the F1 shortcuts sheet (key column and action column), the Copy-as and Edit panels' button rows, and the org dialogs' buttons (collection, tag, note). Fix overflow by shortening the i18n text (VI first) or widening the container, not by shrinking the font.

## Files this task may edit
`apps/desktop/tests/{a11y_scan,secondary_budget}.rs` (new), `apps/desktop/ui/**/*.slint` (fixes only), `apps/desktop/src/{popup,history,ui}/*.rs` (only a pure Narrator-text fix or focus-index property wiring), `crates/darkclip-core/src/i18n.rs` (new keys), `docs/testing/platform-checklist.md` (rows), `CHANGELOG.md` (`[Unreleased]` Changed: accessibility labels and focus fixes), `memory-bank/systemPatterns.md` (one line: the scan tests).

## Owner rows (add to `platform-checklist.md`, **untested**)
Tab walk per window (popup, history, settings, welcome, F1 sheet, dialogs) with the ring on each control and no trap; tooltip appears on Tab focus of each icon button; after closing a dialog or menu focus is back on the opener (or the documented limit); Narrator reads buttons, switches, row label + description, dialogs; Vietnamese pass of welcome, F1 and dialogs at 100/150/200 %.

## Known pitfalls
- Scan the `.slint` text with the same simple parsing as `tests/kit.rs`; no parser crate. Failure messages name file and line.
- Instances inside `for` need the label bound from the model row or index, not left empty.
- `accessible-label` is the action name, `accessible-description` the shortcut (rule 3); never put clip content in either.
- Elements inside a `PopupWindow` cannot be referenced from outside (D-079); focus return goes through properties.
- No `unwrap`/`expect`; never run `#[ignore]` tests; no screenshots or input simulation (GC-19).

## Acceptance
A3, A6 and A7 automated parts green; A5/A8 only as owner rows; `tests/no_font_glyphs.rs` passes; `cargo fmt --all`, then `powershell -File scripts/check.ps1` prints `ALL CHECKS PASSED`.

## Not doing
New components, restyling, key changes, contrast changes (A1 done in 700), real Narrator or Tab-walk runs.
