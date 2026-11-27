# Task 603b: Claim contract fixes from integration review I1

**Agent:** `specialist` · **Model:** opus · **Branch:** `task/603b-claim-contract-fixes` (from `main`) · **Review:** `security-reviewer` (opus)
**Global Constraints:** GC-03, GC-04, GC-06, GC-18, GC-19. Plan: `docs/plans/phase-6-translate-in-place.md`. Source: integration review I1 (2026-11-27, reviewer sonnet, read-only). Its findings are summarised below, with file:line references to `main` at 1f1f029. Tasks 601–604 are merged; Task 605 wires them and needs these contracts first.

## Goal
Change the capture-claim and Windows contracts so that the 605 flow **cannot** store the source text, overwrite a newer copy the user made, or arm a claim that can never match. The checks move inside the actor where possible (compare-and-write), so a wiring mistake in 605 cannot reintroduce the race.

## I1 findings this task fixes
- **M2: identity.** `ExpectForeignCopy` matches `ClipboardSource.app_id`, which is the bare image file name taken from `QueryFullProcessImageNameW` via `file_name_from_path` (`darkclip-windows/src/source.rs:12-31`). No pid-to-app_id function exists, and `PasteTarget.app_id` is always `None` in the desktop (`apps/desktop/src/popup/paste.rs:157-163`).
- **M3: a newer user copy is overwritten.** The claim reply carries no seq. If the user copies X during the API wait, then `CopyText` overwrites X and the restore puts back an older clip.
- **M4: the select-all retry stores the source.** The slot is single-shot. A slow first Ctrl+C is claimed, and the retry's second change from the same app (it has an owner, so it is not the ownerless OLE flush) is stored as a normal clip.
- **R1: thin timing.** 600 ms covers about 400 + 30 ms of flow plus only about 150 ms for a slow app to copy.
- **R4: restore race.** Between the flow's seq check and `CopyBack`.
- **F-44 (b): uncounted late flush.** The late-flush skip moves no counter.

## Interfaces (replace the 603 shapes; update the plan's Interfaces §603/§605 text with a dated "Changed by Task 603b" note)
```rust
// darkclip-app::capture
pub const CLAIM_WINDOW_MS: u64 = 1_000;          // was 600 (R1)
pub const CLAIM_REPLY_SLACK_MS: u64 = 800;       // flow's receive timeout = CLAIM_WINDOW_MS + this (burst cap 500 + read retries)
pub struct ClaimedText { pub text: Redacted<String>, pub seq: u64 }   // manual Debug: seq only
ActorCommand::ExpectForeignCopy { app_id: String, reply: Sender<Result<ClaimedText, ClaimError>> }

/// Compare-and-write: writes `text` as our own write only if the clipboard's current seq is
/// still `expect_seq` (the claimed change); replies the own write seq, or `Ok(None)` when the
/// clipboard moved (nothing written).
ActorCommand::CopyTextIf { expect_seq: u64, text: Redacted<String>, deadline: Instant,
                           reply: Sender<Result<Option<u64>, ApiError>> }
/// Compare-and-write restore: copies clip `id` back only if the current seq is still
/// `expect_seq` (our translation write); `Ok(false)` when it moved.
ActorCommand::RestoreIf { expect_seq: u64, id: ClipId, deadline: Instant,
                          reply: Sender<Result<bool, ApiError>> }

// darkclip-windows (public, safe; unsafe stays in win.rs)
/// The image file name of process `pid`, in the same form as `ClipboardSource.app_id`
/// (`file_name_from_path` over `QueryFullProcessImageNameW`). None when it cannot be read.
pub fn process_app_id(pid: u32) -> Option<String>;
```
`CopyText` (Task 404, "Copy as…") stays as it is today (reply `Result<u64, ApiError>` from 603).

## Behaviour
1. **Arming window.** The claim window is 1000 ms. The rest of the arming, Busy, expiry and Private rules stay exactly as in 603.
2. **The claim reply carries the claimed seq**, the snapshot seq that was read.
3. **Same-app follow-up skip (M4).** After a change from the target `app_id` is claimed (`claimed_recent` from 603), any further change whose primary source `app_id` (same `same_app` rule) is that app is **skipped, never stored**. This holds within `max_burst_wait` plus the claim window (1500 ms) of the claimed change's first signal, as long as no own write came in between. Ownerless OLE flushes keep the 603 rule.
   - Count the skip. Use a new `SkipReason::TranslateSource`, content-free, with an i18n name only if `SkipReason` names are user-visible (check). It must emit the normal `Skipped` event.
   - Apply the same counter to the 603 late-flush skip (F-44 b).
4. **`CopyTextIf`.**
   - Inside the actor, read `access.sequence()`. If it is not `expect_seq`, reply `Ok(None)` and write nothing.
   - Otherwise do the same as `copy_text`, and reply `Ok(Some(own_seq))`.
   - The text is wiped on every path.
5. **`RestoreIf`.**
   - If `access.sequence()` is not `expect_seq`, reply `Ok(false)` and write nothing.
   - Otherwise do the same as `copy_back(id, plain=false)`, but **without** `mark_used`, since a restore is not a use (I1 R3). Reply `Ok(true)`.
   - A missing clip gives `NotFound`, as today.
6. **`process_app_id`.**
   - Opens the process with `PROCESS_QUERY_LIMITED_INFORMATION`, calls `QueryFullProcessImageNameW`, and returns `file_name_from_path`.
   - The handle is closed on every path. Nothing is logged.
   - Reuse the existing helper that `clipboard_owner_image_path` uses for the pid step, if it is separable.

## Tests (write first)
- `claim_reply_carries_claimed_seq`
- `claim_window_is_one_second`: a target signal at 900 ms after arming is still claimed. Use a hook or a fake clock in the style of the existing claim tests, deterministic.
- `same_app_follow_up_after_claim_is_skipped_and_counted`, with the select-all retry shape: claim, then the same app's owned change 450 ms later is not stored, and the skip counter increments.
- `same_app_change_after_window_or_after_own_write_is_stored`, two cases: after 1500 ms, and after an own write. Both are stored as today.
- `late_flush_skip_is_counted` (F-44 b)
- `copy_text_if_writes_only_when_seq_unchanged`, two cases: moved gives `Ok(None)` and nothing is written (the fake write count is unchanged); unchanged gives `Some(seq)`.
- `restore_if_writes_only_when_seq_unchanged_and_does_not_mark_used`, covering moved, unchanged, and a missing id giving `NotFound`.
- `process_app_id_of_own_process`, Windows only: equals the test binary's file name in the same form as `WinSourceResolver` produces. Safe: no input, no clipboard.
- Every existing 603 claim test is updated only for the reply type and the window constant, with the same meaning.

## Files this task may edit
- `crates/darkclip-app/src/capture/**`, `crates/darkclip-app/src/api/transform_ui.rs` (if a type changes)
- `crates/darkclip-core/src/policy.rs` (`SkipReason` variant), `crates/darkclip-core/src/i18n.rs` (only if skip reasons have user-visible names)
- `crates/darkclip-windows/src/{lib.rs,win.rs,source.rs}`
- `apps/desktop/**`, only where a new `SkipReason` variant must be matched (exhaustive matches)
- `docs/plans/phase-6-translate-in-place.md` (dated note; also update the §605 `FlowSys` sketch:
  - `claim(app_id) -> Receiver<Result<ClaimedText, ClaimError>>`
  - `write_own` becomes `write_own_if(expect_seq, text) -> Result<Option<u64>, _>`
  - `restore_if(expect_seq, id) -> Result<bool, _>`
  - add `keys_up(&mut self, wait) -> bool`, for the 605 key-release wait before arming: I1 M1)
- `docs/security/threat-model.md` (T-21 note)
- `memory-bank/progress.md` (the 603b row)

## Acceptance
- Every test above passes; the existing tests are unchanged in meaning.
- `cargo clippy --workspace --all-targets -- -D warnings` is clean.
- `cargo fmt --all` has been run.
- The Linux-target clippy for `darkclip-windows` and `darkclip-platform` is clean (F-19).
- `powershell -File scripts/check.ps1` prints `ALL CHECKS PASSED`.
- No content in logs, events, errors or `Debug`.

## Known pitfalls (from `memory-bank/systemPatterns.md`)
- **Single privacy choke point (GC-03):** reads only after a `decide_*` `Read` decision, inside `capture/`. `access.sequence()` is not content.
- **No logging in `capture/` or adapters** (GC-06 static scan).
- **Manual `Debug`** for anything with content.
- **Windows-only code must lint on macOS/Linux** (F-19); `process_app_id` is `cfg(windows)`.
- **All `unsafe` in `win.rs`** with `// SAFETY:` comments; handles closed on every path (GC-13 style).
- **Deterministic actor tests:** use the existing hooks (`claim_window_ms`, `drop_signal`, `wait_signal_lost`); no real sleeps longer than needed (F-16).
- **F-42:** known timing flakes (`pathological_inputs_are_fast`, `evicted_words_leave_the_file_ten_minutes_after…`): rerun before blaming the change.
