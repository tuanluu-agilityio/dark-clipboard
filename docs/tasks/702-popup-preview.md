# Task 702: Popup preview side pane (760 px, placement, pane states)

**Agent:** `builder` · **Model:** sonnet · **Branch:** `task/702-popup-preview` (from `task/701-popup-layout`, or `main` once 701 is merged) · **Review:** `reviewer` (sonnet) · **Risk:** Low
**Global Constraints:** GC-03, GC-06, GC-08, GC-11, GC-14, GC-15, GC-17, GC-18, **GC-19**. **Source:** `docs/plans/phase-7-ui-redesign.md` (row 702), D-078, D-079. **Review Focus:** 1 (masked: no content in the pane, tooltip or accessible text), 4 (every popup key keeps its action; only Esc gains a layer), 6 (EN + VI for every new string).
Scope: the popup window only. After this task the popup is complete and shippable alone.

## Read first
`CLAUDE.md`, `docs/specs/ui-redesign/README.md` (builder-check 1 widening/placement, 2 overlays), `03-wireframes.md` §1, `04-interaction-keyboard.md` §1.4 and §2, `05-acceptance.md` (A3, A6, A7), `mockups/index.html` section 2, `docs/tasks/701-popup-layout.md`, `memory-bank/systemPatterns.md`, D-079, then the code 701 produced: `ui/app.slint` (`detail-open`, the `DetailPane` block, `Window` width/height), `ui/popup.slint` (`DetailPane`, `DetailState`), `src/popup/{place,detail,keys}.rs`, `src/ui/popup_host.rs` (`show`, `set_detail_open`, `render_detail`, `Action::Close`), `src/winsys.rs` (`monitor_at`, `monitor_at_cursor`), `tests/popup_budget.rs`.

## Requirements
1. **Placement (pure, `place.rs`).** Add `PREVIEW_LOGICAL_WIDTH: u32 = 760`, `pub enum PaneMode { Side, Overlay }` and
   `pub fn widen(origin: (i32, i32), list_w: i32, wide_w: i32, work: Rect) -> ((i32, i32), PaneMode)`
   (physical pixels; the caller scales 440 and 760 with the DPI of the monitor the window is on via `scale`, so each monitor uses its own scale). Rules: `wide_w > work width` (monitor narrower than 760 logical px) gives `(origin, Overlay)` (window keeps `list_w`, the pane draws over the list); otherwise `x' = min(origin.x, work.right - wide_w)`, never below `work.left`, result `((x', y), Side)`. The list's left edge stays put unless the shift is needed. Closing restores the width but not the position (no second jump). `y` never changes. Saturating arithmetic.
2. **Window resize (`popup_host.rs`).** The window is frameless: opening calls `w.window().set_size(PhysicalSize)` to `scale(760, dpi)` x `scale(520, dpi)` and `set_position(PhysicalPosition)` from `widen`; no `hide`/`show`. Monitor and DPI come from `winsys::monitor_at(window x, y)` (not the cursor: the window may have moved), `cfg(windows)` as in `show`; elsewhere only the Slint width changes. In Slint the root `width` is `root.detail-open && !root.pane-overlay ? 760px : 440px` (new `in property <bool> pane-overlay`); the transparent background and the rounded 10 px panel are children sized `parent.width`, so the panel follows. Resize never calls `focus()`, never hides or shows the window and never recreates the search `TextInput`: close-on-focus-loss is not triggered and the caret and typing are undisturbed. Set the position before the size when growing leftwards so no frame shows the wide window at the old origin (the OS may reorder; owner row). `end_session`/`hide` closes the pane and restores 440 so the next `show` starts compact.
3. **Layout.** Left column = 701 list column, 440 wide, full 520 height (the 228 px `Rectangle` below the list in `app.slint` is deleted; the list no longer shrinks). Right column 320 px: new `PreviewPane` in `popup.slint` (replaces the `DetailPane` use; remove `DetailPane` unless history imports it), 1 px `border` divider. In `Overlay` mode the pane is 320 px at `x: parent.width - 320px - 12px` over the list, `surface` background.
4. **Pane content** (03 §1), tokens only, from the existing `DetailState`: header 40 px with kind `Icon` (accent), title (13 px, 600, one line, elided), two `IconButton`s: plain text (`type`, same as Shift+Enter) and transform (`wand`, same as Ctrl+T, hidden when `!can_transform`); scrolling content: text (mono for code, wrapped, existing "load full text"), image (fit, existing "load full image"), files (`file_entries` list, "+n more"), **masked** (no content, only the existing i18n masked note, no buttons, facts show the kind only); facts block (label `text-3` 11 px, value `text` 12 px): Source, Time (absolute local date-time via the offset helper), Size (characters / pixels + bytes / file count). Pure `facts(item, lang, offset) -> Vec<(Msg, String)>` in `popup/detail.rs`.
5. **Header toggle.** The 701 preview `IconButton` shows `active` while open (bound to `detail-open`; no second state).
6. **Keys (pure, `keys.rs`).** `KeyCtx` gains `detail_open: bool`. `Key::Escape` with `detail_open` gives the existing `Action::CloseDetail`, else the unchanged `Close`; an open menu still consumes Esc first. Space, Right, Left and every other arm stay untouched (Review Focus 4).
7. **i18n.** New `Msg` keys EN + VI: facts labels (Source, Time, Size), size strings (`{n} characters`, `{w} x {h} px`, `{n} files`), tooltips for the two pane buttons if 701 has none to reuse. Identical EN/VI strings go in `SAME_IN_BOTH`. No literal in `.slint`.
8. **Docs.** `docs/testing/platform-checklist.md` rows below; `CHANGELOG.md` `[Unreleased]` Added: preview side pane (760 px, list keeps full height), Esc closes the preview first.

## Files this task may edit
`apps/desktop/ui/{app,popup}.slint`, `apps/desktop/src/popup/{place,detail,keys}.rs`, `apps/desktop/src/ui/{popup_host,common}.rs`, `apps/desktop/src/winsys.rs` (only if `monitor_at` lacks a field), `apps/desktop/tests/popup_budget.rs`, `crates/darkclip-core/src/i18n.rs`, `docs/testing/platform-checklist.md`, `CHANGELOG.md`. Nothing else.

## Tests (first, seen failing)
- `place.rs`: `widen` at 100 % on a 1920 work area: mid-screen cursor (x unchanged, Side); right edge (x = `right - 760`); second monitor with negative coordinates at its left edge (never below `left`); exactly 760 wide (Side, x = left); narrower than 760 (Overlay, origin unchanged); 150 % and 200 % (sizes via `scale`, same outcomes); **mixed DPI**: a 100 % monitor beside a 200 % one, each computed from its own `dpi` and `work`; `y` never changes; no overflow with `i32::MAX` inputs.
- `keys.rs`: Esc with `detail_open` gives `CloseDetail` in TYPING, FRESH, LIST contexts; without it (including chip/control focus) still `Close`; the 701 table of existing keys passes unchanged (only `detail_open: false` added).
- `detail.rs`: `facts` for text, image, files, code; **masked: only the kind** (assert no source, no size, nothing from the item preview).
- `popup_budget.rs`: pane header (icon, title min 120 px, two 28 px buttons) and facts rows fit 320 - 24 px in EN and VI.
- i18n completeness and `no_font_glyphs.rs` pass; no unwrap/expect; never run `#[ignore]` tests.

## Owner checklist rows (add to `platform-checklist.md`, **untested**)
Open/close the preview (Space, Right, header button) at 100 %, 150 % and 200 %: list stays, no flicker, search caret keeps blinking and typing continues, rounded transparent corners follow; popup at the right screen edge shifts left once and stays; popup on a second monitor with a different scale widens correctly; monitor narrower than 760 logical px: pane overlays the list; Esc closes the preview first, the second Esc closes the popup; masked clip shows only the masked note; image, file list and long text panes; Vietnamese pass; Narrator reads pane title and facts.

## Known pitfalls
- **D-079:** a `PopupWindow` takes every key while open; menus stay in-window; the pane is an in-tree layer, never a `PopupWindow`; `TooltipLayer` stays the last child of the window root. Menus near the new right edge need no edge logic.
- **Resize without focus change:** no `show()`/`hide()`, no focus move; the focus-loss timer must not see a deactivation.
- Windows-only code must lint on macOS/Linux (`cfg(windows)` leaves items unused elsewhere). Software renderer (D-055): no blur, shadow, gradient, no timer or animation while hidden. UI icons are `Icon`s, never glyphs. Single colour source (`theme.rs`). Previews, sources and paths never logged; `Debug` of new types prints no content (GC-06).
- Run the app only with `--data-dir <temp>` and `--exit-after-ms`; no screenshots, no input simulation, no real clipboard (GC-19).

## Acceptance
Placement tests and Review Focus 1, 4, 6 proven by the tests above; A3, A6, A7 for the pane; `cargo fmt --all`, then `powershell -File scripts/check.ps1` prints `ALL CHECKS PASSED`.

## Not doing
New icons or crates; History/Settings; Copy as panel restyle (706); persisting pane state; animating the resize; any new paste path.
