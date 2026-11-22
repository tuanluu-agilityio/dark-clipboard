# Task 008: Full-text search and listing

**Agent:** `builder` · **Model:** sonnet · **Branch:** `task/008-storage-search` · **Review:** `reviewer` (sonnet)
**Global Constraints:** GC-06 (queries are never logged), GC-10.

## Read first
`docs/architecture/data-model.md` §4 FTS notes, `docs/architecture/ai-integration.md` §1 `SearchQuery`, Task 007 Interfaces.

## Files
Create `crates/darkclip-storage/src/search.rs`; modify `clips.rs` (FTS row maintenance on upsert), `lib.rs`.

## Interfaces
```rust
pub struct SearchQuery { pub text: Option<String>, pub kinds: Vec<ClipType>, pub pinned_only: bool,
                         pub since: Option<Timestamp>, pub include_masked: bool, pub limit: u16, pub cursor: Option<Cursor> }
pub struct Cursor { pub last_copied_at: Timestamp, pub id: ClipId }   // encode()/decode() as opaque base64url
pub struct ClipSummary { pub id: ClipId, pub kind: ClipType, pub preview: String, pub pinned: bool,
                         pub last_copied_at: Timestamp, pub sensitivity: SensitivityLevel, pub thumb: Option<[u8; 32]> }
impl Storage { pub fn search(&self, q: &SearchQuery) -> Result<(Vec<ClipSummary>, Option<Cursor>), StorageError>; }
```
Ranking: pinned first (pin_order), then bm25 for text queries, then `last_copied_at DESC`. Empty text = recent list. FTS query built from user text by quoting each token (`"tok"*`); never interpolate raw input into MATCH syntax. Masked clips: index only the masked preview; `include_masked=false` filters them out in SQL.

## Tests
- `search_prefix` ("kube" finds "kubectl get pods").
- `search_diacritic_insensitive` ("tieng viet" finds "Tiếng Việt"). Review Focus 4.
- `search_short_query_fallback` ("db" finds "mydb_backup" via substring on recent rows). Review Focus 4.
- `search_operators_are_literal` (query `NOT OR "` neither errors nor changes semantics).
- `masked_excluded_from_mcp_style_query` (`include_masked=false`).
- `pagination_stable_under_insert` (insert during paging does not duplicate or skip older items).
- `kinds_filter`, `pinned_first`.
- `bench_search` (`#[ignore]`, release): 10,000 synthetic clips of 50–2,000 chars, 200 queries, prints p50/p95; assert p95 ≤ 30 ms.

## Not doing
Notes/tags columns content (P1; columns exist, stay empty).
