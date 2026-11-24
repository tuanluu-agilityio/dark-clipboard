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
| 411 | Data management: storage usage, cleanup tools (clear keeping pinned+favorites, older than N days, orphan sweep on demand, compact) (owner prompt phase3 §7, D-057) | 401 | builder / sonnet | security-reviewer / opus | Medium | Deletion respects pin/favorite |
| 412 | Opt-in update check (D-037): manual button + weekly toggle off by default, GitHub Releases metadata only, never downloads; network code only in `apps/desktop` (GC-08 amended, D-057) | — | builder / sonnet | security-reviewer / opus | Medium | No request when off (test) |
| 413 | Release blockers from Task 407: WER exclusion (high), data-folder DACL, damaged-DB message and move-aside, T-12 doc and pipe message, backup sync warning, KeePass 2 exclusion, forbid(unsafe) (D-067) | 407 | specialist / opus | security-reviewer / opus | High | F407-01 fixed |
| 410 | README, CHANGELOG `[0.1.0]`, website section in the Darkshot site (if the owner wants it there; otherwise a docs page), release notes | all | scaffolder / sonnet | reviewer / sonnet | Low | Every claim matches the platform matrix |

## Execution (D-057, 2026-11-23)

Phase 5 starts before Phase 4 because Phase 4 cannot be verified without hardware (Q-04). Owner prompt phase3 (organization, editing, transforms, import/export, data management) and the release parts of prompt phase5 are folded in (D-036). Order and models:

| Order | Task | Agent / model | Reviewer | Why this model |
|---|---|---|---|---|
| 1 | 401 Migration 2 | specialist / opus | security-reviewer / opus | Storage integrity, migration with backup |
| 2 | 402 Organization API | builder / sonnet | reviewer / opus | Mechanical over 401; reviewer raised to opus (data integrity) |
| 3 | 404 Transforms + edit copy | builder / sonnet | security-reviewer / opus | Pure functions; the new clip path is privacy-relevant |
| 4 | 411 Data management | builder / sonnet | security-reviewer / opus | Deletion |
| 5 | 403 Organization UI | builder / sonnet | reviewer / opus | UI over 402 |
| 6 | 405 Backup / export / import | specialist / opus | security-reviewer / opus | Untrusted input, data swap |
| 7 | 406 Fuzzing | specialist / opus | security-reviewer / opus | Parsers |
| 8 | 412 Update check | builder / sonnet | security-reviewer / opus | First network code |
| 9 | 407 Security re-review (Windows scope) | security-reviewer / opus | orchestrator | Phase gate |

Not run without the owner: 408 (release workflow; a test tag is a public action), 410 (0.1.0 docs/website), and the 0.1.0 release itself (releases only when the owner asks; macOS/Linux preview needs Phase 4). 409 is done (GPLv3 applied in Task 201, D-045). Paste Stack is planned for 0.2 (D-037); Snippets = a user collection plus pinned clips (403 docs). Markdown conversion is not in 404 (no reliable deterministic converter without a large dependency). Item "rename" is not added (clips have no name field; the note covers it).

## Phase review result (2026-11-24, D-074)

Re-review after Task 415: **APPROVED** (specialist / opus). Gate on `main` 7906839: ALL CHECKS PASSED (1006 passed, 0 failed, 32 ignored).

- [x] RF1 Migration 1→2 at 10k clips, failure mid-migration: `migration_1_to_2_keeps_10k_clips_and_creates_favorites`, `migration_2_failure_mid_transaction_keeps_v1_and_backup` (restore from `.bak` not exercised, F-28).
- [x] RF2 Newer backup refused: `newer_backup_is_refused_and_live_data_is_unchanged`, `restoring_a_newer_backup_is_refused_and_keeps_the_history`.
- [x] RF3 Masked clip in an exported collection excluded and counted: `export_collection_with_masked_member_excludes_and_counts` (Task 415).
- [x] RF4 Vietnamese uppercase tag normalized, searchable without accents: `vietnamese_tag_searchable_without_accents`.
- [x] RF5 Fuzzed DIB/PNG/HTML: no panic, bounded memory (stable fallback harness, `docs/testing/fuzzing.md`); TIFF not applicable (no decoder). libFuzzer not run (F-24).
- [x] Backup privacy (review B1): Task 415 tests (`backup_after_eviction_has_no_evicted_tokens` and others).
- [ ] Owner manual rows in `docs/testing/platform-checklist.md` (UI, J7 keyboard flows, file dialogs, WER/ACL checks): untested by hand (GC-19).
