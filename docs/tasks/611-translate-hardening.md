# Task 611: Translate in place hardening for 0.3.1 (608 follow-ups, F-42, F-44)

**Agent:** `specialist` · **Model:** opus · **Branch:** `task/611-translate-hardening` (from `main`) · **Review:** `security-reviewer` (opus)
**Global Constraints:** GC-03, GC-06, GC-18, GC-19, D-093. **Sources:** the phase review 608 non-blocking items 5 and 6 (`memory-bank/progress.md` 608 row), F-42 and F-44 (`memory-bank/activeContext.md`).

## Goal
Close the known small issues before 0.3.1, with no behaviour change visible to the user except the fixes.

## Items
1. **Credential Manager off the UI thread** (608 item 5; `SecretStore` doc: "call them off the UI thread").
   - In `apps/desktop/src/ui/settings_host.rs`, the key read when the form fills (`fill_key`, ~l.642) and the Save and Remove handlers (~l.1733, ~l.1764) call the store on the UI thread. Run each on a short worker thread.
   - Post the result back with `slint::invoke_from_event_loop`. Use a generation counter so a late result for an older form fill or press is dropped, as the Test button already does.
   - Disable the Save and Remove buttons while a call runs.
   - Wipe the key `String` copies on the worker as today.
   - `translate::host::key_changed()` is still called after a successful set or remove.
   - Test: the pure decision code stays in `prefs/translate.rs`. The headless `tests/translate_settings.rs` still passes. Add a pure test for the generation check.
2. **Late worker jobs after `APP_CALL_WAIT`** (608 item 6; `apps/desktop/src/translate/host.rs` `call`, ~l.411).
   - Problem: when a call times out (5 s), its job may still run later on the UI worker. A late `ui_translate_claim` arms a slot nobody waits on. A late `ui_copy_text_if` writes the translation after the flow gave up.
   - Fix: each flow run owns an `Arc<AtomicBool>` "ended" flag, set when `run_flow` returns, on every path including a drop guard. Every job the host posts checks the flag **first** and does nothing if it is set. A skipped `ui_copy_text_if` wipes its text.
   - Tests (host-level, fake app worker that delays jobs):
     - `late_jobs_after_flow_end_do_nothing`: a delayed claim arm and a delayed copy-text-if run after the flow ended; neither is executed, and the text is wiped.
     - `jobs_within_the_flow_still_run`.
3. **F-42: timing tests flake under load.**
   - `crates/darkclip-core/src/sensitive/mod.rs` `pathological_inputs_are_fast` (bound 200 ms release / 2 s debug, ~l.409): keep it as a guard against catastrophic slowness. Raise the bounds to 1 s release / 10 s debug, and add a comment that the real budget is measured by the `#[ignore]` bench.
   - `crates/darkclip-app/src/capture/tests/retention.rs:286` `evicted_words_leave_the_file_ten_minutes_after_the_first_eviction_despite_steady_captures`: find the wall-clock dependency and make it deterministic with the test clock or the hooks, keeping its meaning. If that is not possible, widen its waits, and say why.
   - Record the outcome in F-42.
4. **F-44 (a): an ownerless signal inherits `held` after a resume** (`crates/darkclip-app/src/capture/pending.rs`).
   - An ownerless change that supersedes a held pending change may inherit `held` only if the settings gate **still** applies at that moment, that is, capture is still Paused or Disabled.
   - After a resume it does not inherit, and it is processed as a normal change. The target-app protections still apply through the 610 tails and the 603b follow-up.
   - Test: `ownerless_change_after_resume_does_not_inherit_held`. Paused, armed, the target change held; resume; an unrelated ownerless change supersedes it. It is stored as today.
5. **F-44 (c): the restore names a deleted clip.**
   - Make sure the flow treats `RestoreIf` → `NotFound` (or any restore error) as "no restore", silently: no failure overlay and no change to the outcome, because the paste already happened.
   - Test in `flow.rs`: `restore_not_found_is_silent`.
6. **Records.**
   - `memory-bank/activeContext.md`: close the 608 follow-ups, F-42 and F-44 (or say what is left).
   - `memory-bank/progress.md`: the 611 row.
   - `CHANGELOG.md` `[Unreleased]` Fixed: one line per user-visible fix (items 1, 2 and 4). Items 3 and 5 are internal.

## Files this task may edit
- `apps/desktop/src/ui/settings_host.rs`, `apps/desktop/src/prefs/translate.rs`, `apps/desktop/src/translate/{host.rs,flow.rs}`, `apps/desktop/tests/translate_settings.rs`
- `crates/darkclip-core/src/sensitive/mod.rs` (test bounds only)
- `crates/darkclip-app/src/capture/{pending.rs,pipeline.rs,claim.rs,tests/**}`
- `CHANGELOG.md`, `memory-bank/activeContext.md`, `memory-bank/progress.md`

## Acceptance
- Every test above passes, and the existing tests are unchanged in meaning (except the F-42 bounds).
- `cargo clippy --workspace --all-targets -- -D warnings` is clean.
- `cargo fmt --all` has been run.
- `powershell -File scripts/check.ps1` prints `ALL CHECKS PASSED` on two consecutive runs.
- No content in logs.

## Known pitfalls
- **GC-03 / GC-06:** no clipboard reads outside the actor; no logging in `capture/`.
- **UI interaction changes need a headless test** (`CLAUDE.md`): the Save and Remove buttons are disabled while busy, so extend `tests/translate_settings.rs`.
- **No panics in release paths;** locks use `unwrap_or_else(PoisonError::into_inner)`.
- **Windows-only code must lint on macOS/Linux** (F-19).
- **GC-19:** no real input, clipboard, network or Credential Manager writes in tests, except an `#[ignore]` test on a test-only target, which already exists in 602.
