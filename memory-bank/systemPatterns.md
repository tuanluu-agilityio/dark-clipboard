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
- **Single clipboard actor:** only `capture.rs` touches the OS clipboard. Signals are hints; the sequence number is the truth. Debounce 75 ms; source captured at the first signal.
- **Policy before read:** `policy::decide_pre_read` (pure) decides from format names, markers, source and mode. Adapters report and never decide.
- **Single DB writer:** the desktop app writes; CLI/MCP open read-only and send mutations over IPC (P2).
- **Content-free observability:** manual `Debug`, `Redacted<T>`, no logging macros in privacy modules, skip counters instead of logs.
- **No panics in release paths** (`panic = "abort"`): no unwrap/expect in non-test code (clippy deny).
- **Atomic file writes:** temp file plus rename (settings, blobs). Corrupt settings → defaults plus a `.corrupt-<ts>` copy.
- **Time through `Clock`**, never read in core.
- **i18n table** with compile-time completeness (Darkshot `messages!` approach; re-implemented, not copied).
- **Capabilities are data:** `Unsupported` is a normal value; the UI renders capabilities.
- **Tests first;** synthetic secrets assembled at runtime.

## Reused from Darkshot (practices only)
Banned-crate check in the gate; single privacy choke point plus a static sink check; no logging in privacy modules; manual `Debug`; small task packages with an independent reviewer; memory-bank; token ledger; PR-only `main`.
