# Task 610: A late copy after a failed translate press is stored in History (Q-08 bug, 0.3.1)

**Agent:** `specialist` · **Model:** opus · **Branch:** `task/610-late-copy-fix` (from `main`) · **Review:** `security-reviewer` (opus)
**Global Constraints:** GC-03, GC-06, GC-18, GC-19, D-093 (never stored), design G4. Plan: `docs/plans/phase-6-translate-in-place.md`. Owner report: Q-08, 2026-11-28 (below).

## Owner report and root cause (orchestrator investigation)
- **Symptom:** in Zalo web on Chrome (`chrome.exe`), History stores both the source text and the translation, each with source `chrome.exe`. Notepad is clean. A press that translates on the first try stores nothing (owner re-test). In Chrome the first press often fails.
- **Log evidence:** the owner's log has many `translate outcome=failed reason=nothing_copied ... total_ms=1799..3672`, each followed a few seconds later by a press that succeeds.
- **Mechanism, confirmed in code:**
  - `crates/darkclip-app/src/capture/claim.rs` `expire_claim_if_due` (around l.176) replies `Expired` and drops the slot.
  - No rule covers a copy from the target app that arrives **after** the deadline. `claimed_recent` and the follow-up skip (`recent_claim_follow_up`, around l.264) are set only when a change **was** claimed.
  - So when Chrome answers the synthetic Ctrl+C later than the 1 s window, that copy is captured as a normal clip.
  - When the field already holds a translation (the user presses again on the same field), the late copy holds the **translation**, which explains "both are stored, source chrome.exe".
- **Second defect:** `apps/desktop/src/translate/flow.rs` `COPY_WAIT_MS = 400` decides "nothing copied, select all" after only 400 ms. In a slow app the first Ctrl+C is still pending, so the retry sends Ctrl+A + Ctrl+C. The **whole field** gets selected and copied even though the user selected only a part.
- **Unknown:** Chrome's real reply time to a synthetic Ctrl+C. The log has no copy timing. Add it (below), content-free.

## Goal
A press of the translate shortcut never causes the target app's copy to be stored, whether it arrives in time, late, or after a failure or cancel. Slow apps like Chrome should work on the first press, and a partial selection is never widened to the whole field because the app was slow.

## Changes
1. **Tail after a slot ends without a claim (capture, `claim.rs`).**
   - When a slot ends without a claimed change, set `unclaimed_tail = { app_id, armed_signal_at, until }`. A slot ends without a claim when it expires, or when the requester is gone and it then expires; Esc in the flow drops the receiver, and the slot then expires.
   - A later change whose **primary source** matches the slot's app (the `same_app` rule), or that is ownerless with an F-14 heir naming it, is **skipped and never stored** when all of these hold:
     - it is first signalled at or after `armed_at`;
     - it is within `UNCLAIMED_TAIL_MS = 5_000` after the deadline (signal time);
     - there was no own write since arming.
   - Count it as `SkipReason::TranslateSource` with the normal `Skipped` event. This is the same treatment as the 603b follow-up skip.
   - A **new** live slot always wins: the check runs only when the change is not claimed.
   - Arming a new slot clears the tail for any other app id, and keeps it if the app id is the same.
   - Note the residual in T-21: a copy the user makes themselves in the same app within 5 s after a failed press is not stored; it is counted, and nothing is overwritten.
2. **Longer copy wait for the first copy.**
   - `CLAIM_WINDOW_MS` goes from 1000 to **3000**. A claimed change still replies as soon as it is read, so the success path does not slow down.
   - `CLAIM_REPLY_SLACK_MS` stays 800.
   - In `flow.rs`, `COPY_WAIT_MS` goes from 400 to **1200**. After 1200 ms with the seq unchanged, select-all is still allowed, re-checked as today right before it. Measure the cost: a press with nothing selected in a fast app now waits about 1.2 s before select-all. Record it as a known trade-off in the CHANGELOG.
   - Update the 603b follow-up-skip bound (`max_burst_wait + CLAIM_WINDOW_MS`) so it stays tied to the new window, and check that its tests still mean the same.
3. **Content-free copy timing in the log (`flow.rs` / `host.rs`).** Add `copy_ms=` (time from the first `send_copy` to the claim reply) and `select_all=0|1` to the per-press `translate outcome=` line, so the next owner report shows the real Chrome latency. No text, no app id.
4. **Records:** `docs/testing/platform-checklist.md` Translate rows get the owner's Q-08 results of 2026-11-28 (each marked with that date and "owner, 0.3.0"):
   - rows 1 (focus etc.), 3 (Paused/off/Private), 5 (Esc, twice, held), 7 (offline, wrong or no key), 8 (consent), 9 (key), 10 (clash) and 11 (DPI, monitors, VI): **T**.
   - row 2 (history): **FAIL in Zalo web on Chrome, fixed in Task 610, re-test pending**; Notepad clean.
   - row 4 (KeePassXC, terminals): skipped by the owner, still **untested**.
   - row 6 (newer copy wins): not testable by hand (translation too fast), covered by the unit test `newer_user_copy_during_api_wait_is_not_overwritten`, still **untested** by hand.
   - Map these to the checklist's actual row order.
   - Add a `CHANGELOG.md` `[Unreleased]` Fixed entry for the History bug, and update the relevant Known issues lines.

## Tests (write first)
- capture (`capture/tests/claim_contract.rs` or a new file):
  - `late_target_copy_after_expiry_is_skipped_and_counted`: arm, no change within the window, then a target-app change 500 ms after the deadline. It is not stored, and the `TranslateSource` counter increments.
  - `late_copy_after_tail_or_from_other_app_is_stored`: a target change 5 001 ms after the deadline is stored, and another app's change inside the tail is stored.
  - `own_write_ends_the_tail`
  - `new_arm_for_same_app_keeps_tail_and_new_claim_wins`
  - `late_ole_flush_naming_target_is_skipped`
  - every existing claim test passes, updated only for the new window constants, with the same meaning.
- flow (`flow.rs` tests):
  - `slow_copy_is_claimed_without_select_all`: the seq moves at 900 ms; no select-all is sent and the claim is used.
  - `select_all_only_after_1200_ms_unchanged`
  - the `outcome` log line includes `copy_ms` and `select_all`, and is still content-free.

## Files this task may edit
- `crates/darkclip-app/src/capture/{claim.rs,pipeline.rs,actor.rs,mod.rs,tests/**}`
- `apps/desktop/src/translate/{flow.rs,host.rs}`
- `docs/security/threat-model.md` (T-21 note)
- `docs/testing/platform-checklist.md`, `CHANGELOG.md`
- `memory-bank/progress.md` (the 610 row)

## Acceptance
- Every test above passes.
- `cargo clippy --workspace --all-targets -- -D warnings` is clean.
- `cargo fmt --all` has been run.
- `powershell -File scripts/check.ps1` prints `ALL CHECKS PASSED`.
- No content in logs.
- Owner re-test in Zalo web on Chrome: on the first press, History stays clean, both after a failure and after a success. This is an owner row, recorded as pending.

## Known pitfalls
- **GC-03:** decide from snapshot metadata only; read nothing to decide the tail.
- **No logging in `capture/`** (GC-06 scan). The timing goes in the desktop flow log line only.
- **Deterministic tests:** signal times via the existing `copy_burst_at` / hooks; use the `claim_window_ms` hook for the long window rather than real 3 s sleeps (F-16).
- **F-42:** known timing flakes; rerun before blaming the change.
- **GC-19:** no real input, clipboard or network in tests.
