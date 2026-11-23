# Task 401: Storage migration 2 (collections, tags, notes, favorites)

**Agent:** `specialist` · **Model:** opus · **Branch:** `task/401-migration-2` (from `main`) · **Review:** `security-reviewer` (opus; storage integrity, deletion)
**Global Constraints:** GC-02, GC-06, GC-10, GC-11, GC-15, GC-16. **Plan:** Phase 5 Task 401; `docs/architecture/data-model.md` §3 (Collection, Tag, RetentionPolicy), §4, §7; Review Focus 1, 4; decision D-057.

## Read first
`crates/darkclip-storage/src/{migrate.rs, db.rs, clips.rs, search.rs, retention.rs, stats.rs}`, `migrations/0001_init.sql`, schemas `collection.schema.json`, `tag.schema.json`.

## Requirements
1. `migrations/0002_organization.sql` (sets `user_version = 2`): tables `collections` (id, name ≤ 80, kind `user|system`, system_key, color, sort `manual|recent`, created_at, updated_at; `system_key` unique), `collection_clips` (collection_id, clip_id, ord, added_at; PK (collection_id, clip_id); both FKs `ON DELETE CASCADE`), `tags` (id, name unique, created_at), `clip_tags` (clip_id, tag_id; cascades). The system collection Favorites (`system_key = 'favorites'`) is created by the migration and can never be renamed or deleted.
2. The existing backup-before-migration path (`history.db.pre-vN.bak`) covers 1 → 2. A failure inside the migration transaction leaves version 1 data intact and the backup present (Review Focus 1, on a 10,000-clip DB).
3. Storage methods (synchronous; mutations on the writer, reads also on the read-only connection): create / rename / delete user collection, list collections with derived `clip_count`, add / remove clip to collection (idempotent), reorder a clip inside a `manual` collection (dense `ord`), set the full tag set of a clip (≤ 20 tags; unused tags deleted in the same transaction), list tags with counts, set note (≤ 4,000 chars, `None` clears). Each mutation is one transaction.
4. **Tag normalisation** (pure, in `darkclip-core`): trim, Unicode lowercase, NFC, `[\p{L}\p{N}_-]+`, ≤ 40 chars; invalid → typed error (no content in the message). Vietnamese with diacritics kept as written and searchable without accents (Review Focus 4). If NFC needs a new crate (`unicode-normalization`), add it with `cargo add` and say so.
5. **FTS:** note and tag names are indexed in `clips_fts.note` / `clips_fts.tags` and re-indexed when they change (contentless table: delete + re-insert the row with every column, using the existing preview/body indexing rules). Masked clips: notes and tags are refused (`InvalidArgument`), so nothing beyond the masked preview is ever indexed.
6. **Search:** `SearchQuery` gains `collection: Option<CollectionId>` and `tag: Option<String>` filters, combined with the existing ones; a `manual` collection filter lists in `ord`, otherwise the existing order. Keep the search bench within budget (run it per D-030 and report).
7. **Retention:** favorites (membership in the system collection) are exempt like pinned clips (`keep_favorites`, data-model §3). Expiry still beats favorite (D-025).
8. Clip deletion cascades membership and tags; FTS row removed as today; unused tags cleaned.

## Tests (write first, see them fail)
`migration_1_to_2_keeps_10k_clips_and_creates_favorites`, `migration_2_failure_mid_transaction_keeps_v1_and_backup`, `favorites_cannot_be_renamed_or_deleted`, `collection_membership_idempotent_and_cascades_on_clip_delete`, `manual_order_reorder_is_dense`, `tags_normalised_and_unused_removed`, `vietnamese_tag_searchable_without_accents`, `note_indexed_and_reindexed_on_change`, `note_and_tags_on_masked_clip_refused`, `retention_keeps_favorites`, `expiry_beats_favorite`, `search_filters_by_collection_and_tag`. Existing tests unchanged.

## Not doing
API and UI (402, 403). Export/backup (405).
