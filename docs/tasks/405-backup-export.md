# Task 405: Backup / restore, export / import

**Agent:** `specialist` · **Model:** opus · **Branch:** `task/405-backup-export` · **Review:** `security-reviewer` (opus)
**Global Constraints:** GC-06, GC-08, GC-10, GC-11, GC-14, GC-18. **Plan:** Phase 5 Task 405; data-model §8; `export.schema.json`; owner prompt phase3 §6 (D-057); Review Focus 2, 3.

## Requirements
1. **Backup** (`.dcbackup`, zip): SQLite online backup of `history.db` + referenced blobs + `manifest.json` `{ format: 1, schema_version, app_version, created_at, clip_count, files: [{path, size, sha256}] }`. Written to a temp file, then renamed. The user picks the destination (Settings > History).
2. **Restore:** validate before touching anything: entry names (no absolute paths, no `..`, no drive letters; only `manifest.json`, `history.db`, `blobs/xx/<64 hex>`), entry count and total uncompressed size caps, per-file sha256, manifest fields; newer `schema_version` or `format` → refused with a clear i18n message (Review Focus 2). Extract to a temp dir, open with `Storage` (runs migrations), `PRAGMA integrity_check`, then swap with the live data while capture is stopped; the previous data is kept as `history.db.pre-restore-<ts>` until the next successful start. Any failure leaves the live data untouched.
3. **Export** (a collection or selected clips): zip with `export.json` matching `export.schema.json` + blobs. Masked/sensitive clips are never exported; the result reports how many were excluded (Review Focus 3). `export_collection` implemented for `Caller::Ui`.
4. **Import** of an export file: validation as in 2; duplicates (same content hash) merge (bump, no second row); imported metadata is not trusted: kinds and hashes re-derived with `darkclip_core`, sensitive detection runs on imported text (handled per settings), source fields dropped, timestamps clamped to ≤ now; partial errors reported as counts per reason (no content). Nothing in imported content is executed or fetched.
5. New dependency `zip` (deflate only, no other default features), `cargo add`, `cargo deny check` passes.

## Tests (first, failing)
Backup → restore round trip (clips incl. an image blob, collections, tags); zip-slip entry refused; too many / too large entries refused; sha mismatch refused; newer schema refused with live data unchanged; failure during the swap leaves live data; export excludes masked and reports the count; import dedupes; imported secret-shaped text handled per settings; future timestamp clamped; export validates against the schema.
