# Architecture and conventions

Full source: `docs/architecture/system-overview.md`, ADR-0004, ADR-0006.

## Layers
```
apps/desktop (Slint)   darkclip-cli (P2)   darkclip-mcp (P2)
          └──────── darkclip-app (ClipApi, CaptureActor, PermissionGuard) ────────┘
   darkclip-core (models, policy, markers, classify, hash, sensitive, i18n)
   darkclip-storage (SQLite + FTS5 + blobs)   darkclip-platform (traits, caps, fake)
   darkclip-windows | darkclip-macos | darkclip-linux (adapters, cfg-gated)
```
Portable crates (core, platform, storage, app) have no OS, UI or async dependencies (`check.ps1`).

## Patterns to follow
- **Single clipboard actor:** only the capture actor (`crates/darkclip-app/src/capture/`) touches the OS clipboard. Signals are hints; the sequence number is the truth. Debounce 75 ms; source captured at the first signal.
- **Policy before read:** `policy::decide_pre_read` (pure) decides from format names, markers, source and mode. Adapters report and never decide.
- **Single DB writer:** the desktop app writes; CLI/MCP open read-only and send mutations over IPC (P2).
- **Content-free observability:** manual `Debug`, `Redacted<T>`, no logging macros in privacy modules, skip counters instead of logs.
- **No panics in release paths** (`panic = "abort"`): no unwrap/expect in non-test code (clippy deny).
- **Atomic file writes:** temp file plus rename (settings, blobs). Corrupt settings → defaults plus a `.corrupt-<ts>` copy.
- **Time through `Clock`**, never read in core.
- **i18n table** with compile-time completeness (Darkshot `messages!` approach; re-implemented, not copied).
- **Capabilities are data:** `Unsupported` is a normal value; the UI renders capabilities.
- **Tests first;** synthetic secrets assembled at runtime.
- **`SchemaVersion` unit type** serializes as `1` and rejects other values; every versioned model carries it (Task 002).
- **i18n:** `messages! { Variant => ("en", "vi"), ... }` generates `Msg`, `Msg::ALL`, `t(lang, msg)`; identical strings must be listed in `SAME_IN_BOTH` (Task 002).
- **Windows-only code must lint on macOS/Linux too** (F-19, Tasks 405, 406): the local gate builds only for Windows, CI also runs clippy `-D warnings` on macOS and Ubuntu. Items, fields, variants or `mut` used only under `#[cfg(windows)]` need `#[cfg_attr(not(windows), allow(dead_code))]` / `allow(unused_mut)` or a `cfg` of their own. Implementers check every `cfg(windows)` they add for what stays unused elsewhere.
- **UI icons are vector `Path`s, never symbol glyphs** (Task 414): the software renderer has no font fallback, so ⚙/✕/★/emoji draw nothing on Windows. Use `ui/icons.slint` (`Icon`, `IconButton`); `apps/desktop/tests/no_font_glyphs.rs` fails on symbols in U+2190–U+2BFF or emoji (allow-list: →). Footer labels must fit the 420 px popup in both languages (`tests/footer_budget.rs`).
- **SQLite temporary data stays in memory** (Task 416, D-073): the bundled SQLite is built with `SQLITE_TEMP_STORE=1` (temp files on disk by default). Every non-test connection on user data calls `db::keep_temp_in_memory` right after opening, before any other statement; in-memory connections are exempt.

## Known pitfalls (copy the relevant ones into each task package, D-077)
- **Anything that copies the database** (backup, pre-restore, migration `.bak`) must first merge FTS (`INSERT INTO clips_fts(clips_fts) VALUES('optimize')`) and VACUUM with `temp_store=MEMORY`: the contentless FTS5 index keeps deleted tokens until a merge (D-062, D-072 B1, Tasks 415, 416).
- **Every new SQLite connection on user data** calls `db::keep_temp_in_memory` first (the bundled build is `SQLITE_TEMP_STORE=1`).
- **Untrusted SQLite files** (restore, import): match the whole unfiltered `sqlite_master`, including hidden `sqlite_*` objects; open on a defensive connection (Task 405).
- **Hot paths:** no FTS `optimize` or VACUUM per capture or eviction; batch them (Task 411, D-062).
- **Flags that follow content** (`no_export`, sensitivity, masked): every path that creates a clip from another clip or from an import must decide each flag explicitly (F-26).
- **Windows-only code** must lint on macOS/Linux, and **UI symbols** must be vector icons (see above).
- **Release scripts:** PowerShell scripts end with `exit 0` (leaked `$LASTEXITCODE`, PR #32); offline tools need every locked crate fetched (PR #34).

## Reused from Darkshot (practices only)
Banned-crate check in the gate; single privacy choke point plus a static sink check; no logging in privacy modules; manual `Debug`; small task packages with an independent reviewer; memory-bank; token ledger; PR-only `main`.
