# Task 603: Capture claim for translate in place (the source copy is never stored)

**Agent:** `specialist` · **Model:** opus · **Branch:** `task/603-capture-claim` (from `main`) · **Review:** `security-reviewer` (opus)
**Global Constraints:** GC-03, GC-04, GC-06, GC-07, GC-18, GC-19. Plan: `docs/plans/phase-6-translate-in-place.md` (Global Constraints, Interfaces §603). Spec: `docs/specs/translate-in-place/design.md` §5.1 steps 2–4 and 8, §5.3 (capture skip contract), §6.2. Owner gate G1 passed (2026-11-27, Zalo, Slack and VS Code; see the 603 progress row).

## Goal
When the translate flow (Task 605) sends Ctrl+C to the app the user is typing in, the capture actor hands that one clipboard change to the flow, as text, and does **not** store it. Every privacy skip still wins. Any other clipboard change is handled exactly as today. The actor also tells the flow which stored clip to restore afterwards, and returns the seq of its own text write.

## Facts (read before designing)
- **Source matching uses `app_id`, not pid.** `ClipboardSource` (`darkclip-core/src/model.rs:202`) has `app_id` (the executable identity), and the platform resolves it at signal time; it has no pid. `PasteTarget.app_id` carries the same identity for the target window. The claim therefore matches by `app_id`. This replaces "pid" in design §5.3 and in the plan. Record it as a one-line plan note ("Changed by Task 603").
- **The gate runs before any pending change.** `Actor::on_signal` (`pending.rs:113`) drops a change while Paused, Disabled or Private, or when it is our own write, before it is ever pending. So a claim must get past that gate for Paused and Disabled (design §5.1: they do not block translate, since nothing is stored), and must **not** get past it for Private.
- `process` (`pipeline.rs:170`): snapshot meta → `seq = meta.seq` (the truth) → sources (primary, `heir_source` for an ownerless OLE flush, F-14 rule 2) → `PreReadInput` → `decide_all_sources` → read plan → `read` → "changed during read" check → `store`. `NoCloud` sets `no_export` today and is not a skip.
- `write_own` (`copy_back.rs:62`) sets `last_self_write_seq = Some(seq)`. A stale signal of an earlier own write is still dropped, because the snapshot seq (the truth) is the later own write. Prove this with a test rather than assuming it.
- `ActorCommand::CopyText` has one other caller, `api/transform_ui.rs:100` ("Copy as…").

## Interfaces (plan §603, with the `app_id` change)
```rust
// darkclip-core::policy
/// `decide_pre_read` without the settings that only stop *storing*: capture disabled, a
/// running pause, and the enabled kinds. Private still skips. Every other step is the same,
/// in the same order, and the plan reads text/plain only.
pub fn decide_translate_read(input: &PreReadInput<'_>) -> Decision;

// darkclip-app::capture
pub const CLAIM_WINDOW_MS: u64 = 600;
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClaimError { Skipped(SkipReason), NoCloud, NotText, TooLong, Expired, Busy }

ActorCommand::ExpectForeignCopy {
    app_id: String,
    reply: Sender<Result<Redacted<String>, ClaimError>>,
}
ActorCommand::RestoreCandidate { reply: Sender<Option<ClipId>> }
// CopyText: reply becomes Sender<Result<u64, ApiError>> (the own write seq).
```

## Behaviour
1. **Arming.** `ExpectForeignCopy` arms one slot `{ app_id, deadline: now + CLAIM_WINDOW_MS, reply }`.
   - If a slot is already armed and not expired, the new request gets `Busy` and the old slot stays.
   - When the deadline passes with no claimed change, the slot replies `Expired` and disarms. The actor's wake-up time must include the slot deadline, so `Expired` arrives on time even with no clipboard activity.
2. **Gate.** While a slot is armed:
   - `on_signal` lets a change through the gate when the gate reason is `Paused` or `Disabled`.
   - `Private` and `SelfWrite` are dropped as today. If the slot is armed and the gate reason is `Private`, the slot replies `Skipped(Private)` and disarms.
   - When no slot is armed, the gate is exactly as today. Prove it with a test.
3. **Claim decision** in `process`, after the snapshot and the source resolution, before any read. The change is **claimed** only if all of these hold:
   - a slot is armed and not expired;
   - the change's primary source `app_id` equals the slot's `app_id`, **or** it is ownerless and `heir_source` (F-14 rule 2, unchanged) names a source with that `app_id`.
   An ownerless change with no such heir is never claimed. A change from another app is never claimed. Both are processed exactly as today, with today's gate (a change that passed only because of the slot is then dropped with its gate reason, as it would have been).
4. **A claimed change:**
   - runs `decide_translate_read` (with `others` checked as in `decide_all_sources`: any excluded source among them gives `Skipped(ExcludedApp)`);
   - if the markers contain `PrivacyMarker::NoCloud`, replies `NoCloud` (D-093);
   - on `Decision::Read`, reads **text/plain only** (no text format gives `NotText`);
   - keeps the "changed during read" check (a changed seq gives `Skipped(Busy)`-style discard; use `Expired` if no fitting reason exists, and document the choice);
   - if the read is truncated by the size cap, replies `TooLong`;
   - otherwise replies `Ok(Redacted(text))`.
   In **every** outcome, the seq is marked processed and the slot disarms. The change is never stored, deduplicated, counted as a capture, or emitted as a new-clip event. Skip counters and events stay as today, without content. Log only `capture skip=translate_source` (or the skip reason), never content.
5. **RestoreCandidate.**
   - Returns `Some(id)` only if the current OS seq (`access.sequence()`) equals `last_processed_seq` **and** that seq was stored (inserted or deduplicated) as clip `id`.
   - Otherwise returns `None`, for example a skipped, claimed or own-write seq, or a newer unprocessed change.
   - The actor tracks `last_stored: Option<(u64, ClipId)>`.
6. **CopyText** returns the own write seq. `transform_ui.rs` ignores it, so behaviour there is unchanged.
7. **Two own writes in a row** (translation text, then the `CopyBack` restore) are never stored, in any signal order. Use a small set of the last 4 own write seqs, or the existing snapshot-truth rule if a test proves that is enough. Either way the test below must pass.

## Tests (write first; actor tests with `FakePlatform`, policy unit tests in core)
- core `decide_translate_read`:
  - `translate_read_ignores_disabled_paused_and_kinds`
  - `translate_read_still_skips_private_markers_transient_excluded_selfwrite`: one case per reason, each the same reason as `decide_pre_read`.
  - `translate_read_plans_text_only`: an image plus text offer plans text only; image only gives `NothingReadable`.
- actor:
  - `claimed_change_is_returned_and_never_stored`: clip count unchanged, no new-clip event, `last_processed_seq` set.
  - `other_app_change_is_stored_and_slot_stays_armed_until_expiry`
  - `ownerless_change_without_target_heir_is_not_claimed`
  - `ownerless_ole_flush_with_target_heir_is_claimed` (Notepad shape, F-14)
  - `claim_works_while_paused_and_while_disabled_and_nothing_is_stored`
  - `claim_refused_in_private_mode`, which replies `Skipped(Private)`
  - `claim_respects_markers_excluded_app_and_transient`, one case each
  - `claim_refuses_no_cloud_marker`
  - `claim_expires_without_activity`: the reply is `Expired` within `CLAIM_WINDOW_MS` plus a small slack, with no signal at all
  - `second_claim_while_armed_is_busy`
  - `claim_over_cap_is_too_long_and_not_stored`
  - `gate_unchanged_when_no_slot_armed`: a paused or disabled change is dropped as today
  - `restore_candidate_some_only_for_latest_stored_seq`: covers stored, deduplicated, skipped, claimed, own write, and a newer unprocessed seq
  - `copy_text_returns_own_seq`
  - `two_own_writes_in_a_row_are_never_stored`, with signal orders A,B / B,A / B only
- Existing capture tests pass unchanged.

## Files this task may edit
- `crates/darkclip-core/src/policy.rs`
- `crates/darkclip-app/src/capture/{mod.rs,actor.rs,pending.rs,pipeline.rs,copy_back.rs}`, plus a new `capture/claim.rs` if useful (keep the files focused)
- `crates/darkclip-app/src/api/transform_ui.rs` (CopyText reply type only)
- capture test files
- `docs/plans/phase-6-translate-in-place.md` (the dated `app_id` note)
- `docs/specs/translate-in-place/design.md` §5.3 (a dated "Changed by Task 603" note: `app_id` instead of pid)
- `docs/security/threat-model.md` (one T-21 line: claim contract)
- `memory-bank/progress.md` (the 603 row)

## Acceptance
- Every test above passes, and the existing capture tests are unchanged in meaning.
- `cargo clippy --workspace --all-targets -- -D warnings` is clean.
- `cargo fmt --all` has been run.
- `powershell -File scripts/check.ps1` prints `ALL CHECKS PASSED`. The GC-03 check (`ClipboardAccess` only in `capture/`) still holds: the claim reads inside the actor only.
- No content in logs, events, errors or `Debug`.

## Known pitfalls (from `memory-bank/systemPatterns.md`)
- **Single privacy choke point:** every read goes through `decide_*` first (GC-03). Never read the clipboard to decide whether to claim. Decide from the snapshot meta (formats, markers, source) only.
- **No logging in privacy modules** (the static GC-06 scan covers `capture/` and `policy.rs`).
- **Manual `Debug`** for anything with content (`Redacted`).
- **No panics in release paths.**
- **F-16 / F-19 timing:** actor tests that depend on debounce or windows must not be flaky on CI. Use the fake clock and the existing test hooks, and make the fake platform deliver signals deterministically. Avoid real sleeps longer than needed, and re-copy-until-seen patterns only where existing tests already do so.
- **Time through `Clock`**, never read in core.
- **Hot paths:** nothing new per capture when no slot is armed (one `Option` check).
- **F-42:** the darkclip-core timing test `pathological_inputs_are_fast` and the darkclip-app `evicted_words_leave_the_file_ten_minutes_after…` test may flake under load. Rerun before blaming your change.
