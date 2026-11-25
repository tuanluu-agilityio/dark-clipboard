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
- **UI icons are vector `Path`s, never symbol glyphs** (Task 414): the software renderer has no font fallback, so ⚙/✕/★/emoji draw nothing on Windows. Use `ui/icons.slint` (`Icon`, `IconButton`); `apps/desktop/tests/no_font_glyphs.rs` fails on symbols in U+2190–U+2BFF or emoji (allow-list: →). The popup header, filter segment, status line and menus must fit the 440 px popup in both languages (`tests/popup_budget.rs`, which replaced `tests/footer_budget.rs` in Task 701 when the text footer went away).
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
- **Settings width budget** (Task 705): the settings nav labels must fit 220 - 2 x 12 - 16 - 8 px, the three capture-mode cards the 680 px minimum window (content 680 - 220 - 2 x 24, 8 px gaps), the save bar (message, Revert, Save with icon and Ctrl+S cap) and every `Select` item its 200 px control, in English and Vietnamese (`apps/desktop/tests/settings_budget.rs`, 7 px per character). Key meaning lives in Rust (`src/ui/settings_keys.rs`); `Select` forwards keys like `Menu`. Every setting has a `// section:N id` marker in `ui/settings.slint` and an entry in `prefs::sections::SETTING_SECTIONS` (`tests/settings_sections.rs`); the Always protected card holds no control (GC-07).
- **Accessibility scans** (Task 707): `apps/desktop/tests/a11y_scan.rs` reads every `ui/**/*.slint` (not the gallery) and fails with file:line when a `Button`, `IconButton`, `NavItem`, `Switch`, `Select`, `Field` or `ModeCard` has no bound accessible name (a literal is not a name), a control component has no `accessible-role`, an icon button is under 28 px or any control under 24 px, or a window lacks its single last `TooltipLayer`; `tests/secondary_budget.rs` holds the width budgets of the secondary surfaces. New controls follow the kit and need no allow-list entry.
- **Integration tests that include app source with `#[path]`** (F-37): put every `#[path]` module at the top level of the test file and re-export it into inline modules (`mod popup { pub use super::popup_keys as keys; }`). Inside an inline module rustfmt on Linux/macOS resolves the path through a non-existent `tests/<module>/` folder and `cargo fmt --check` fails only there (Windows normalises the path).
- **Slint layout traps** (Task 711): Slint centres a child of `Window` that has no `x`/`y`, so a 0x0 anchor such as `Menu` needs `x: 0px; y: 0px;` for `open(x, y)` to take window positions (`tests/layout_scan.rs`). A fixed-size child directly in a `VerticalLayout` caps the layout's cross-axis max width, so a stretched card renders content-sized: wrap it in a `HorizontalLayout { alignment: start; }`.
- **Call out before closing a PopupWindow** (Task 713): closing a `PopupWindow` destroys its repeated items, so a handler that calls `pop.close()` before `root.callback(...)` never reaches the callback. Call out first, then close. `TouchArea.moved` fires only while a button is held; use `changed has-hover` for hover. Regression test: `apps/desktop/tests/menu_click.rs` (headless software renderer).
