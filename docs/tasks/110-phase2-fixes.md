# Task 110: Phase 2 review fixes

**Agent:** `specialist` · **Model:** opus · **Branch:** `task/110-phase2-fixes` (from `main`) · **Review:** `security-reviewer` (opus)
**Global Constraints:** GC-03, GC-06, GC-10, GC-11, GC-13, GC-18.
**Source:** Phase 2 review (Task 108), findings 1–10. Decision entry D-043.

## Read first
`crates/darkclip-windows/src/{dib.rs, access.rs, win.rs}`, `crates/darkclip-app/src/{capture.rs, image.rs}`, `apps/desktop/src/main.rs`, `scripts/check.ps1` + `scripts/check.sh`, D-033, D-038, D-040, D-042.

## Requirements
1. **(blocking) Copy-back decode bound.** `png_to_dibs` (`dib.rs`) applies the same decoded caps as the app (`MAX_PIXELS` 64 MP, `MAX_RGBA_BYTES` 256 MiB, side ≤ 32,768) from the header **before** allocating. Over the cap, the write contains the registered `PNG` format only (no DIBs); never abort. Share the constants (core or a small shared module) instead of duplicating magic numbers. Test: `copy_back_png_over_decode_cap_writes_png_only` (header-only 32767×32767 PNG, no large allocation, plan contains only `PNG`).
2. **Source on a newer snapshot.** In `process` (`capture.rs`), when `meta.seq != p.seq`, the pending change's sources (and its superseded chain) are used only for the exclusion check, never as the stored primary source; the primary comes from the snapshot owner or stays None. Test: `newer_snapshot_seq_does_not_inherit_old_owner` (owner A signalled, owner B's content present at snapshot time with a higher seq → stored source B or None, never A; A still checked for exclusion).
3. **One meaning for the image cap (D-043).** `image_max_bytes` caps the **stored** image (the encoded PNG). DIB input is bounded by the decode caps (64 MP / 256 MiB RGBA, checked from the header and `GlobalSize` before allocating); after DIB→PNG encoding, an encoded PNG over `image_max_bytes` → `TooLarge`. The registered `PNG` path is unchanged. Update the `ReadPlan`/`read` doc comments. Tests: `dib_4k_screenshot_under_decode_cap_is_read` (3840×2160 32-bit, default 20 MiB cap → stored), `dib_over_decode_cap_too_large`.
4. **Logging rule scope.** The GC-06 no-logging rule in both check scripts also covers `crates/darkclip-(windows|macos|linux)/src/.*` and `crates/darkclip-app/src/image.rs`. Verify with a planted macro in a temp file, then remove it.
5. **Docs truth (GC-18).** `README.md` (Windows capture status: tested in Task 107, link the checklist), `CHANGELOG.md` (remove "real-clipboard tests still pending" and "No application build exists yet"; the F-14 entry says "fixed in the capture actor; real-clipboard recheck pending"), `crates/darkclip-windows/src/lib.rs` caps comment (real-clipboard tests ran in Task 107).
6. **Skip reasons in the headless log.** On stop, the binary logs one content-free line with per-`SkipReason` counts using `SkipReason::name()` (static snake_case names). Update `KNOWN_EVENTS`/the S-10 check if needed.
7. **Copy-back retry cost.** `write` prepares the converted data (plan, PNG decode, `GlobalAlloc` blocks) once per copy-back; Busy retries repeat only open/empty/set/close. The actor's retry loop must not rebuild the blocks. Test with the fake seam that a Busy-then-success write converts once.
8. **Monitor teardown asserted.** Extend a monitor test to assert the window handle is no longer valid (`IsWindow` false) after `stop`.

## Accepted without code (record in D-043)
- Dequeue-time source fallback on Windows can attach a later owner (low impact; the change is usually superseded).
- A `SetClipboardData` failure after `EmptyClipboard` leaves the clipboard empty or partial; the partial content may be captured again (deduplicated).
- The Darkshot **Copy image** row alone does not prove the marker path (skip reasons were not logged); the Copy link row does. Requirement 6 lets a later run prove each row.

## Tests
All existing tests pass unchanged, plus the tests named above. Do not run `#[ignore]` tests; never write the real clipboard.

## Not doing
UI (Phase 3), new features.
