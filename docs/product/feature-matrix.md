# Feature Matrix and Product Research

Status: Phase 0, 2026-11-22. This file separates **observed** capabilities of reference products (from their public docs or sites) from **our recommendations**. The products were not installed and exercised in Phase 0. "Observed" means documented by the vendor or project, as of the date above.

## 1. Reference products (observed)

| Product | Platforms | Notable capabilities | Privacy approach | Source |
|---|---|---|---|---|
| CopyQ | Win, macOS, Linux (X11; Wayland partial) | Tabs (collections), tags, notes, item editing, full scripting (JS-like API) and CLI, commands triggered by content/window, image and HTML items, encryption plugin for tabs | Ignore by window title/app via commands; encrypted tabs (GnuPG). On GNOME Wayland, needs its own GNOME Shell extension (CopyQ 14+), which does not work from Flatpak/AppImage | github.com/hluk/CopyQ, copyq.readthedocs.io known-issues |
| Ditto | Windows | SQLite-backed history, groups, search, paste in order, network sync between Ditto instances (encrypted with shared key), plain-text paste, special paste | Respects `Clipboard Viewer Ignore`-style exclusion; app exclusion list | ditto-cp.sourceforge.io |
| Maccy | macOS | Minimal keyboard-first popup, fuzzy search, pinning, ignore apps, ignore by regex, pause | Ignores pasteboard types from nspasteboard.org (concealed, transient, auto-generated) by default | maccy.app (maccy.at) |
| PowerToys Advanced Paste | Windows | Paste as plain text / Markdown / JSON locally; AI transforms via OpenAI/Azure/other providers, and since v0.96 local models (Foundry Local, Ollama); uses Windows clipboard history | Local transforms need no network; AI is opt-in with user-provided keys | learn.microsoft.com/windows/powertoys/advanced-paste |
| ClipboardFusion | Windows (+ mobile sync) | Text scrubbing rules, macros (C#), hotkeys, sync via vendor cloud | Vendor cloud sync is optional | clipboardfusion.com/features |
| Paste | macOS, iOS | Visual timeline, pinboards, iCloud sync, Shortcuts | iCloud sync; app exclusion | pasteapp.io |
| Paste MCP | macOS (Paste 6.6+) | Local MCP server (`npx @pasteapp/mcp`): AI can search clips, pull one into context, save output to a pinboard | Runs locally; per-AI-app approval and revoke in Paste settings | pasteapp.io/mcp, github.com/pasteapp/paste-mcp |
| Linux (cliphist, clipman, GPaste, Klipper, wl-clipboard) | Linux | `wl-paste --watch` piping into a store (cliphist); GPaste is a GNOME Shell integration; Klipper is built into KDE | Most ignore `x-kde-passwordManagerHint=secret` (Klipper, cliphist via option) | project READMEs, KDE docs |

## 2. Patterns worth adopting (recommendations)

| Pattern | From | Adopt in |
|---|---|---|
| Keyboard-first popup: shortcut, type to filter, Enter to copy, modifier to paste as plain text | Maccy, Ditto | P0 |
| Pinning and "ignore apps" list | Maccy, Ditto | P0 |
| Respect the platform's concealed/transient markers by default | Maccy (nspasteboard.org), Windows `ExcludeClipboardContentFromMonitorProcessing` | P0 |
| SQLite as the history store | Ditto | P0 |
| Collections (tabs/pinboards), tags, notes | CopyQ, Paste | P1 |
| Local, non-AI transforms (plain text, JSON pretty, trim, case) | PowerToys Advanced Paste | P1 |
| CLI with machine-readable output | CopyQ | P2 |
| Local MCP server with per-client approval | Paste MCP | P2 (read-only default, stricter scopes than Paste) |
| Optional local-model AI transforms | PowerToys v0.96 | P2, provider plug-in, no default provider |
| Sync | Ditto, Paste, ClipboardFusion | P3 only, end-to-end encrypted, opt-in |

## 3. Patterns to avoid

- Vendor-cloud sync on by default (Paste/iCloud, ClipboardFusion): conflicts with principle 2.
- Full scripting engine in P0 (CopyQ): large attack surface and support load.
- Window-title capture by default: titles often contain document names, email subjects, URLs.
- Requiring a shell extension or a running X server for basic Linux function without telling the user.

## 4. Feature levels

Legend: **P0** core manager (first public release), **P1** organization and productivity, **P2** CLI, MCP, AI and automation, **P3** sync and ecosystem.

| Feature | Level | Notes |
|---|---|---|
| Capture text (plain UTF-8) | P0 | All platforms |
| Capture rich text (HTML, RTF) alongside plain | P0 | Stored as extra representations; preview shows plain |
| Capture images (PNG on disk, thumbnail) | P0 | Size cap, default 20 MiB per image |
| Capture file lists (paths only, never file contents) | P0 | |
| Popup with search (FTS5 prefix plus substring fallback) | P0 | |
| Copy back; paste as plain text | P0 | |
| Auto-paste after select | P0 Windows; P0-optional macOS (Accessibility permission); Linux X11 only, Wayland none | See platform matrix |
| Pin clips | P0 | Pinned clips are exempt from retention |
| Delete clip, clear history | P0 | Secure delete of blobs |
| Pause capture (timed: 5 min / 1 h / until resumed) | P0 | |
| Private mode (nothing captured, tray shows state) | P0 | |
| Exclude apps | P0 | Defaults include known password managers |
| Honor OS and Darkshot exclusion markers | P0 | Mandatory; cannot be turned off |
| Retention: max items, max age, max storage | P0 | Defaults: 1,000 items, 30 days, 500 MiB |
| Sensitive-data detection (secondary) | P0 | Detected sensitive clips are kept for a short time (default 0 = not stored; option "store masked, expire in 2 min") |
| Tray / menu bar, start at login | P0 | |
| Settings UI, always-dark theme | P0 | Darkshot convention |
| i18n English and Vietnamese | P0 | Darkshot convention (all user-visible strings from a table) |
| Favorites (system collection), collections, tags, notes | P1 | |
| Local transforms (plain, JSON pretty/minify, trim, case, URL decode) | P1 | No AI |
| Export/import collection (JSON + blobs zip), backup/restore | P1 | |
| Merge/paste multiple in sequence | P1 | |
| CLI `darkclip` with JSON output | P2 | |
| MCP server (stdio), read-only default | P2 | |
| Optional AI provider plug-in (local first, e.g. Ollama) | P2 | No default provider; per-request consent for cloud |
| Rules/automation (on capture: tag, route to collection) | P2 | Declarative rules, no scripting engine |
| Encrypted-at-rest database (SQLCipher or app-level AEAD) | P2 | See ADR-0003 |
| Sync between own devices (E2E) | P3 | |
| Mobile companion | P3 | |
| Darkshot integration (send image to Darkshot annotate) | P3 | Must respect Darkshot exclusions |
