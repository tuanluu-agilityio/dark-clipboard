# Task 714: Favorite star hidden on the selected popup row

**Agent:** `builder` · **Model:** sonnet · **Branch:** `task/714-row-favorite-badge` (from `main`) · **Review:** `reviewer` (sonnet)
**Global Constraints:** GC-14, GC-19. **Source:** owner report 2026-11-25 with screenshot: after "Yêu thích" in the row menu, the menu shows "Bỏ yêu thích" (the clip is a favorite) but the row shows no star.

## Root cause
`apps/desktop/ui/popup_row.slint` (~201-240): the badges (favorite star, masked eye-off, pin, Alt+N number) are drawn only when `!root.show-actions`; `show-actions` is true while the row is selected or hovered, and the row actions (copy, pin, more) take that place. Pin state stays visible through the pin / pin-off action icon, but favorite and masked have no action, so their badges vanish on the selected row (the one the user just changed). The history row (`ui/history_row.slint`) always shows its badges and is not affected.

## Requirements
1. While the row actions show, keep the **favorite** star and the **masked** eye-off visible, immediately left of the action buttons (same icons, 14 px, same tints as the badges), without overlapping the meta text (reserve their width only when present). The pin badge stays replaced by the pin action, and the Alt+N number may stay hidden as today.
2. The row's accessible description is unchanged (it already contains the favorite and masked words).
3. Tests (first, seen failing): a scan test over `ui/popup_row.slint` that the favorite and masked badges are not gated only by `!root.show-actions` (both states render them), and the popup width budget (`tests/popup_budget.rs`) still holds with the extra icons on the selected row at 440 px in EN and VI (meta line keeps at least the width it has today minus 2 × (14 + 6) px; adjust the budget constants honestly if needed). If practical, a headless render check in the style of `tests/menu_click.rs` (MinimalSoftwareWindow, no visible window) that the star's pixels appear on a selected favorite row; skip with a note if it needs more than two attempts.
4. Owner checklist row: a favorite clip shows its star whether its row is selected, hovered or neither; the same for a masked clip's eye-off.
5. CHANGELOG `[Unreleased]` Fixed.

## Files this task may edit
`apps/desktop/ui/popup_row.slint`, `apps/desktop/tests/**` (new scan/budget/headless tests), `docs/testing/platform-checklist.md`, `CHANGELOG.md`.

## Acceptance
Tests above pass and were seen failing first; all existing tests pass; `cargo fmt --all`, then `powershell -File scripts/check.ps1` prints `ALL CHECKS PASSED`.

## Known pitfalls
- No hex colours in `.slint` outside `Theme`; icons are SVG via the kit `Icon`, never glyphs.
- Integration-test `#[path]` modules only at top level (F-37); tests must build on Linux/macOS CI.
- `set_platform` once per test binary for headless tests.
- Never run the app with a visible window except with `--data-dir <temp>` and `--exit-after-ms` (GC-19).
