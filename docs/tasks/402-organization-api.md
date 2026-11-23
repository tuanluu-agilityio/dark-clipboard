# Task 402: Application API for collections, tags, notes, favorites

**Agent:** `builder` · **Model:** sonnet · **Branch:** `task/402-organization-api` · **Review:** `reviewer` (opus; data integrity)
**Global Constraints:** GC-03, GC-05, GC-06, GC-10, GC-11, GC-14. **Plan:** Phase 5 Task 402; `docs/architecture/ai-integration.md` §1; D-028 (actor owns the writer); D-057.

## Requirements
1. Implement the P1 `ClipApi` methods that return `NOT_YET` today: `list_collections`, `create_collection`, `add_clip_to_collection`, `update_clip_note`, `update_clip_tags`. Add (on `ClipApi`, P1): `rename_collection`, `delete_collection` (user collections only; clips are kept), `remove_clip_from_collection`, `reorder_clip_in_collection`, `set_favorite(clip, bool)`, `list_tags`. All mutations go through deadline-carrying `ActorCommand`s (D-028); reads use the read-only connection.
2. `SearchQuery` collection/tag filters reachable from the API; `ClipSummaryView` gains `favorite: bool` and `tags` (names). Update `docs/architecture/schemas/*.json` and examples only if the serialized shape changes, then run `node tools/schema-check/validate.mjs`.
3. Permission: `Caller::Ui` allowed; `Caller::Cli`/`Mcp` keep the existing guard behaviour (no new grants in P1). `export_collection` stays `NOT_YET` (405).
4. Masked or expired clips: organisation calls return `NotFound` like other reads.
5. An `AppEvent` notifies the UI after a mutation so open windows refresh.

## Tests (first, failing)
One test per method (happy path; invalid name/tag → `InvalidArgument`; unknown id → `NotFound`; system collection rename/delete refused); favorites survive retention through the API; a mutation from a non-UI caller follows the guard; schema validation passes.

## Not doing
UI (403), export (405).
