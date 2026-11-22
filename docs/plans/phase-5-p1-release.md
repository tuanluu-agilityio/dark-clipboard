# Phase 5: P1 Organization, Hardening, First Public Release (0.1.0)

> Task packages `docs/tasks/4NN-*.md` are written at phase start. Storage migration 2 (collections, tags) follows `docs/architecture/data-model.md` §3, §4 (P1 notes).

**Goal:** Collections, favorites, tags, notes, local transforms, backup/restore and export; fuzzing and security hardening; signing and release process; public 0.1.0 (Windows supported, macOS/Linux preview).
**Global Constraints:** all; GC-17 (no P2 work).

## Review Focus
1. Migration 1 → 2 on a DB with 10,000 clips, with a crash mid-migration (backup restore path).
2. Restoring a backup made by a newer app version → refused with a clear message.
3. Export of a collection that contains a masked clip → the clip is excluded and the count is reported.
4. A tag name with Vietnamese diacritics and uppercase → normalized, searchable without accents.
5. Fuzzed DIB/PNG/TIFF/HTML-header inputs → no panic, no unbounded allocation.

## Tasks

| Task | Title | Depends | Agent / model | Reviewer | Risk | Acceptance criteria |
|---|---|---|---|---|---|---|
| 401 | Migration 2: collections, collection_clips, tags, clip_tags; system Favorites collection created | P4 | specialist / opus | reviewer / opus | High | Review Focus 1 test |
| 402 | API: collections, tags, notes, favorites (`ClipApi` P1 methods); FTS indexes note and tags | 401 | builder / sonnet | reviewer / sonnet | Medium | Schemas/examples still validate (`node tools/schema-check/validate.mjs`) |
| 403 | UI: sidebar/filter chips for collections and tags, note editor, favorite toggle, multi-select add-to-collection | 402 | builder / sonnet | reviewer / sonnet | Medium | Keyboard-only flows for J7 |
| 404 | Local transforms (plain, trim, case, JSON pretty/minify, URL decode/encode, Base64 decode with size cap) as "Copy as…" | P4 | builder / sonnet | reviewer / sonnet | Low | Pure functions in core with tests |
| 405 | Backup/restore (`.dcbackup`) and export collection (`export.schema.json`) | 401 | specialist / opus | security-reviewer / opus | High | Review Focus 2, 3 tests |
| 406 | Fuzzing (`cargo fuzz`) for image converters, HTML Format header parser, sensitive scanners; fix findings | P4 | specialist / opus | security-reviewer / opus | Medium | 1 h per target without crash; corpus committed |
| 407 | Threat model re-review against the built system; `cargo deny`; dependency audit | 401–406 | security-reviewer / opus | — | High | Report in `docs/security/review-0.1.0.md` |
| 408 | Signing and release: Windows (Authenticode or unsigned + SHA-256, as Darkshot did), macOS (Developer ID + notarization if available, else unsigned preview), Linux (checksums); GitHub release workflow from tags | 331 | builder / sonnet | reviewer / opus | Medium | Release dry-run on a test tag |
| 409 | Licence decision implemented (Q-01): `LICENSE`, Slint attribution if the royalty-free licence is chosen, third-party notices | owner decision | scaffolder / sonnet | reviewer / sonnet | Low | `cargo deny licenses` passes |
| 410 | README, CHANGELOG `[0.1.0]`, website section in the Darkshot site (if the owner wants it there; otherwise a docs page), release notes | all | scaffolder / sonnet | reviewer / sonnet | Low | Every claim matches the platform matrix |
