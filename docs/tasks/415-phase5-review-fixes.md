# Task 415: Phase 5 review fixes (backup carries deleted search words)

**Agent:** `specialist` · **Model:** opus · **Branch:** `task/415-phase5-review-fixes` (from `main`) · **Review:** `security-reviewer` (opus)
**Global Constraints:** GC-03, GC-06, GC-07, GC-19. **Source:** Phase 5 phase review 2026-11-24 (D-072), blocking item B1.

## Cause (B1)
`crates/darkclip-storage/src/backup.rs` `private_snapshot` runs `fts_optimize` and `VACUUM` only `if gone > 0`, that is only when the snapshot held masked or expired clips. Under D-062, automatic evictions and note/tag replacements stay pending in the contentless FTS5 index (up to 10 minutes or 100 changes), so their tokens are still in `clips_fts_data`. `write_backup` (called from `crates/darkclip-app/src/capture/transfer.rs`) never merges first. A backup made in that window carries the words of evicted clips and old notes, against data-model §6 ("deletion is real deletion") and D-064. The same applies to the `history.db.pre-restore-<ms>` copy.

## Requirements
1. **Every backup snapshot is merged and vacuumed.** In `private_snapshot`, run `fts_optimize` and `VACUUM` unconditionally (the tag cleanup may stay under `gone > 0` or run always). Keep the doc comment true.
2. **Pre-restore copy.** Make sure the `pre-restore` copy also holds no pending FTS tokens (merge on the live writer before copying, or apply the same snapshot treatment). Prefer the smallest change that is true; document it in the code.
3. **Regression tests (red first; commit the failing test before the fix):**
   - `backup_after_eviction_has_no_evicted_tokens`: `evictable_store`-style setup, `apply_retention` so `fts_pending() > 0`, no masked/expired clips, `write_backup`, then assert the evicted clip's unique token is absent from the archive's `history.db` bytes.
   - `backup_after_note_replace_has_no_old_note_tokens`: set a note with a unique token, replace it, back up at once, assert the old token is absent.
   - A test for requirement 2.
4. **Review Focus 3 literal test (non-blocking 3):** `export_collection_with_masked_member_excludes_and_counts`: a collection whose member is re-captured as masked (D-059) is exported with `ExportSelection::Collection`; the masked clip is absent and counted.
5. **Docs (non-blocking 4, 5):**
   - `docs/testing/fuzzing.md`: say there is no TIFF decoder (`CF_TIFF` is only named), so Review Focus 5 TIFF is not applicable, and no separate CF_HTML header parser exists.
   - `docs/architecture/data-model.md:144`: P2 tables move to migration 4; add a dated "Changed by D-072" note (do not rewrite the rest).
6. **CHANGELOG** `[Unreleased]` → Fixed: backups made shortly after automatic cleanup or a note change could contain search words of the removed items.

## Out of scope (recorded as follow-ups in D-072, need owner decisions)
- "Save as new clip" does not inherit `no_export` from the source clip (F-26).
- Copy-back of a `no_export` clip does not re-add `CanUploadToCloudClipboard=0` (F-27).

## Done when
- The new tests fail before the fix and pass after.
- `powershell -File scripts/check.ps1` prints `ALL CHECKS PASSED`.
- No clipboard content in logs; no test writes the real clipboard (GC-19).
