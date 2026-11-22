# ADR-0003: Storage engine and layout

- Status: **Accepted**, 2026-11-22 (not prototyped; low technical risk)

## Decision

- SQLite through `rusqlite` with the `bundled` feature (ships a known SQLite version with FTS5 on every OS; no system library variance).
- WAL journal, `foreign_keys=ON`, `secure_delete=ON`, `busy_timeout=2000`.
- FTS5 contentless index with `unicode61 remove_diacritics 2` (Vietnamese accent-insensitive search).
- Text ≤ 64 KiB inline; larger text and all images go to content-addressed blob files under `blobs/`, keyed by SHA-256.
- PNG thumbnails (max 256 px) generated at capture on the actor thread with the `png` crate (as Darkshot does), and image resizing via a small dependency (`image` with only the `png` feature, or a hand-written box filter; decided in the Phase 1 storage task, preferring fewer dependencies).
- Migrations: embedded SQL plus `user_version`; pre-migration backup copy.
- No ORM, no async database layer.

## Alternatives

| Option | Why not |
|---|---|
| `sqlx` | Async runtime and compile-time database features we do not need; heavier build |
| `redb` / `sled` / LMDB | No full-text search; we would build our own index |
| Tantivy for search | Powerful but large (several MiB) and a second store to keep consistent; FTS5 is enough for ≤ 100k clips |
| Store everything in SQLite BLOBs | Large images bloat the DB and WAL; content addressing on disk also dedupes identical images across clips |
| SQLCipher from day one | Changes the crypto toolchain on three OSes; deferred to P2 (see data-model §9) |

## Consequences

- One file DB plus a blob directory; backup is the online backup API plus blob copy.
- CLI and MCP can open the DB read-only concurrently (WAL). See ADR-0006.
