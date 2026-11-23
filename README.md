# Dark Clipboard

A local-first clipboard manager for Windows, macOS and Linux, written in Rust. It is part of the Darkshot family.

> **Status: Windows alpha (0.1.0-alpha, internal).** A Windows desktop app exists and runs from source; it has not been released, tagged or published. macOS and Linux have no app yet. See [Windows alpha](#windows-alpha-010-alpha-internal) below for what is verified and what is not.

## What it is meant to do (first release)

- Remembers what you copy (text, rich text, images, file lists) and finds it again as you type.
- Keyboard first: one shortcut, type, Enter.
- **Never stores** what your OS, your password manager or Darkshot marked private. Pause, private mode, excluded apps, retention limits.
- No account, no cloud, no telemetry. Works offline.
- AI tools (via MCP, later release) see nothing unless you grant access, and only what you grant.

On first start a one-screen welcome explains what is never captured and where your data lives. **Nothing is captured until you press Start**; "Not now" keeps capture off, and the welcome stays reachable from Settings > About and the tray. Start at login is off unless you tick it.

## Windows alpha (0.1.0-alpha, internal)

An internal alpha for the owner's own testing. It is built from source; there is no installer, no signed binary and no release. User guide: [docs/user/windows-alpha.md](docs/user/windows-alpha.md). The exact build that was hashed is in [docs/testing/alpha-artifact.md](docs/testing/alpha-artifact.md).

### What works (implemented)

- Tray icon with menu (pause, private mode, history, settings, quit) and a global shortcut (default `Ctrl+Shift+V`).
- Popup near the cursor: search as you type (accent-insensitive), filters, previews for text, images and file lists, pin, delete with undo.
- Copy back with Enter; paste into the previous app with Ctrl+Enter (opt-in "Enter also pastes"). Paste is refused for elevated apps, the taskbar and windows it cannot identify.
- History window with filters, multi-select, pin, delete (always asks first) and a detail pane.
- Settings window (shortcut, language, start at login, capture mode, retention, excluded apps, about and licences) and a first-run welcome. English and Vietnamese.
- Start at login through the current user's `Run` registry key (off unless you tick it).
- Software renderer by default (small memory, no idle CPU).

### What is tested

- **Automated, passing in the quality gate:** the portable core, storage (SQLite with FTS5), capture policy, privacy markers, sensitive-data detection, the application API, settings handling, and the Windows adapter's non-interactive parts. Run `powershell -File scripts/check.ps1`.
- **Tested by hand on one Windows 11 machine (Task 107):** capture from KeePassXC (not stored), Darkshot (not stored), Notepad, Chrome, Explorer file copies and Snipping Tool, and the ignored real-clipboard tests. Details and the apps that were not available (Word, Excel, Bitwarden, 1Password, RDP) are in the [platform checklist](docs/testing/platform-checklist.md).
- **Measured by script, not by hand:** popup open time driven through `--toggle` (p95 about 22-27 ms with 10,000 clips) and memory and idle CPU, in [perf-windows.md](docs/testing/perf-windows.md). The real-shortcut timing is not measured.
- **Untested by hand:** almost every Windows UI row, including the tray, the real shortcut, popup keys, paste into Notepad, VS Code, Edge or Word, elevated targets, the history and settings windows, the welcome, start at login, mixed-DPI monitors, Vietnamese input methods and screen readers. These are owner manual rows in the [platform checklist](docs/testing/platform-checklist.md) marked **untested**. Since D-048 agents never take screenshots, click or type on the owner's desktop and run the app only with a temporary data folder; the one screen capture an agent took earlier (Task 204) was deleted at once (D-048).
- **macOS and Linux:** untested, and there is no app for them.

### Build and run

Requires a recent stable Rust toolchain on Windows 11.

    cargo build --release -p dark-clipboard
    target\release\dark-clipboard.exe

The app starts hidden with a tray icon. Open the popup with `Ctrl+Shift+V`. On the first start a welcome appears and **nothing is captured until you press Start**. To try it without touching your own history, add `--data-dir <empty folder>` (the folder then also holds the settings; the login entry cannot be changed in that mode).

### Where your data lives

| What | Default location |
|---|---|
| History database, image blobs, log | `%LOCALAPPDATA%\DarkClipboard` (`history.db`, `blobs\`, `logs\`) |
| Settings | `%APPDATA%\DarkClipboard\settings.json` |
| Start at login | Registry value `DarkClipboard` under `HKCU\Software\Microsoft\Windows\CurrentVersion\Run` (only if you turn it on) |

With `--data-dir <folder>` the history, log and settings all live in that folder. The log never contains clip content.

### Privacy promises

- Clips that carry the Windows clipboard-history, cloud or monitor-exclusion markers, or that Darkshot marks private, are never stored. Those markers cannot be switched off in the settings.
- Clips from excluded apps are never stored. The list has built-in entries (password managers such as KeePassXC, plus Darkshot) that you can edit after a confirmation. App identity is cooperative, so the markers are the main defence.
- Capture is off on a fresh install until you press Start in the welcome. "Not now" keeps it off.
- No account, no cloud, no telemetry. The app makes no network request, with one opt-in exception: the update check. It is off by default and runs only when you press "Check for updates" in Settings > About or turn on "Automatically check weekly". It sends one HTTPS request to `api.github.com` (the latest release of this project) with only two headers, the fixed `User-Agent: DarkClipboard/<version>` and `Accept: application/vnd.github+json`; no clip data, settings, identifiers or cookies. Your network address is visible to GitHub as in any web request. If the environment variables `HTTPS_PROXY`, `HTTP_PROXY` or `ALL_PROXY` are set, the request goes through that proxy, which then sees that you contact `api.github.com`; the Windows system proxy setting is not used. When the settings file is read-only the toggle is disabled and no weekly check runs (the time of the last check cannot be saved). Only the version number and the release page address are read from the answer, and nothing is downloaded or installed; the button opens the release page in your browser. To keep it off, leave the toggle off and do not press the button.
- Verified: KeePassXC 2.8.0 and Darkshot 0.6.0 clips were not stored (see the checklist). Not verified: Bitwarden, 1Password, Office, RDP.

### Text transforms and edited copies

Select a text clip and press `Ctrl+T` (popup or history window) or use "Copy as...": pick a transform (trim, line endings, upper or lower case, JSON pretty or minify, URL or Base64 encode or decode, HTML to text), check the result in the preview and press Copy. The result goes to the clipboard as Dark Clipboard's own write; nothing is stored. In the history window, "Edit..." (`Ctrl+E`) opens a copy of a text clip in an editor and "Save as new clip" stores the edit as a new clip; the original never changes. A text that looks like a secret follows your sensitive-content setting (dropped by default). Masked clips, images and files have neither. All of this runs locally.

### Collections, tags, notes and favorites

The history window's left pane lists **Favorites**, your **collections** and your **tags**, each with a count; selecting one filters the list (select it again to clear it; one collection and one tag can combine with the other filters). Everything has a key:

| Key (history window) | Action |
|---|---|
| `Ctrl+D` | Favorite or unfavorite the marked clips (several: a confirmation shows the count) |
| `Ctrl+N` | Edit the note of the selected clip (up to 4,000 characters, with a counter; `Ctrl+Enter` saves, `Esc` cancels) |
| `Ctrl+G` | Edit the tags of the selected clip as chips (`Enter` adds, `Backspace` in the empty box removes the last one; letters, digits, `-` and `_`, 40 characters, 20 tags at most), or add one tag to several marked clips |
| `Ctrl+L` / `Ctrl+Shift+L` | Add the marked clips to a collection / remove them from one (pick with the arrows, `Enter`) |
| `Ctrl+Shift+N` | New collection |
| `Ctrl+Shift+R` / `Ctrl+Shift+Del` | Rename / delete the collection the list is filtered to (deleting asks first and names it; its clips stay in your history) |

The same actions are buttons and context-menu entries; the detail pane shows the favorite star, the note and the tags of the selected clip, and each tag has a remove button. In the popup, a star marks favorite rows, the chip row has **Favorites** and your collections (the first six), and `Ctrl+D` favorites or unfavorites the selected row. Favorites are kept by "Clear history" and "Delete old clips" (see below).

A masked clip (one that may contain a secret) takes no add action: it cannot be favorited, noted, tagged or added to a collection, so a secret can never become a favorite that retention skips. Unfavoriting and removing it from a collection still work. A multi-select that includes masked clips leaves them out and says how many.

**Snippets** are not a separate feature: make a collection called "Snippets" and pin or favorite the clips you reuse. There is no template or variable engine.

### Data management

Settings > History shows what is stored (clips, pinned, favorites, database size, blob size, clips per kind) and has cleanup tools. Each one asks first and names how many clips or files it will touch: **Clear history** (keeps pinned clips and favorites unless you untick the box), **Delete old clips** (older than N days; pinned clips and favorites are kept), **Remove orphaned files** (data files no clip uses; reports how many and how much space it freed) and **Compact database** (needs free disk space of twice the database size, otherwise it refuses). Deletions are real: when you delete (a clip, Clear history, Delete old clips, Compact), the search index is merged and the write-ahead log truncated at once, so the deleted words are gone from the database file as well. Clips removed automatically by the retention limits leave the database file within about 10 minutes (the next periodic merge, or at shutdown). Everything runs locally.

### Backup, restore, export and import

Settings > History > **Backup and transfer**:
- **Back up now...** writes your whole history (clips, images, collections, tags, notes, favorites) to a `.dcbackup` file you choose. Masked clips and clips that have already expired are left out. While it is written the file is named `<your file>.dark-clipboard-partial`; it gets its real name only when it is complete. If the app stops in the middle, such a partial file can be left behind and is safe to delete. A backup that the app could not open again (too many or too large files) is refused before anything is written.
- **Restore from backup...** replaces your whole history with a backup after you confirm. The file is checked completely first (names, sizes, checksums, the database itself); a file that is damaged, unsafe or made by a newer Dark Clipboard is refused with a message and nothing changes. Your current history is kept next to it as `history.db.pre-restore-<time>` until Dark Clipboard starts again. A restore needs free disk space of about twice the backup plus your current database; with less (or when the free space cannot be measured) it is refused. Retention limits then apply to the restored clips as usual. A restored history is not scanned again for sensitive content: it holds what your history held when the backup was made (masked clips were left out then). Backups, restores, exports and imports run in the background. A pause or private mode chosen meanwhile (for example from the tray) takes effect before Dark Clipboard looks at the clipboard again, including for a copy made just before the operation started.
- **Import clips...** adds the clips of a `.dcexport` file. Nothing in the file is trusted or run: kinds, previews and hashes are worked out again, the source app is dropped, dates in the future become now, secrets follow your sensitive-content setting (dropped by default), turned-off kinds, file lists and over-limit clips are skipped, and clips you already have are merged (no second copy). The result names how many were imported, merged and skipped, per reason. Import is refused in private mode.

In the history window, **Ctrl+Shift+E** exports the collection the list is filtered to, or else the marked clips, to a `.dcexport` file you choose. Masked and sensitive clips are never exported, and neither are clips the source app marked "do not upload to the cloud" (Windows `CanUploadToCloudClipboard` = 0); the message says how many were left out. Clips captured before this rule existed carry no such mark and can be exported. A backup keeps them (it is your own full copy). An export file holds the full content of its clips in its data files, and in `export.json` their previews (up to 512 characters), notes, tag names and the collection name; it carries no source app. All of this is local; the file picker is the Windows one (not available on macOS and Linux yet).

### Known limits

See "Known issues" in the [CHANGELOG](CHANGELOG.md). In short: the Windows build has no console, so `--toggle`, `--quit` and `--help` do not set a shell exit code interactively; a popup opened from the tray only copies; the context menu in the history window is mouse only; an Explorer restart may remove the tray icon; GNOME on Wayland may not support background capture later.

## Platform status

Only Windows has been tested, and only partly (see the alpha section). See `docs/architecture/platform-compatibility.md` for every capability with its evidence (tested, documented, reported or unknown). macOS and Linux are designed but untested. GNOME on Wayland may not support background capture.

## Documentation

- Product: [vision](docs/product/product-vision.md), [features](docs/product/feature-matrix.md), [MVP scope](docs/product/mvp-scope.md), [journeys](docs/product/user-journeys.md)
- Architecture: [overview](docs/architecture/system-overview.md), [platforms](docs/architecture/platform-compatibility.md), [data model](docs/architecture/data-model.md), [AI interfaces](docs/architecture/ai-integration.md)
- Security: [threat model](docs/security/threat-model.md), [privacy design](docs/security/privacy-design.md)
- Decisions: [ADRs](docs/adr/)
- Plans: [roadmap](docs/plans/roadmap.md), [phase plans](docs/plans/), [task packages](docs/tasks/)
- Spikes: [Windows clipboard](docs/spikes/windows-clipboard-monitor.md), [UI shell comparison](docs/spikes/ui-shell-comparison.md)

## Development

Contributors and AI agents: start with `CLAUDE.md` / `AGENTS.md` and `memory-bank/activeContext.md`. Quality gate: `powershell -File scripts/check.ps1` (Windows) or `bash scripts/check.sh`.

Windows app: `cargo run -p dark-clipboard` starts the app hidden with a tray icon; `Ctrl+Shift+V`, a tray click or `--toggle` opens the popup near the cursor: type to search, arrows to move, Enter to copy back (the focus returns to the app you were in), Ctrl+Enter to copy and paste into that app (never into an elevated app or the taskbar: then it only copies and says so), Esc to close (a second start, `--toggle` or `--quit` talks to the running one through a per-user named pipe). To try a large history without touching your own: `cargo run -p dark-clipboard -- --seed-demo-clips 10000 --data-dir <empty folder>`, then `cargo run -p dark-clipboard -- --data-dir <same folder>` (synthetic text only; refused for the default data folder). Developer run (Windows only for now, no window): `cargo run -p dark-clipboard -- --headless` captures the clipboard into `%LOCALAPPDATA%\DarkClipboard` until Ctrl+C. `--quit` does not stop a `--headless` run (it has no instance pipe): stop it with Ctrl+C, or end the process in Task Manager. `--data-dir <path>` picks another folder. On Windows the UI draws with Slint's software renderer (much less memory and no idle CPU than the GPU renderer); setting the environment variable `SLINT_BACKEND` (for example `SLINT_BACKEND=winit-femtovg`) overrides that choice, and the start line in the log records `renderer=software` (requested), `default` or `env`. If selecting the software renderer fails the log has `renderer_fallback`, but the app may then not start at all. A content-free log is written to `<data dir>\logs\dark-clipboard.log`; on stop it records how many changes were skipped, per reason (names and counts only). Windows capture was tested on a real clipboard in Task 107 (Windows 11; see the [platform checklist](docs/testing/platform-checklist.md)).

## Licence

GPL-3.0-only (see [LICENSE](LICENSE)). The UI uses Slint under its GPLv3 option.
