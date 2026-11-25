# Dark Clipboard

A local-first clipboard manager for Windows, macOS and Linux, written in Rust. It is part of the Darkshot family.

> **Status: 0.2.0 alpha, Windows only.** Windows 11 x64 has a desktop app you can download (see [Download](#download-windows-x64)). macOS and Linux have no app yet and nothing is claimed for them. The latest release is 0.2.0, with the redesigned interface (Phase 7). The app is **unsigned**, and most of its screens have **not been checked by hand** on a real desktop: see [What is tested](#what-is-tested) and the Known issues in the [CHANGELOG](CHANGELOG.md).

## Download (Windows x64)

The latest release is **0.2.0** (the redesigned interface described under "What works"). Releases are on GitHub: <https://github.com/tuanluu-agilityio/dark-clipboard/releases>. Release notes: [docs/user/release-notes-0.1.0.md](docs/user/release-notes-0.1.0.md).

Assets of a release (see [docs/release.md](docs/release.md)):

- `dark-clipboard-<version>-windows-x64.zip`: `dark-clipboard.exe`, `LICENSE`, `README.md`, `CHANGELOG.md` and `THIRD-PARTY-NOTICES.txt`. For 0.2.0 the name is `dark-clipboard-0.2.0-windows-x64.zip`.
- `dark-clipboard.exe` on its own.
- `SHA256SUMS.txt`: the SHA-256 of the zip and of the bare exe.

**Verify the download.** Put `SHA256SUMS.txt` next to the file and, in PowerShell:

    Get-FileHash .\dark-clipboard-0.2.0-windows-x64.zip -Algorithm SHA256
    Get-Content .\SHA256SUMS.txt

The hash must equal the line for that file. This catches a damaged download or one swapped somewhere else; it is not a signature and does not prove who built it, because the build is unsigned.

**The binary is unsigned.** There is no Authenticode certificate, so Windows SmartScreen may show "Windows protected your PC". Only run a file you have verified as above; to continue, choose "More info" and then "Run anyway". On Windows 11, Smart App Control may block an unsigned exe with no "Run anyway" option. There is no installer and nothing to uninstall (see the [user guide](docs/user/windows-alpha.md#uninstall-and-data-removal)).

**Run it.** Unzip, then start `dark-clipboard.exe`. It starts hidden with an icon in the notification area (open the overflow arrow if you do not see it). Press `Ctrl+Shift+V` for the popup. On the first start a welcome appears and **nothing is captured until you press Start**. The first run on a fresh Windows 11 machine is an owner manual row and has not been checked by hand yet.

## What it is meant to do (first release)

- Remembers what you copy (text, rich text, images, file lists) and finds it again as you type.
- Keyboard first: one shortcut, type, Enter.
- **Never stores** what your OS, your password manager or Darkshot marked private. Pause, private mode, excluded apps, retention limits.
- No account, no cloud, no telemetry. Works offline.
- AI tools (via MCP, later release) see nothing unless you grant access, and only what you grant.

On first start a one-screen welcome explains what is never captured and where your data lives. **Nothing is captured until you press Start**; "Not now" keeps capture off, and the welcome stays reachable from Settings > About and the tray. Start at login is off unless you tick it.

## Windows alpha (0.1.x)

An alpha: there is no installer and no signed binary. Download it (above) or build it from source (below). User guide: [docs/user/windows-alpha.md](docs/user/windows-alpha.md). How a release is built and checked: [docs/release.md](docs/release.md); the history of the first hashed alpha build is in [docs/testing/alpha-artifact.md](docs/testing/alpha-artifact.md).

### What works (implemented)

- Tray icon with menu (pause, private mode, history, settings, quit) and a global shortcut (default `Ctrl+Shift+V`). A new app icon (a clipboard on a dark tile) is used for the exe, the taskbar, the window title bars and the tray; the tray adds a small badge for paused or off, private and capture unavailable.
- **Popup** near the cursor (440 x 520, always dark): a header with the search field and four icon buttons (preview, history, settings, close), a filter segment (All, Text, Link, Code, Image, Files, Pinned, Favorites) with a More filters menu (Today, Last 7 days, Rich text, Color, Collections, Apps), 48 px rows with a kind icon, source, age, the `Alt+1`..`Alt+9` number and copy, pin and more buttons on the hovered or selected row, a star on favorite rows, PINNED and RECENT headings, a right-click row menu, delete with undo, and a status line with the capture state and key hints. The preview is a side pane (the popup grows to 760 px; on a monitor narrower than that the pane draws over the list) with plain-text and Copy as buttons, the content (text, image, file list) and facts. A masked clip shows only a fixed "Hidden" line. The menus and buttons only copy; paste stays `Ctrl+Enter`.
- `F1` or `Ctrl+/` opens a keyboard shortcuts sheet in the popup and in the history window; it lists that window's keys, and `Esc` closes it first.
- Copy back with Enter; paste into the previous app with Ctrl+Enter (opt-in "Enter also pastes"). Paste is refused for elevated apps, the taskbar and windows it cannot identify.
- **History window** (opens at 1060 x 660, shrinks to 640 x 460): a sidebar with Library (All, Pinned, Favorites), Types, Collections, Tags and Apps; a toolbar with the filter name, the search field, a date menu (Any time, Today, Last 7 days, Last 30 days) and a select-several toggle; 56 px rows with two preview lines; a 340 px detail panel with a toolbar (copy, plain text, Copy as, Edit a copy, pin, favorite, delete), the preview, facts, and Tags, Note and Collections sections; a status line. Multi-select with a bulk bar (Pin, Favorite, Add to collection, Tag, Export, Delete, Clear), a keyboard context menu, pin and delete (always asks first). Below 1000 px the sidebar becomes an icon rail, below 820 px the details become a drawer over the list.
- **Settings window** (860 x 640, minimum 680 x 480) with five sections in a nav: General, Capture & Privacy, History & Storage, Data & Transfer (backup, restore, import, data folder) and About. Settings sit in cards with a description under each label; capture mode is three cards (Recording, Private, Paused); an "Always protected" card lists the system, password-manager and Darkshot privacy markers, which cannot be turned off. Save and Revert appear in a bar only when something changed. A first-run welcome. English and Vietnamese.
- Start at login through the current user's `Run` registry key (off unless you tick it).
- Software renderer by default (small memory, no idle CPU).

### Keyboard shortcuts

Read from the real key maps (`apps/desktop/src/popup/keys.rs`, `src/history/keys.rs`, `src/ui/settings_keys.rs`); `F1` shows the same lists in the app. The history window's organizing keys are in the table under "Collections, tags, notes and favorites".

| Popup | Action |
|---|---|
| Type, `Up` / `Down`, `PageUp` / `PageDown`, `Home` / `End` | Search and move |
| `Enter` / `Shift+Enter` | Copy / copy as plain text |
| `Ctrl+Enter` | Copy and paste into the previous app (opt-in) |
| `Alt+1` to `Alt+9` | Copy the numbered row |
| `Space`, `Right` / `Left` | Toggle, open / close the preview pane |
| `Ctrl+T` | Copy as... (text transforms) |
| `Ctrl+P` / `Ctrl+D` | Pin / favorite the selected row |
| `Del`, `Ctrl+Z` | Delete (with undo), undo |
| `Shift+F10` or `Menu` | Row menu |
| `Ctrl+K` | Focus the search field |
| `Ctrl+1` to `Ctrl+9` | Filter segment tab |
| `Ctrl+Shift+F` | More filters |
| `Ctrl+H` / `Ctrl+,` | Open the history window / Settings |
| `F1` or `Ctrl+/` | Shortcuts sheet |
| `Esc` | Close the preview, then the popup |

| History window | Action |
|---|---|
| `Up` / `Down`, `PageUp` / `PageDown`, `Home` / `End` | Move |
| `Shift+Down`, `Space`, `Ctrl+A` | Extend the marks, mark a row, select all |
| `Enter` / `Shift+Enter` | Copy / copy as plain text |
| `Ctrl+T` / `Ctrl+E` / `Ctrl+Shift+E` | Copy as... / edit a copy / export |
| `Ctrl+P`, `Ctrl+D`, `Del` | Pin, favorite, delete (asks first) |
| `Ctrl+F` or `Ctrl+K` | Focus the search field |
| `Ctrl+B`, `F6` / `Shift+F6`, `Right` | Rail or sidebar, next / previous part, details drawer (narrow window) |
| `Ctrl+,` | Settings |
| `F1` or `Ctrl+/` | Shortcuts sheet |
| `Esc` | Closes one layer at a time: menu, sidebar overlay, drawer, marks, window |

| Settings window | Action |
|---|---|
| `Ctrl+S` | Save |
| `Ctrl+1` to `Ctrl+5` | Switch section (`Up` / `Down` in the nav also move between sections) |

### What is tested

- **Automated, passing in the quality gate:** the portable core, storage (SQLite with FTS5), capture policy, privacy markers, sensitive-data detection, the application API, settings handling, and the Windows adapter's non-interactive parts. Run `powershell -File scripts/check.ps1`.
- **Tested by hand on one Windows 11 machine (Task 107):** capture from KeePassXC (not stored), Darkshot (not stored), Notepad, Chrome, Explorer file copies and Snipping Tool, and the ignored real-clipboard tests. Details and the apps that were not available (Word, Excel, Bitwarden, 1Password, RDP) are in the [platform checklist](docs/testing/platform-checklist.md).
- **Measured by script, not by hand:** popup open time driven through `--toggle` (p95 about 22-27 ms with 10,000 clips) and memory and idle CPU, in [perf-windows.md](docs/testing/perf-windows.md). The real-shortcut timing is not measured.
- **Phase 7 redesign:** the owner reviewed the demo on 2026-11-25. The checks run by agents are scripts and headless tests (key maps, layout and accessibility scans, budgets, simulated pointer clicks); the individual rows of the "Phase 7 demo checklist" in the [platform checklist](docs/testing/platform-checklist.md) are not marked tested. Measured against 0.1.2 with 10,000 clips: popup opening about 22 ms (was 17 ms) on a quiet machine, the release exe about 850 KiB larger (accepted).
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

At every start the app gives its data folder a protected access list: only your account and SYSTEM are on the folder's access list, and everything inside inherits that (nothing is taken over from the parent folder). It does this only for the default folder or a folder the app created itself (it leaves a small `.dark-clipboard-data` marker file in it), and only when your account owns the folder (or, for an app run as administrator, the Administrators group). A `--data-dir` folder that existed before, for example one that holds other files of yours, is never changed, not even after it holds a `history.db`; the log notes `not_ours`. Administrators are not on the list. The first start after this change rewrites the access list of every file in an existing data folder, which can take a moment with many images; the log records how long (`data_dir_acl ... ms=`). This has not been checked by hand yet (an owner manual row: `icacls` on the folder).

If the history cannot be opened at start, a message window says why instead of the app quitting silently. A history made by a newer Dark Clipboard is never changed: install the newer version. A damaged history can be moved aside: choosing Yes renames `history.db` (with its `-wal` and `-shm` files), the `blobs` folder and any `history.db.pre-...` copies with the mark `damaged-<time>` (`history.db.damaged-<time>`, `history.db.damaged-<time>-wal`, `-shm`, `blobs.damaged-<time>`, `history.db.damaged-<time>.pre-...`) and starts with an empty history; nothing is deleted, and you can then restore a backup. **These moved files keep all your old clips, including ones that would have expired. Retention and Clear history do not touch them.** Delete them yourself from the data folder when you no longer need them. The database is checked at every start (`PRAGMA quick_check`).

### Privacy promises

- Clips that carry the Windows clipboard-history, cloud or monitor-exclusion markers, or that Darkshot marks private, are never stored. Those markers cannot be switched off in the settings.
- Clips from excluded apps are never stored. The list has built-in entries (password managers such as KeePassXC, plus Darkshot) that you can edit after a confirmation. App identity is cooperative, so the markers are the main defence.
- Capture is off on a fresh install until you press Start in the welcome. "Not now" keeps it off.
- No account, no cloud, no telemetry. The app makes no network request, with one opt-in exception: the update check. It is off by default and runs only when you press "Check for updates" in Settings > About or turn on "Automatically check weekly". It sends one HTTPS request to `api.github.com` (the latest release of this project) with only two headers, the fixed `User-Agent: DarkClipboard/<version>` and `Accept: application/vnd.github+json`; no clip data, settings, identifiers or cookies. Your network address is visible to GitHub as in any web request. If the environment variables `HTTPS_PROXY`, `HTTP_PROXY` or `ALL_PROXY` are set, the request goes through that proxy, which then sees that you contact `api.github.com`; the Windows system proxy setting is not used. When the settings file is read-only the toggle is disabled and no weekly check runs (the time of the last check cannot be saved). Only the version number and the release page address are read from the answer, and nothing is downloaded or installed; the button opens the release page in your browser. To keep it off, leave the toggle off and do not press the button.
- Built-in excluded apps on Windows: KeePassXC (`keepassxc.exe`), KeePass 2 (`keepass.exe`), 1Password, Bitwarden and Darkshot.
- Verified: KeePassXC 2.8.0 and Darkshot 0.6.0 clips were not stored (see the checklist). Not verified: KeePass 2, Bitwarden, 1Password, Office, RDP.
- Crash reports: the app removes itself from Windows Error Reporting for your account at every start (`WerAddExcludedApplication`, per user) and asks Windows not to put heap memory in any report of this process (`WerSetFlags` with `WER_FAULT_REPORTING_FLAG_NOHEAP`). This is meant to keep crash dumps, which could hold clips, from being queued or sent; it has not been checked by hand yet (see the checklist). If either call fails the app still starts and notes it in the log.

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
| `Menu` or `Shift+F10` | Open the context menu of the selected row (Up, Down, Home, End, a letter, `Enter`; `Right` opens "Add to collection", `Left` or `Esc` goes back). It has Copy, Copy plain text, Copy as..., Edit a copy..., Pin, Favorite, Add to collection, Remove from collection, Tags, Note, Export, Select all and Delete |
| `Ctrl+F` or `Ctrl+K` | Focus the search field |
| `Ctrl+B` | Collapse the sidebar to a rail of icons, or show it again (on a narrow window it opens over the list) |
| `F6` / `Shift+F6` | Move the keyboard to the next / previous part of the window (sidebar, list, details) |
| `Ctrl+,` | Open Settings |
| `Right` on a narrow window | Show the details as a drawer over the list (`Esc` or `Left` closes it) |

The same actions are buttons in the detail panel's toolbar and context-menu entries; the panel shows the note, the tags and the collections of the selected clip, each tag and collection has a remove button, and the sidebar's Collections header has a New collection button. In the popup, a star marks favorite rows, the filter segment has a **Favorites** tab, **More filters** lists your collections, and `Ctrl+D` favorites or unfavorites the selected row. Favorites are kept by "Clear history" and "Delete old clips" (see below).

A masked clip (one that may contain a secret) takes no add action: it cannot be favorited, noted, tagged or added to a collection, so a secret can never become a favorite that retention skips. Unfavoriting and removing it from a collection still work. A multi-select that includes masked clips leaves them out and says how many.

**Snippets** are not a separate feature: make a collection called "Snippets" and pin or favorite the clips you reuse. There is no template or variable engine.

### Data management

Settings > History shows what is stored (clips, pinned, favorites, database size, blob size, clips per kind) and has cleanup tools. Each one asks first and names how many clips or files it will touch: **Clear history** (keeps pinned clips and favorites unless you untick the box), **Delete old clips** (older than N days; pinned clips and favorites are kept), **Remove orphaned files** (data files no clip uses; reports how many and how much space it freed) and **Compact database** (needs free disk space of twice the database size, otherwise it refuses). Deletions are real: when you delete (a clip, Clear history, Delete old clips, Compact), the search index is merged and the write-ahead log truncated at once, so the deleted words are gone from the database file as well. Clips removed automatically by the retention limits leave the database file within about 10 minutes (the next periodic merge, or at shutdown). Everything runs locally.

### Backup, restore, export and import

Settings > History > **Backup and transfer**:
- **Back up now...** writes your whole history (clips, images, collections, tags, notes, favorites) to a `.dcbackup` file you choose. Masked clips and clips that have already expired are left out. **A backup is a plain zip file and is not encrypted.** It contains every other clip, including clips the source app asked not to sync to the cloud. If you save it in a folder that a sync client uploads (OneDrive, Dropbox, Google Drive), your whole history is uploaded with it; keep backups out of synced folders. While it is written the file is named `<your file>.dark-clipboard-partial`; it gets its real name only when it is complete. If the app stops in the middle, such a partial file can be left behind and is safe to delete. A backup that the app could not open again (too many or too large files) is refused before anything is written.
- **Restore from backup...** replaces your whole history with a backup after you confirm. The file is checked completely first (names, sizes, checksums, the database itself); a file that is damaged, unsafe or made by a newer Dark Clipboard is refused with a message and nothing changes. Your current history is kept next to it as `history.db.pre-restore-<time>` until Dark Clipboard starts again. A restore needs free disk space of about twice the backup plus your current database; with less (or when the free space cannot be measured) it is refused. Retention limits then apply to the restored clips as usual. A restored history is not scanned again for sensitive content: it holds what your history held when the backup was made (masked clips were left out then). Backups, restores, exports and imports run in the background. A pause or private mode chosen meanwhile (for example from the tray) takes effect before Dark Clipboard looks at the clipboard again, including for a copy made just before the operation started.
- **Import clips...** adds the clips of a `.dcexport` file. Nothing in the file is trusted or run: kinds, previews and hashes are worked out again, the source app is dropped, dates in the future become now, secrets follow your sensitive-content setting (dropped by default), turned-off kinds, file lists and over-limit clips are skipped, and clips you already have are merged (no second copy). The result names how many were imported, merged and skipped, per reason. Import is refused in private mode.

In the history window, **Ctrl+Shift+E** exports the collection the list is filtered to, or else the marked clips, to a `.dcexport` file you choose. Masked and sensitive clips are never exported, and neither are clips the source app marked "do not upload to the cloud" (Windows `CanUploadToCloudClipboard` = 0); the message says how many were left out. Clips captured before this rule existed carry no such mark and can be exported. A backup keeps them (it is your own full copy). An export file holds the full content of its clips in its data files, and in `export.json` their previews (up to 512 characters), notes, tag names and the collection name; it carries no source app. All of this is local; the file picker is the Windows one (not available on macOS and Linux yet).

### Known limits

See "Known issues" in the [CHANGELOG](CHANGELOG.md). In short: the Windows build has no console, so `--toggle`, `--quit` and `--help` do not set a shell exit code interactively; a popup opened from the tray only copies; an Explorer restart may remove the tray icon; GNOME on Wayland may not support background capture later.

## Platform status

Only Windows (11, x64) has been tested, and only partly (see the alpha section); Windows 10 is untested. See `docs/architecture/platform-compatibility.md` for every capability with its evidence (tested, documented, reported or unknown). macOS and Linux are designed but untested. GNOME on Wayland may not support background capture.

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
