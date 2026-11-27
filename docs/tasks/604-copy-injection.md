# Task 604: Ctrl+C / Ctrl+A+Ctrl+C injection for translate in place

**Agent:** `specialist` · **Model:** opus · **Branch:** `task/604-copy-injection` (from `main`) · **Review:** `security-reviewer` (opus)
**Global Constraints:** GC-06, GC-18, GC-19 (agents never send real input: fakes only). Plan: `docs/plans/phase-6-translate-in-place.md` (Interfaces §604). Spec: `docs/specs/translate-in-place/design.md` §5.1 step 3, §5.2 "Key injection". Threat: T-19 (`docs/security/threat-model.md`).

## Goal
Extend the existing paste driver (`crates/darkclip-windows/src/paste.rs`, Task 205) so the translate flow can ask for a **copy** from the window the user is working in, with the same safety checks as paste:
- `select_all = false`: Ctrl+C.
- `select_all = true`: Ctrl+A, then Ctrl+C.

Task 605 calls it; nothing calls it yet.

## Interfaces
```rust
// crates/darkclip-platform/src/traits.rs, on PasteInjector (default method)
/// Sends the copy shortcut (optionally after select-all) to `target`, which must already be
/// the foreground window. Never moves the focus. Same checks as `paste`.
fn copy(&mut self, target: &PasteTarget, select_all: bool) -> Result<(), PlatformError> {
    let _ = (target, select_all);
    Err(PlatformError::Unsupported("copy"))
}

// crates/darkclip-windows/src/paste.rs
pub(crate) enum Mode { Focus, Paste, Copy, SelectAllCopy }
pub(crate) enum CtrlKey { A, C, V }
// WindowSys: `fn send_ctrl_v(&mut self)` is replaced by
fn send_ctrl(&mut self, key: CtrlKey) -> Result<(), PlatformError>;
// win.rs: `send_ctrl_v()` becomes `send_ctrl(key: CtrlKey)` (VK_A / VK_C / VK_V only)
```
The plan sketched `send_ctrl(vk: u16) -> bool`. This package replaces it with the closed `CtrlKey` enum, so no arbitrary key can be sent, and keeps the existing `Result` error shape. Record the change in the 604 progress row.

## Behaviour
- **`Mode::Copy` and `Mode::SelectAllCopy`:**
  1. `recorded(target)`, then `check_target` (the same static checks as `Paste`: normal top-level window, not ours, same pid, integrity not higher, known integrity).
  2. Wait for keys to come up: `wait_until(KEYS_WAIT, !keys_held())`, else `Refusal::KeysHeld`.
  3. **Do not call `set_foreground`.** The target must already be the foreground window, else `Refusal::NotForeground`. The copy never steals focus.
  4. Re-check right before the input: `check_window`, `!keys_held()`, and foreground equals the window.
  5. `SelectAllCopy`:
     - send `CtrlKey::A`;
     - `sleep(SELECT_ALL_GAP)`, where `pub(crate) const SELECT_ALL_GAP: Duration = Duration::from_millis(30)`;
     - re-check foreground and `keys_held` (a refusal here returns the refusal; Ctrl+A has already happened, which is harmless);
     - send `CtrlKey::C`.
  6. `Copy`: send `CtrlKey::C`.
- `Mode::Paste` and `Mode::Focus` behave exactly as today. The existing tests stay unchanged except for the `send_ctrl_v` to `send_ctrl(CtrlKey::V)` rename in the fake.
- `win.rs` `send_ctrl(key)`:
  - the same 4-event sequence and partial-send handling as today's `send_ctrl_v`: on a partial send, release `key` and Ctrl, and return `Backend{code}`;
  - the virtual-key mapping is A→`VK_A`, C→`VK_C`, V→`VK_V`.
- `WinPasteInjector::copy` runs `run(&mut WinWindows, target, if select_all { SelectAllCopy } else { Copy })`.
- Nothing logs; refusal names stay static.
- The driver does **not** wait for the translate shortcut's own main key (e.g. `T`). Task 605 waits for it before it calls `copy`. Say so in the `copy` doc comment.

## Tests (write first, in `paste.rs` tests with the existing fake `WindowSys`)
- `copy_sends_ctrl_c_only_when_foreground`: target is foreground and checks pass; the fake records exactly `[C]` and no `set_foreground` call.
- `copy_refuses_when_not_foreground_and_never_moves_focus`: foreground is another window; returns `Refused(NotForeground)`, no keys are sent, and `set_foreground` is never called.
- `select_all_copy_sends_a_then_c_with_gap`: the recording is `[A, C]` with a sleep of `SELECT_ALL_GAP` between them.
- `select_all_copy_stops_after_a_if_focus_moves`: foreground changes after A; the result is `Refused(NotForeground)` and the recording is `[A]`.
- `copy_refuses_elevated_and_unknown_integrity`: both give `Refused(Elevated)`, with no keys sent.
- `copy_waits_for_held_keys_then_refuses`: keys stay held longer than `KEYS_WAIT`; the result is `Refused(KeysHeld)`, with no keys sent.
- `copy_refuses_own_window_and_reused_handle`: our pid gives `WrongProcess`, and so does a pid mismatch.
- `paste_still_sends_v`: the existing paste tests pass with the recording `[V]`.
- `default_copy_is_unsupported`: in `darkclip-platform`, a dummy `PasteInjector` without `copy` returns `Unsupported`.

## Files this task may edit
`crates/darkclip-platform/src/traits.rs`, `crates/darkclip-windows/src/{paste.rs,win.rs}`, any other `PasteInjector` impls (fake platform, macOS/Linux stubs), which keep the default; `docs/security/threat-model.md` (one T-19 line: copy mode, no focus move), `memory-bank/progress.md` (the 604 row).

## Acceptance
- Every test above passes, and the existing paste tests are unchanged in meaning.
- `cargo clippy --workspace --all-targets -- -D warnings` is clean.
- `cargo fmt --all` has been run.
- The Linux-target clippy for `darkclip-windows` and `darkclip-platform` is clean (F-19), as in Task 602.
- `powershell -File scripts/check.ps1` prints `ALL CHECKS PASSED`.
- No test sends real input (GC-19).

## Known pitfalls (from `memory-bank/systemPatterns.md`)
- **Windows-only code must lint on macOS/Linux too** (F-19): `cfg_attr(not(windows), allow(dead_code))` for items only used under `#[cfg(windows)]`; the new `Mode` variants and `CtrlKey` are used by tests on every OS.
- **No panics in release paths.**
- **All `unsafe` in `win.rs`** with `// SAFETY:` comments.
- **GC-19:** never call the real `SendInput` from a test; never run the app against the default data dir.
- **UI interaction changes need a headless test** (`CLAUDE.md`): not applicable here (no UI); 606 covers the overlay.
