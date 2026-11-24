# Task 414: Icons that do not depend on font glyphs; popup footer fits

**Agent:** `builder` · **Model:** sonnet · **Branch:** `task/414-icon-glyphs` (from `main`) · **Review:** `reviewer` (sonnet)
**Global Constraints:** GC-14, GC-18, **GC-19**. **Source:** owner report 2026-11-24 on 0.1.0: in the popup, the settings and close buttons right of the search box show nothing. A screenshot from the owner shows empty space where the buttons are.

## Cause
`apps/desktop/ui/app.slint:168` (`"⚙"`, U+2699) and `:178` (`"✕"`, U+2715) draw the buttons as text. The Windows UI font has no glyph for them, and the software renderer (Task 212) does not fall back to a symbol font, so the buttons are invisible though still clickable. Other places carry the same risk:
- `ui/organize.slint:81` and `src/popup/row.rs:99`: `★` (U+2605) favorite badge and star.
- `ui/settings.slint:513`: `🔒` (emoji).

## Requirements
1. **A shared `Icon` component** (in `popup.slint` or a new `icons.slint`) draws each icon as vector `Path` commands, not text. It takes a size and a colour bound to the theme, including the hover colour where the button already has one.
   - Icons: settings (gear), close (X), star (filled), lock.
   - Keep them simple and legible at 14–16 px on a dark background.
2. **Replace the glyphs:**
   - The popup settings and close buttons. Add an icon variant of `ActionButton`, or a separate `IconButton`. The size stays 32×36, and `accessible-label` stays unchanged.
   - The favorite star in the organize strip, and the row favorite badge. The row currently gets the badge text from Rust (`favorite_badge`). Change it to a `bool` and draw the icon in Slint. Update the tests.
   - The lock in Settings > Privacy: draw it as an icon before the label text.
   - Leave the other symbols as they are: `×` (U+00D7, in Segoe UI), `·` and `→` in the key hints (`→` U+2192 is in Segoe UI). Record that choice in the test allow-list below.
3. **Regression test.** Add a test (desktop crate, not ignored) that scans every `apps/desktop/ui/*.slint` file and the UI string sources (`apps/desktop/src/**/*.rs`, `crates/darkclip-core/src/i18n.rs`). It fails on any character, literal or `\u{...}` escape, in U+2190–U+2BFF or U+1F000 and above, except an explicit allow-list (`→`). Comments are excluded. The test message names the file and line.
4. **Checklist.** Add owner rows to `docs/testing/platform-checklist.md`:
   - the popup settings and close icons are visible at 100/150/200 % scaling;
   - the favorite star shows in rows and in the organize strip;
   - the lock shows in Settings > Privacy.
5. **CHANGELOG:** add an `[Unreleased]` → Fixed entry for the invisible popup buttons in 0.1.0.

6. **Popup footer must not be cut off (owner request 2026-11-24).**
   - **Cause.** The footer in `ui/app.slint:287-329` is one `HorizontalLayout` holding Copy, Copy plain, Copy as…, Pin and Delete. Each button's `min-width` equals its label width, and the panel clips. In Vietnamese ("Sao chép", "Sao chép dạng văn bản thuần", "Sao chép dưới dạng…", "Ghim", "Xóa") the row is wider than the popup, so the last buttons are cut off ("Ghi…" in the owner's screenshot). The toast row and Undo need the same care.
   - **Fix.** Every action stays reachable and visible in both languages at the popup's width:
     - Use short footer labels: new i18n keys for en and vi (for example "Văn bản thuần", "Chuyển đổi…"). The full text stays in `accessible-label` and in the key hints.
     - Labels may shrink with `overflow: elide` as a last resort. A button is never pushed out of view.
     - Pick the smallest change that guarantees this, and say what you chose.
   - **Test.** Add a test that fails on the current Vietnamese labels. Two acceptable forms:
     - Slint's headless testing backend, if it is already available as a dev-dependency or cheap to add (no window shown, no screenshot, GC-19), measuring the footer's preferred width against the popup width in both languages.
     - Otherwise, a measured character budget per footer label, documented in the test.
   - **Checklist.** Add an owner row: every footer button is fully visible in English and in Vietnamese.

## Tests
- All existing tests pass, plus the scan test above, written first and seen failing on the current tree.
- Do not run `#[ignore]` tests.
- Never launch the UI, screenshot or simulate input (GC-19). The owner checks the look.
- CI also lints macOS and Ubuntu; see `memory-bank/systemPatterns.md`.

## Not doing
Bundling a font, an icon font or image files. A release: the orchestrator decides 0.1.1 separately.
