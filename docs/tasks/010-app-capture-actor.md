# Task 010: CaptureActor (single serialized clipboard worker)

**Agent:** `specialist` · **Model:** opus · **Branch:** `task/010-app-capture-actor` · **Review:** `security-reviewer` (opus)
**Global Constraints:** GC-03 (blocking), GC-05, GC-06 (no logging in `capture.rs`), GC-07, GC-10.

## Read first
`docs/architecture/system-overview.md` §2–3 (rules 1–7 are normative), `docs/spikes/windows-clipboard-monitor.md` (design consequences), `docs/security/privacy-design.md` §2, §5; Interfaces of Tasks 003–007.

## Files
Create `crates/darkclip-app/src/{lib.rs, capture.rs, events.rs, config.rs}`; enable `darkclip-platform` feature `fake` in dev-dependencies.

## Interfaces
```rust
pub struct CaptureConfig { pub debounce_ms: u64 /* 75 */, pub open_retries: u32 /* 10 */, pub retry_delay_ms: u64 /* 20 */ }
pub enum ActorCommand { CopyBack { id: ClipId, plain: bool, reply: Sender<Result<(), ApiError>> },
                        SetMode(CaptureMode), UpdatePolicy(PrivacyPolicy, RetentionPolicy), Shutdown { reply: Sender<()> } }
pub enum AppEvent { ClipAdded(ClipId), ClipBumped(ClipId), Skipped(SkipReason), ModeChanged(CaptureMode), Unavailable(&'static str) }
pub struct CaptureHandle { /* Sender<ActorCommand>, join handle */ }
impl CaptureHandle {
    pub fn spawn(bundle: PlatformBundle, storage: Storage, clock: Arc<dyn Clock>, cfg: CaptureConfig,
                 policy: PrivacyPolicy, retention: RetentionPolicy, events: Sender<AppEvent>) -> Result<Self, ApiError>;
    pub fn send(&self, cmd: ActorCommand) -> Result<(), ApiError>;
    pub fn shutdown(self, deadline: Duration) -> Result<(), ApiError>;   // drains, 2 s default
}
pub struct SkipCounters { /* per SkipReason, in memory */ }   // exposed via get_storage_stats later
```
Algorithm: one thread selects over the monitor's signal channel and the command channel (`recv_timeout` loop). Remember the source at the **first** signal of a burst. After `debounce_ms` with no new signal: if `seq == last_processed_seq` → drop; else `snapshot_meta` (retry `Busy`) → `decide_pre_read` → (if `Read`) `read` → `classify` → `sensitive::scan` per `sensitive_handling` → hash → `upsert_clip` → event. Copy-back: read the reps from storage → `write` → store the returned seq as `last_self_write_seq` → `mark_used`. Run retention on start, every 10 minutes, and after an insert when `stats` exceeds a limit.

## Tests (all with `FakePlatform` and a fake clock; no sleeps above 200 ms in total per test, so use a small debounce in tests)
- `stores_plain_text`.
- `burst_same_seq_stores_once` (copy + `signal_again(19)`). Review Focus 1.
- `ole_three_phase_keeps_first_source` (copy with placeholder formats + source; `render`; `clear_source` + signal; exactly one clip, source = first). Review Focus 2.
- `excluded_never_read`: for each mandatory marker, `copy` with it → `read_calls() == 0`, no clip, `Skipped(..)` event.
- `paused_never_reads_formats` and `resume_does_not_backfill` (copy during pause; resume; no clip until a new copy). Review Focus 3.
- `self_write_not_recaptured` (copy-back then the fake's signal → no new clip, `use_count` 1).
- `sensitive_drop_default` (API-key text → not stored, `Skipped(SkipReason::Sensitive)`), `sensitive_store_masked_sets_expiry_and_masked_preview`.
- `busy_retries_then_drops` (`set_busy(11)` → dropped, counter incremented, actor alive).
- `shutdown_within_deadline`.
- `kind_disabled_after_classification_dropped` (D-021): `capture_kinds = [Url]`, copy prose → classified `Text` → not stored, `Skipped(KindDisabled)`.
- `paused_passes_no_formats_to_policy` (D-021): while paused/private, the actor calls `decide_pre_read` with empty `markers`/`available` and never calls `snapshot_meta` (assert via the fake's call counters).
- `no_logging_macros_in_capture_rs` is enforced by check.ps1, not a test.

## Not doing
API surface (011); real platforms.
