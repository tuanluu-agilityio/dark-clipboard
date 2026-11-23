# Task 213: Phase 3 review fixes

**Agent:** `specialist` · **Model:** opus · **Branch:** `task/213-phase3-fixes` (from `main`) · **Review:** `security-reviewer` (opus)
**Global Constraints:** GC-06, GC-10, GC-13, GC-14, GC-18, **GC-19**. **Source:** Phase 3 review (Task 210 item 5), findings 1–14. Decision entry D-056.

## Requirements
1. **(blocking) Popup shows stale rows after hide.** `popup_host.rs` `hide()` (and `shutdown_pending`) clears the list rows, chips/sources and the `ListModel` like `history_host::leave` (D-052), so a clip deleted in History, cleared in Settings or expired while the popup was hidden never appears, even for one frame, and Enter/Alt+n cannot act on a stale selection. Test (view-model or host-level): after hide, the next show renders no previous row before the first page arrives. Owner checklist row: delete in History / Clear history, then open the popup — the deleted clip never appears.
2. **Double-click pastes.** Row activation (mouse double-click) uses `Via::Other` (restore only), never paste, even with "Enter also pastes" on (D-050, T-19). Test the mapping; checklist row.
3. **Stale Settings form overwrites the welcome's start-at-login answer.** `Draft::apply_to` writes `launch_at_login` only if the user changed the toggle in this form (compare with the value the form opened with); otherwise keep the worker's current value. Same rule for any other field the welcome or tray can change while Settings is open (capture state is already untouched). Test: open form → welcome Start with login on → Save form with only language changed → login stays on.
4. **F-18.** Settings excluded-app list shows `name`/`app_id` with bidi controls replaced by U+FFFD (reuse the D-051 helper); the built-in removal confirmation text distinguishes password managers from the Darkshot entry (its markers still exclude it). i18n en + vi.
5. **`--data-dir` equal to the default data dir** is treated as the default profile (settings from `%APPDATA%`, instance scope default, autostart allowed), not as a custom profile. Test the pure decision.
6. **Shutdown:** hide every window (popup, history, settings, welcome) before `worker.stop()`.
7. **Unsafe hygiene:** narrow the unsafe block in `winsys.rs` `running_image_names` to the FFI calls; fix the stale comments in `main.rs:9` and `single_instance.rs:9-10` (unsafe also lives in `winsys.rs`).
8. **Docs:**
   - `README.md:36`: replace "no agent clicked, typed or screenshot it" with an accurate statement (since D-048 agents never screenshot, click or type; one capture taken in Task 204 was deleted, D-048).
   - `docs/user/windows-alpha.md`: capture mode is in Settings > Privacy (with sensitive handling and capture kinds); line 3 says the view-models (not the screens) are tested; pause lists 5 minutes, 1 hour, until resumed; double-click copies only.
   - `CHANGELOG.md`: pause entries include 1 hour.
   - `docs/testing/alpha-artifact.md`: the binary reports version 0.0.1; "0.1.0-alpha" is the milestone name; no version bump (releases only when the owner asks).
   - `docs/testing/platform-checklist.md`: new owner rows for findings 1, 2, 3, focus after the shortcut closes the popup, and the settings path rows (`%APPDATA%\DarkClipboard\settings.json` unless `--data-dir`).

## Tests
All existing tests pass unchanged, plus the tests above (written first, seen failing). Do not run `#[ignore]` tests.

## Not doing
New features; macOS/Linux.
