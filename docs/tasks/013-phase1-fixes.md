# Task 013: Phase 1 review fixes (search performance and small code items)

**Agent:** `specialist` · **Model:** opus · **Branch:** `task/013-phase1-fixes` (from `main`) · **Review:** `reviewer` (opus)
**Global Constraints:** `docs/plans/global-constraints.md`, especially GC-06, GC-10.
**Source:** Phase 1 review (Task 012), findings B-1, S-7 (partly), S-11, S-12. Decision entry D-029.

## Read first
`crates/darkclip-storage/src/search.rs`, `crates/darkclip-app/src/api.rs` (stats, error mapping), `crates/darkclip-app/src/capture.rs` (error mapping), `docs/plans/phase-1-foundation.md` exit checklist.

## Files
`crates/darkclip-core/src/policy.rs` (SkipReason::name only), `crates/darkclip-storage/src/search.rs`, `crates/darkclip-app/src/api.rs`, `crates/darkclip-app/src/capture.rs`, `crates/darkclip-app/src/events.rs` (only if the shared error mapping lives there).

## Requirements
1. **B-1 search performance.** The text-query path must meet the budget on this machine: p95 ≤ 30 ms, taken as the **median of 5** release runs of `bench_search` (D-029 restates the criterion as the median of 5 runs, because single runs vary 35–118 ms under the same load).
   - Rank only a bounded candidate set: take up to 500 rowids from `MATCH` ordered by `rank` (FTS5's built-in bm25 ordering is optimized), then join to `clips` for filters, pinned-first ordering and the limit. Keep the result order "pinned first, then bm25, then recency" for the returned page.
   - Keep the existing bench corpus (it is the deliberate worst case). Add one realistic-corpus variant (a vocabulary of at least 2,000 words) as a second `#[ignore]` bench, and report both.
   - Every existing search test must still pass.
2. **S-12:** search excludes clips with `expires_at <= now` (pass `now` from the caller; storage has a `Clock`). Test `expired_clips_not_returned`.
3. **S-11:** replace raw NUL bytes in `search.rs` source with `'\0'`. Afterwards `git grep -P '\x00' -- crates` must return nothing.
4. **S-7:**
   - The stats `skipped` keys use an explicit `snake_case` name per `SkipReason` (a `fn name(&self) -> &'static str` in core `policy.rs` is allowed), not `format!("{:?}")`. Test that the keys match a fixed list.
   - The `StorageError → ApiError` mapping exists once (one function), used by both `api.rs` and `capture.rs`.

## Tests
- All existing tests pass, plus the new ones above.
- `bench_search` and `bench_search_realistic` results: list all 5 p95 values per bench and the median of each, in the report.

## Not doing
Phase 2 adapter work. Doc updates (the orchestrator handles data-model §4/§7, ai-integration §1, check scripts).
