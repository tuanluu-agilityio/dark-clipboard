# System Overview

Status: Phase 0, 2026-11-22. Decisions referenced: ADR-0001 (UI), ADR-0002 (monitoring), ADR-0003 (storage), ADR-0004 (platform abstraction), ADR-0005 (AI access), ADR-0006 (process model).

## 1. Layers and crates

> Changed by D-034 (2026-11-22): the UI framework is **Slint** (ADR-0001 accepted), not Tauri; read `apps/desktop (Tauri)` and every `tauri` entry below as Slint. Changed by D-038: on Windows the copy-back owner window is created per write.

```
                 ┌──────────────────────┐ ┌──────────────┐ ┌──────────────┐
 Clients         │ apps/desktop (Tauri) │ │ darkclip-cli │ │ darkclip-mcp │
                 └──────────┬───────────┘ └──────┬───────┘ └──────┬───────┘
                            │ Application API (Rust trait + DTOs, JSON-serializable)
                 ┌──────────▼────────────────────▼────────────────▼───────┐
 Application     │ darkclip-app: ClipService, SearchService, PolicyService,│
                 │ CaptureActor (single serialized clipboard worker),      │
                 │ RetentionJob, PermissionGuard, AuditLog                 │
                 └───────┬──────────────────────┬───────────────────┬─────┘
                         │                      │                   │
 Domain          ┌───────▼────────┐   ┌─────────▼────────┐  ┌───────▼─────────┐
                 │ darkclip-core  │   │ darkclip-storage │  │ darkclip-platform│
                 │ models, policy │   │ SQLite, FTS5,    │  │ traits, caps,    │
                 │ dedupe, detect │   │ blobs, migrations│  │ fake impl        │
                 └────────────────┘   └──────────────────┘  └───────┬─────────┘
                                                                    │ implemented by
 Platform                      ┌────────────────────┬───────────────┼──────────────┐
                               │ darkclip-windows   │ darkclip-macos │ darkclip-linux│
                               │ Win32              │ AppKit (objc2) │ X11 / Wayland │
                               └────────────────────┴────────────────┴──────────────┘
 Optional (P2)   darkclip-ai: provider trait + local providers; called only through PermissionGuard
```

| Crate | Kind | Depends on | Must not depend on |
|---|---|---|---|
| `darkclip-core` | lib | `serde`, `sha2`, `thiserror` | any OS crate, `rusqlite`, `tauri`, any async runtime, the network |
| `darkclip-platform` | lib | `darkclip-core` | OS crates (only traits, capability types, a `FakeClipboard` for tests) |
| `darkclip-storage` | lib | `darkclip-core`, `rusqlite` (bundled, FTS5) | OS crates, `tauri` |
| `darkclip-app` | lib | core, platform, storage | `tauri`, OS crates (adapters are injected) |
| `darkclip-windows` | lib, `cfg(windows)` | platform, core, `windows` | storage, app, tauri |
| `darkclip-macos` | lib, `cfg(target_os = "macos")` | platform, core, `objc2-app-kit` | storage, app, tauri |
| `darkclip-linux` | lib, `cfg(target_os = "linux")` | platform, core, `x11rb`, `wayland-client` + protocols | storage, app, tauri |
| `apps/desktop` | bin `dark-clipboard` | app, the three adapters (cfg), `tauri` | — |
| `darkclip-cli` | bin `darkclip` (P2) | app, storage | `tauri` |
| `darkclip-mcp` | lib, served by `darkclip mcp serve` (P2) | app | `tauri`, any filesystem API except via app |

`scripts/check.ps1` enforces the "must not depend on" column with `cargo tree`, following Darkshot's banned-crate check.

### 1a. Layer view, updated 2026-11-22 (Changed by D-039)

Same architecture as above, redrawn: Slint instead of Tauri, the CLI/MCP write path made explicit (ADR-0006), the two security choke points shown as their own boxes, and reserved places for the P2 Rule Engine and AI.

```
   Desktop UI (Slint)            CLI (P2)                 MCP stdio (P2)
        |                         | reads: DB read-only     |
        |                         +---- writes: IPC --------+--> desktop process (only writer)
        v                                                   v
 +------------------- darkclip-app (Application Services) --------------------+
 | ClipService . SearchService . RetentionJob . Settings                       |
 | [CaptureActor]  the only thread that reads/writes the OS clipboard          |
 | [PermissionGuard]  CLI/MCP scopes, read-only default, never sensitive       |
 | RuleRunner (P2: executes a fixed set of actions) . AI port (P2, optional)   |
 +-------+--------------------------+-----------------------------+-----------+
         v                          v                             v
  darkclip-storage           darkclip-platform              darkclip-ai (P2, optional)
  SQLite . FTS5 . blobs      traits + Capabilities          provider trait, local first
                                    | implemented by
                    +---------------+----------------+
               darkclip-windows  darkclip-macos  darkclip-linux (X11 / Wayland)

  All of the above depend on darkclip-core (models . policy . rule evaluation (P2) .
  hash . sensitive detection . i18n). Core depends on nothing: no I/O, no OS, no SQLite.
```

Placement rules for future parts: rule **evaluation** is pure data + logic in core (deterministic, tested like `policy`); rule **actions** run only in app services, never in adapters. AI providers live in `darkclip-ai` and are reachable only through `PermissionGuard`. A `ClipRepository` trait is introduced only when encryption at rest or sync needs a second storage implementation.

## 2. Runtime model (desktop app)

```
 OS clipboard ──notify──► Platform monitor thread ──ChangeSignal{seq}──► CaptureActor (one thread, owns all
                          (Win: message-only window;                     clipboard reads and writes)
                           macOS: changeCount poll;                         │ 1. coalesce by seq (debounce 75 ms)
                           Linux: XFixes / data-control)                    │ 2. PolicyService::pre_read(formats, source, mode)
                                                                            │      → Skip(reason) | Read(formats)
 UI / CLI / MCP ──commands (copy back, paste)──► CaptureActor               │ 3. read representations (size caps)
                                                                            │ 4. detect sensitive → policy decision
                                                                            │ 5. hash → dedupe → storage (single writer)
                                                                            ▼
                                                                       events → UI (new clip, stats)
```

Rules:

1. **One clipboard worker.** All reads and writes of the OS clipboard go through `CaptureActor` (a thread with an mpsc command queue). No other code path opens the clipboard. On Windows the clipboard is a global lock (`OpenClipboard`), and the spike showed contention (`open_attempts=4` while PowerShell was writing). On X11 and Wayland, ownership of the selection is per client connection. Serializing avoids self-races on all three.
2. **Notification is a hint, the sequence number is truth.** Windows delivered 20 notifications for 20 writes, but by the time each was handled only 2 distinct `GetClipboardSequenceNumber` values were visible (spike S5). The actor processes a change only if `seq != last_processed_seq`. macOS uses `changeCount`; Linux uses a local counter incremented per selection event.
3. **Debounce.** OLE writers (PowerShell/.NET, Office) produced 3 notifications per copy: first only `DataObject`/`Ole Private Data` (delayed rendering), then real formats, then a flush with owner = none (spike S6/S7). The actor waits 75 ms after the last signal before reading, and captures the **source app at the first signal**, because the owner may be gone by the last one.
4. **Policy before read.** Format enumeration and source detection happen before any data is read. Mandatory exclusions short-circuit: data is never read.
5. **Self-write suppression.** After a copy back, the actor records the resulting sequence number and ignores exactly that change (spike S8 confirmed the post-write seq equals the notified seq).
6. **Single DB writer.** The actor and the desktop's command handlers write through one `Storage` handle on one thread (or a mutex-guarded connection). CLI and MCP processes open the database read-only in WAL mode (ADR-0006).
7. **No content in logs.** Logging uses a `Redacted<T>` wrapper; `Clip` and representation types implement `Debug` manually without content (Darkshot pattern from `OcrLine`).

## 3. Data flow on capture (detail)

| Step | Component | Output | Can skip with reason |
|---|---|---|---|
| Signal | platform monitor | `ChangeSignal { seq, at }` | — |
| Snapshot | adapter `snapshot_meta()` | `ClipboardMeta { formats, markers, source: Option<SourceApp>, seq }` | `Busy` (retry ×10, 20 ms) |
| Pre-read policy | core `policy::decide_pre_read` | `Read(plan)` or `Skip(SkipReason)` | `Paused`, `PrivateMode`, `OsConcealed`, `DarkshotExcluded`, `ExcludedApp`, `SelfWrite`, `Unchanged` |
| Read | adapter `read(plan)` | `Vec<Representation>` with caps | `TooLarge`, `ReadFailed` |
| Classify | core `classify` | `ClipKind`, preview text, URL/colour detection | — |
| Sensitive scan | core `sensitive::scan` | `Sensitivity { level, categories }` (no matched text kept) | `SensitiveDropped` per policy |
| Hash + dedupe | core `hash`, storage | existing id or new id | — |
| Store | storage | `ClipId` | `StorageFull` → retention then retry once |
| Notify | app event bus | `AppEvent::ClipAdded { id }` | — |

Skip reasons are counted in memory (`get_storage_stats` exposes counts per reason) but never logged with content or app names by default.

## 4. Error handling

- Each crate has one `thiserror` enum. Errors carry codes and non-sensitive context, never clipboard content, never full file paths of user data in user-facing messages.
- Platform errors map to `PlatformError { kind: Unsupported | PermissionDenied | Busy | Backend(code) }`. `Unsupported` is a normal outcome, used for capability gaps.
- User-facing messages come from the i18n table via an error code, never from `Display` of foreign errors.
- Release profile: `panic = "abort"` (Darkshot). Therefore no `unwrap`/`expect` in non-test code of core, storage, app, adapters (clippy `unwrap_used`, `expect_used` deny in those crates).

## 5. Configuration and paths

| Item | Windows | macOS | Linux |
|---|---|---|---|
| Data dir | `%LOCALAPPDATA%\DarkClipboard` | `~/Library/Application Support/DarkClipboard` | `$XDG_DATA_HOME/dark-clipboard` |
| Config | `%APPDATA%\DarkClipboard\settings.json` | `~/Library/Application Support/DarkClipboard/settings.json` | `$XDG_CONFIG_HOME/dark-clipboard/settings.json` |
| DB | `<data>/history.db` (+ `-wal`, `-shm`) | same | same |
| Blobs | `<data>/blobs/<aa>/<sha256>` | same | same |
| Logs | `<data>/logs/dark-clipboard.log` (content-free, rotated) | same | same |

Data dir is local (not roaming) on Windows on purpose: history must not roam via domain profiles.

## 6. Background lifecycle

- Single instance per user session (Windows: named mutex; macOS/Linux: lock file in runtime dir plus a local socket used for `--toggle`).
- Start at login: Windows `HKCU\...\Run` (Darkshot `autostart.rs` pattern), macOS `SMAppService` (login item), Linux XDG autostart `.desktop`. Tauri's autostart plugin is acceptable if it uses these mechanisms; verify in Phase 3.
- The popup window is created hidden at startup and shown or hidden on demand (keeps NFR-01).
- Shutdown: stop monitor, drain actor queue (deadline 2 s, Darkshot's bounded-exit pattern), checkpoint WAL.

## 7. What is reused from Darkshot (practices, not code)

| Darkshot practice | Dark Clipboard form |
|---|---|
| `darkshot-core` OS-independent, enforced by `cargo tree` ban in `check.ps1` | Same, for core/platform/storage/app |
| `privacy_gate` single choke point plus static check of sinks | `PolicyService::decide_pre_read` is the only path to `read()`; `check.ps1` greps that adapter `read` is called only from `darkclip-app/src/capture.rs` |
| No logging in privacy modules (check enforced) | No `println!`/`dbg!`/`log::*!` with clip types in `capture.rs`, `sensitive/` |
| Manual `Debug` without content | Same for `Representation`, `Clip`, `ClipboardSnapshot` |
| i18n table, compile-time complete | Same macro approach in core |
| Atomic file writes, no panic on real paths | Same |
| Small task packages, separate reviewer, memory-bank, token ledger, protected `main` via PR | Same (see `CLAUDE.md`) |

Darkshot's Win32 code (`darkshot-windows`) is **not** copied into any Dark Clipboard crate. The Windows adapter is written against the Phase 0 spike findings.
