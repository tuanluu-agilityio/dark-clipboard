# Task 109: Keep the source across an ownerless OLE follow-up change (F-14)

**Agent:** `specialist` · **Model:** opus · **Branch:** `task/109-ole-source-carry` (from `main`) · **Review:** `security-reviewer` (opus)
**Global Constraints:** GC-03, GC-04, GC-06 (no logging in `capture.rs`), GC-10, GC-11.
**Source:** Task 107 finding F-14 (`docs/testing/platform-checklist.md`), Task 107 review (D-041). Related: D-027 (a new seq inside a debounce window starts a fresh pending change), D-031 (signal source primary).

## Problem
OLE writers that flush (Notepad sometimes, PowerShell `Set-Clipboard`) bump the clipboard sequence number at each phase, and the final phase has no owner. Each new seq replaces the pending change and drops its sources (`capture.rs` `on_signal`), so:
1. the stored clip has `source = None` (lost label), and
2. **privacy gap:** an excluded app **without** markers that writes this way is not matched by the excluded-app list.

## Read first
`crates/darkclip-app/src/capture.rs` (`on_signal`, `Pending`, `process`, `decide_all_sources`, `carried`), `crates/darkclip-core/src/policy.rs` (excluded-app step), D-027, D-031, spike answer 5 in `docs/spikes/windows-clipboard-monitor.md`.

## Requirements (two separate rules)
1. **Exclusion check, always (fail safe):** keep the sources of every pending change superseded within the same burst (the chain bounded by the existing `max_burst_wait` / 500 ms cap from the first signal). Pass all of them to `decide_all_sources` as additional sources. This can only add skips.
2. **Stored primary source, narrowly:** promote a superseded change's source to the stored primary source only when **all** hold:
   - the final change has no source from its signal, from `resolve()`, or from the snapshot owner;
   - the superseded change was never processed (no snapshot/read completed for it);
   - it is in the same burst and the time between the two signals is ≤ `debounce_ms`;
   - the seq gap between them is small (≤ 8);
   - the final snapshot still lists `DataObject` or `Ole Private Data` (the OLE flush signature; the adapter reports these with `mime: None`).
   Otherwise the stored source stays None. Never carry from a change that was processed, and never across a self-write seq.
3. The fake platform must be able to simulate: copy with owner A, then a quick follow-up seq with no owner and OLE placeholder formats.

## Tests (write first, see them fail)
- `ole_flush_follow_up_keeps_source` (owner A, then ownerless seq+1 with `DataObject` within debounce → one clip, source A).
- `excluded_app_ole_flush_without_markers_is_skipped` (A is on the excluded list, no markers, ownerless follow-up → no clip, `Skipped(ExcludedApp)`; `read_calls() == 0` for the final seq if the policy allows proving it).
- `ownerless_write_without_ole_signature_does_not_inherit` (A copies, then a different ownerless writer without `DataObject`/`Ole Private Data` within the window → stored source None, still checked against A for exclusion).
- `processed_change_is_never_carried` (A's change already stored, later ownerless change → source None).
- `self_write_seq_breaks_the_chain`.
- `burst_cap_bounds_the_chain` (signals beyond the 500 ms cap do not inherit).
- All existing capture tests pass unchanged.

## Not doing
Adapter changes. UI diagnostics for skip reasons (Phase 3).
