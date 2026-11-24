# Task 416: SQLite temporary data stays in memory on every connection; migration copy without pending search words

**Agent:** `specialist` · **Model:** opus · **Branch:** `task/416-temp-store-memory` (from `main`) · **Review:** `security-reviewer` (opus)
**Global Constraints:** GC-03, GC-06, GC-07, GC-19. **Source:** follow-ups F-29 and F-30 (D-073, D-074); owner approved 2026-11-24 together with a 0.1.2 release (D-075).

## Cause
- **F-29.** The bundled SQLite 3.54.1 is compiled with `SQLITE_TEMP_STORE=1`, and `PRAGMA temp_store` defaults to 0, so temporary tables, sorters and index builds go to files in `%TEMP%` once they outgrow the page cache. `crates/darkclip-storage/src/db.rs` (open, around lines 63–112) never sets `temp_store` on the live read-write or read-only connections. Only `Storage::compact` (`maintenance.rs`) and the backup/staged copies (Task 415) do. The likeliest spill is the non-FTS fallback search (`search.rs`, `ORDER BY last_copied_at DESC, id DESC LIMIT 2000`): up to 2,000 rows with previews pass through a sorter, which can land in `%TEMP%` outside the private data folder (T-04, T-05). `quick_check`, migrations and the blob sweep run on the same connection.
- **F-30.** At open, `backup(&conn, &backup_path(..))` (`VACUUM INTO`) makes `history.db.pre-v{N}.bak` before migrating. If the previous session ended without its shutdown FTS merge, the copy keeps the search words of evicted clips and replaced notes until the next good open. If the database is then moved aside as damaged, it keeps them indefinitely.

## Requirements
1. **`temp_store=MEMORY` on every connection the storage crate opens on the user's data.** In `db.rs` open, set it right after `Connection::open_with_flags` and `busy_timeout`, before `quick_check`, migrations and the sweep, in **both** `ReadOnly` and `ReadWrite` modes. `compact` saves the old value and restores it afterwards; simplify that code if the value is now always MEMORY, but keep `compact` correct. Check every other `Connection::open*` call in non-test code under `crates/` and `apps/`. Each one that touches user data must set the pragma or state in a comment why it does not need to (in-memory connections do not).
2. **Migration copy without pending words (F-30).** Before `backup(...)` in the migrating branch of open, run the FTS merge (`INSERT INTO clips_fts(clips_fts) VALUES('optimize')`) on the live connection. Only do this when the `clips_fts` table exists, because a v1 database without FTS must still migrate. Decide whether a failed merge fails the open or is best-effort, and justify the choice in a comment. Never leave the database unopenable because of a merge that is only for privacy; if you choose best-effort, the copy must still be made.
3. **Tests (red first; commit failing tests before the fix):**
   - `writer_and_reader_connections_keep_temporary_data_in_memory`: open a store read-write and read-only, and assert `PRAGMA temp_store` = 2 on each live connection. Add a test-only accessor if needed.
   - A test that `temp_store` is already 2 when migrations run, through a seam or by checking it inside a test migration.
   - `migration_backup_has_no_pending_fts_tokens`: build a database at the previous version with an evicted clip or replaced note whose merge is still pending (unique token present in the db or WAL bytes). Open it with the current migrations and assert the token is absent from `history.db.pre-v{N}.bak`. If building an older-version database with pending tokens is not practical, use a test migration list with one extra no-op migration, and document that.
4. **CHANGELOG** `[Unreleased]` → Fixed: temporary data that SQLite uses for large searches and maintenance no longer goes to the system temp folder, and the safety copy made before a database upgrade no longer keeps search words of removed items.
5. No behaviour change in search results or ordering. The existing search, retention and maintenance tests must stay green.

## Out of scope
F-26, F-27 (owner decisions pending).

## Done when
- The new tests fail before the fix and pass after.
- `powershell -File scripts/check.ps1` prints `ALL CHECKS PASSED`.
- No clipboard content in logs; no test writes the real clipboard (GC-19).
