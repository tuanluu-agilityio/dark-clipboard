# Task 612: Mask the lone Alt release after the translate shortcut (Chrome menu steals the first Ctrl+C)

**Agent:** `specialist` · **Model:** opus · **Branch:** `task/612-alt-menu-mask` (from `main`) · **Review:** `security-reviewer` (opus)
**Global Constraints:** GC-06, GC-18, GC-19; threat T-19 (input injection). Source: owner re-test 2026-11-28, recorded in `memory-bank/activeContext.md`.

## Evidence (owner log, Zalo web on Chrome)
- With `Ctrl+Alt+T`: `select_all=1 copy_ms=1631`, `1439` and `1406`. The first Ctrl+C is lost, and only the Ctrl+A + Ctrl+C retry after 1.2 s copies.
- With `Ctrl+Shift+Y`, which has no Alt: `select_all=0 copy_ms=120`.
- **Explanation:** the global hotkey swallows `T`. Chrome then sees Ctrl down, Alt down, Ctrl up and Alt up, which ends as a lone Alt tap. Windows apps (Chrome, Firefox, Office) treat a lone Alt tap as "focus the menu bar". The flow's first synthetic Ctrl+C goes to the menu, not the page. The next key (Ctrl+A) dismisses the menu, so the retry works.
- **Risk:** today the retry happened to keep the selection. But a Ctrl+A that reaches the page would widen a partial selection to the whole field.

## Goal
With a shortcut that contains Alt (or Win, which opens the Start menu on a lone tap), the first synthetic Ctrl+C reaches the page. In Chrome, `copy_ms` should be close to the no-Alt case, with `select_all=0` on a normal press.

## Approach: the standard "menu mask key" (AutoHotkey `#MenuMaskKey`)
- As soon as the translate hotkey fires, while the user is still holding Alt (or Win), inject one key-down plus key-up of an **unassigned** virtual key, `VK 0xE8`. Windows then sees "Alt + another key" and does not treat the later Alt release as a lone tap, so no menu is activated.
- `0xE8` has no meaning in apps and produces no character.

## Changes
1. **`crates/darkclip-windows`:**
   - `win.rs`: `pub(crate) fn send_menu_mask() -> Result<(), PlatformError>` sends `0xE8` down and up with `SendInput`, using the same partial-send handling as `send_ctrl`. All unsafe code stays in `win.rs`, with `// SAFETY:` comments.
   - Expose a safe `pub fn mask_menu_activation()` (Windows only). Do it through the paste module or a new small `input.rs`, so the closed key set is kept: this function can send only `0xE8`.
2. **`apps/desktop/src/translate/host.rs` `on_press`:** the order is: single-flight check (a press while running sends nothing) → the shortcut contains Alt or Win **and** that modifier is still down (`winsys::any_key_down`) → `mask_menu_activation()` → the existing flow.
   - Send the mask only for the translate shortcut, and only when Alt or Win is part of it.
   - Never send it for a blocked press where the flow sends no keys? Decide and document. Recommendation: **mask anyway**. It is harmless and prevents the menu even when the press is refused. Say so in the T-19 note.
3. **Mask failure:** if `SendInput` fails, log it content-free (`translate_menu_mask status=failed code=N`) and continue. The 1.2 s retry remains the fallback.
4. **Threat model:** add a T-19 note. The injected key set for translate is now Ctrl+A, Ctrl+C, Ctrl+V plus the unassigned `0xE8` mask, which is sent only while the user is holding the shortcut's Alt or Win key.
5. **CHANGELOG `[Unreleased]` Fixed:** "With a shortcut containing Alt (default Ctrl+Alt+T), Chrome and other apps with a menu bar no longer lose the first copy (lone Alt release opened the menu); the press is about 1.3 s faster and no longer relies on the select-all retry."
6. **Checklist:** add an owner row to the Translate section: Chrome with Ctrl+Alt+T should show `select_all=0` and `copy_ms` under 500 in the log; also Word and Firefox if available.

## Tests (write first; no real input, GC-19)
- Pure:
  - `shortcut_needs_menu_mask`: true for Ctrl+Alt+T, Alt+Shift+X and Win+Ctrl+X; false for Ctrl+Shift+Y.
  - The host decision function takes "modifier still down" as an input, so it can be tested without OS calls.
- Host with a fake injector seam:
  - the mask is sent once per press, before any flow key;
  - no mask on a busy (single-flight) press;
  - no mask when the modifier is already released.
- `darkclip-windows`: a test that the mask function's key set is exactly `[0xE8 down, 0xE8 up]`. Build the INPUT array through a pure helper and assert on it; never call `SendInput`.

## Files this task may edit
`crates/darkclip-windows/src/{win.rs,lib.rs,paste.rs or new input.rs}`, `apps/desktop/src/translate/host.rs`, `apps/desktop/src/winsys.rs` (only if a helper is needed), `docs/security/threat-model.md`, `docs/testing/platform-checklist.md`, `CHANGELOG.md`, `memory-bank/progress.md` (the 612 row).

## Acceptance
- Every test above passes.
- `cargo clippy --workspace --all-targets -- -D warnings` is clean, and the Linux-target clippy for `darkclip-windows` is clean (F-19).
- `cargo fmt --all` has been run.
- `powershell -File scripts/check.ps1` prints `ALL CHECKS PASSED`.
- Owner row (pending): Chrome with Ctrl+Alt+T shows `select_all=0` and a small `copy_ms`.

## Known pitfalls
- **GC-19:** never call the real `SendInput` from a test.
- **Windows-only code must lint on macOS/Linux** (F-19).
- **All unsafe code in `win.rs`.** No logging in adapters; the host logs content-free.
- **UI interaction changes need a headless test** (`CLAUDE.md`): this is not a UI change. The hotkey path is covered by host tests with fakes.
