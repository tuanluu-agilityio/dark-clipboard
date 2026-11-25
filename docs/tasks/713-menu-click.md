# Task 713: Menu items do nothing on click; highlight does not follow the mouse

**Agent:** `builder` · **Model:** sonnet · **Branch:** `task/713-menu-click` (from `main`) · **Review:** `reviewer` (sonnet)
**Global Constraints:** GC-14, GC-19. **Source:** owner report 2026-11-25 with screenshots: in the popup row menu and the history context menu, clicking an entry does nothing; the highlight stays on the first entry while the pointer is over another one.

## Root cause (reproduced by the orchestrator with a headless probe)
1. `apps/desktop/ui/kit/menu.slint`, `MenuItem` `activated` handler in `Menu`: it calls `pop.close()` **before** `root.activated(i)`. Closing the `PopupWindow` destroys the repeated `MenuItem` whose handler is running, so `root.activated(i)` never runs. Probe (software renderer, `MinimalSoftwareWindow`, pointer press/release on entry B): original order → `activated` never called; `root.activated(i)` first, then `pop.close()` → `activated(1)`. Keyboard Enter is unaffected (it goes through Rust `menu_key` → `activate_menu`).
2. `MenuItem`'s `TouchArea` reports hover through `moved`, which Slint only fires while a button is pressed, so `hovered` never reaches the host and the highlight never follows the pointer (same probe: no `hovered` call on a plain move).

## Requirements
1. In `Menu`, call `root.activated(i)` first, then close the popup for non-submenu entries. Keep "a submenu entry leaves the menu open".
2. Report hover when the pointer enters an entry (e.g. `changed has-hover` on the `TouchArea`, or `pointer-event` move), so the host highlight follows the mouse. No timer.
3. Check every other place where a handler closes a `PopupWindow` (or destroys its own element through an `if`) before calling out (grep `pop.close()`, `.close();` in `ui/**/*.slint`) and fix the same ordering bug if present; list what you checked.
4. **Regression test (headless, no visible window, GC-19):** a new integration test `apps/desktop/tests/menu_click.rs` that builds a tiny component with `slint::slint!` importing `../ui/kit/menu.slint`, uses `slint::platform::software_renderer::MinimalSoftwareWindow` with a custom `Platform`, opens the menu, dispatches `PointerMoved` / `PointerPressed` / `PointerReleased` on the second entry and asserts (a) `activated(1)` was called exactly once, (b) `hovered(1)` was called on the move, (c) a submenu entry click calls `activated` and leaves the popup open. Seen failing on the current code first. Note: `slint::platform::set_platform` can be called once per process, so keep all cases in one `#[test]` or one test binary with a shared platform. Probe code for reference: the orchestrator's scratch probe used `w.window().dispatch_event(WindowEvent::PointerPressed { position: LogicalPosition::new(60.0, 56.0), button: PointerEventButton::Left })` with the menu opened at (10, 10), padding 4, rows 28 px, and `slint::platform::update_timers_and_animations()` + `draw_if_needed` between steps (`use slint::platform::WindowAdapter;` for `.window()`).
5. Owner checklist row: clicking each entry of the popup row menu, popup More filters, history row menu, history date menu runs it; the highlight follows the mouse.
6. CHANGELOG `[Unreleased]` Fixed.

## Files this task may edit
`apps/desktop/ui/kit/menu.slint`, other `apps/desktop/ui/**/*.slint` only for requirement 3, `apps/desktop/tests/menu_click.rs` (new), `apps/desktop/Cargo.toml` only if the test needs a dev-dependency feature (prefer none), `docs/testing/platform-checklist.md`, `CHANGELOG.md`, `memory-bank/systemPatterns.md` (one pattern line: never close a PopupWindow before calling out from its handler).

## Acceptance
The new test passes and was seen failing first; all existing tests pass; `cargo fmt --all`, then `powershell -File scripts/check.ps1` prints `ALL CHECKS PASSED`.

## Known pitfalls
- `#[path]`/test modules: top level only (F-37); Windows-only code must lint on macOS/Linux (the test must build and pass on all three CI OSes: the software renderer is cross-platform; avoid Windows-only APIs).
- Keys go to an open `PopupWindow`; elements inside it cannot be referenced from outside (D-079).
- Never run the app with a visible window except with `--data-dir <temp>` and `--exit-after-ms`.
