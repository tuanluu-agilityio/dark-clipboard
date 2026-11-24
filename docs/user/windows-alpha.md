# Dark Clipboard 0.1.0 for Windows: user guide

This is the guide for Dark Clipboard 0.1.0, an **alpha** for Windows 11 x64 (the file keeps its old name so that links to it still work). macOS and Linux have no app yet. The logic behind the screens below is covered by automated tests, but the screens have **not yet been checked by hand on a real desktop**: those rows are listed as untested in the [platform checklist](../testing/platform-checklist.md). If something here does not match what you see, the guide is wrong, not you. The binary is unsigned.

## Install and start

There is no installer. Download the zip from the GitHub release, check its SHA-256 and unzip it (see the [README](../../README.md#download-windows-x64) and [release.md](../release.md#verifying-a-download)), then start `dark-clipboard.exe`. Windows SmartScreen may warn because the file is unsigned. To build it yourself instead (see the [README](../../README.md#build-and-run)):

    cargo build --release -p dark-clipboard
    target\release\dark-clipboard.exe

The app starts hidden. Look for its icon in the notification area (you may need to open the overflow arrow). Settings > About shows the version (0.1.0).

## First run

On the very first start a welcome window opens. It shows your shortcut, what is never captured, where your data lives, and says there is no account and no cloud.

- **Nothing is captured until you press Start.** Until then the tray tooltip says capture is off and copies are not stored.
- **Start** turns capture on. The "Start with Windows" box is unchecked by default.
- **Not now** (or Esc, or the X) keeps capture off. You can start later from the tray menu ("Start capturing...") or from Settings > Privacy and Settings > About.
- If you already had a settings file from an earlier build, the welcome is skipped and your capture state is kept.

## Shortcut and keys

The default shortcut is `Ctrl+Shift+V`. It opens the popup near the cursor, on the monitor under it. Pressing it again closes the popup. A tray icon click does the same. You can change the shortcut in Settings > General; it must include Ctrl, Alt or Win. If another program already owns the shortcut you get a warning and no shortcut is registered until you change it or restart the app.

## The popup

The search box is focused when the popup opens. Recent clips are listed, pinned ones first. Type to search (accents are ignored, so `viet` finds `Việt`). Chips filter by kind, pinned, today, 7 days and source app. Scroll to load more.

| Key | Action |
|---|---|
| Up, Down, Page Up, Page Down, Home, End | Move in the list |
| Enter | Copy the clip back to the clipboard and return to the app you were in |
| Double-click a row | Copy the clip back and return to the app you were in; it never pastes, even with "Enter also pastes" on |
| Ctrl+Enter | Copy back and paste into that app |
| Shift+Enter | Copy as plain text |
| Alt+1 .. Alt+9 | Copy the n-th result |
| Ctrl+P | Pin or unpin |
| Delete | Delete the clip; an Undo toast stays for 5 seconds |
| Ctrl+H | Open the history window |
| Tab, Shift+Tab | Move between search, chips and list |
| Esc, or a click elsewhere | Close the popup |

Every action also has a button. Images show a thumbnail and the details pane shows the first part of long text, image information or file lists. A clip that was masked (for example because it was flagged sensitive) shows only a "hidden" note.

## History window

Open it from the tray ("History...") or with Ctrl+H in the popup. It has filters on the left, the list in the middle and details on the right. You can select several clips (Ctrl+click, Shift+click, Shift+arrows, Space, Ctrl+A), copy, copy as plain text, pin and delete. **Delete always asks for confirmation** in this window. Its context menu is mouse only; every action also has a key. The window remembers its size and position.

## Paste back and its safety rules

Enter only puts the clip on the clipboard and gives the focus back to your previous window; it never types anything. Ctrl+Enter also sends Ctrl+V. The setting "Enter also pastes" (Settings > General, off by default) makes Enter paste too.

Dark Clipboard never pastes blindly. It pastes only when all of this is true:

- the window you were in when the popup opened still exists and is the same program;
- it is a normal visible window (never the taskbar, the desktop or a tool window);
- it accepts the focus;
- Shift, Alt, Win and Enter are released.

If any check fails, or the target runs as administrator (Windows does not let a normal app send input to it), the clip stays on the clipboard and you see "Copied - paste manually": press Ctrl+V yourself. When the popup was opened from the tray icon the previous window is the taskbar, so Ctrl+Enter only copies there.

## Pause, private mode and capture

From the tray menu:

- **Pause**: 5 minutes, 1 hour, or until resumed. The icon and tooltip change. Copies made while paused are never stored, not even later.
- **Private mode**: the same idea, shown as private; switch it off from the same menu.
- **Resume**: back to normal.

You can also pick the mode in Settings > Privacy.

## What is never stored

- Clips whose source marked them private: the Windows clipboard-history, cloud and monitor-exclusion markers, and Darkshot's markers. This is always on and is not configurable.
- Clips from excluded apps (below).
- Anything that arrives while capture is off, paused or in private mode.

No account, no cloud, no telemetry. The app makes no network request, with one opt-in exception: the update check (see "Update check" below). The log file contains event names and counts, never clip content.

Limits of this protection: app identity on the clipboard is cooperative, so the markers are the main defence. Verified by hand so far: KeePassXC 2.8.0 and Darkshot 0.6.0. KeePass 2, Bitwarden, 1Password, Office and RDP are untested.

## Excluded apps

Settings > Privacy lists apps whose copies are never stored. The built-in list contains KeePassXC, KeePass 2 (`keepass.exe`; only in a settings file created by 0.1.0, an older settings file keeps its own list), 1Password, Bitwarden and Darkshot. You can add your own apps and remove built-in ones after a confirmation (removing the Darkshot entry does not make its clips storable, because Darkshot's markers still exclude them).

## Settings

Settings has four sections: **General** (shortcut, language, start at login, popup behaviour, "Enter also pastes"), **Privacy** (capture mode, the privacy markers note, sensitive-data handling, the kinds of content captured, excluded apps, a "Start capturing..." button if capture is off), **History** (retention limits for items, days and storage size; 0 means unlimited; lowering a limit asks first because old clips can then be removed; storage and cleanup; backup and transfer) and **About** (version, licence notices, show the welcome again, the update check). One Save applies the whole form. If the settings file cannot be written, you see a message and the app does not claim it saved.

English and Vietnamese are available; the first run follows your Windows language.

## Collections, tags, notes and favorites

In the history window the left pane lists **Favorites**, your **collections** and your **tags**, each with a count; pick one to filter the list. Keys: `Ctrl+D` favorite, `Ctrl+N` note (up to 4,000 characters), `Ctrl+G` tags, `Ctrl+L` / `Ctrl+Shift+L` add to / remove from a collection, `Ctrl+Shift+N` new collection, `Ctrl+Shift+R` rename, `Ctrl+Shift+Del` delete a collection (asks first; its clips stay). The popup shows a star on favorites and has Favorites and collection chips; `Ctrl+D` toggles the favorite. A masked clip takes no add action. "Snippets" are a collection plus pinned clips; there is no template engine.

## Transforms and edited copies

`Ctrl+T` (popup or history window) opens "Copy as...": trim, line endings, upper or lower case, JSON pretty or minify, URL encode or decode, Base64 encode or decode, HTML to text. You see the result before you copy it; copying writes it to the clipboard and stores nothing. Input over 1 MiB is refused. "Edit..." (`Ctrl+E`, history window) opens a text clip in an editor; "Save as new clip" (or `Ctrl+Enter`) stores a new clip that goes through the normal rules (a secret is dropped or masked as your setting says). The original never changes. Images, files and masked clips have neither.

## Data management

Settings > History shows how much is stored and has cleanup tools, each with a confirmation that names the count: **Clear history** (keeps pinned clips and favorites unless you untick the box), **Delete old clips** (pinned clips and favorites are kept), **Remove orphaned files** and **Compact database** (needs free disk space of twice the database size). Deleting is real: the search index is merged and the write-ahead log truncated at once, so deleted words do not stay in `history.db`. Clips removed automatically by the retention limits leave the file within about 10 minutes.

## Backup, restore, export and import

Settings > History > Backup and transfer (the file dialogs are the Windows ones and were never run by hand):

- **Back up now...** writes a `.dcbackup` file with your history, images, collections, tags, notes and favorites. Masked and expired clips are left out. **It is a plain zip and is not encrypted**, and it keeps clips that apps asked not to sync to the cloud: if you save it in a synced folder (OneDrive, Dropbox, Google Drive) your whole history is uploaded. Keep backups out of synced folders.
- **Restore from backup...** asks first and checks the whole file before changing anything. A damaged, unsafe or newer file is refused with a message and nothing changes. Your previous history is kept as `history.db.pre-restore-<time>` until the app starts again. A restore needs free disk space of about twice the backup plus your database.
- **Import clips...** adds clips from a `.dcexport` file. Nothing in the file is trusted: kinds, previews and hashes are worked out again, sources are dropped, secrets follow your setting, file lists are skipped, duplicates are merged. Refused in private mode.
- **Export** (`Ctrl+Shift+E` in the history window) writes the collection you are looking at, or the marked clips. Masked and sensitive clips and clips marked "do not upload to the cloud" are never exported; the message says how many were left out.

## Update check

Off by default. Settings > About has "Check for updates" and "Automatically check weekly". A check is one HTTPS request to `api.github.com` (this project's latest release) with only a fixed `User-Agent: DarkClipboard/<version>` and an `Accept` header: no clip data, settings or identifiers. Your network address is visible to GitHub as in any web request; `HTTPS_PROXY`, `HTTP_PROXY` and `ALL_PROXY` are honoured, the Windows system proxy is not. The answer is either "You are up to date" or "New version available: X" with a button that opens the release page in your browser. Nothing is downloaded or installed. With the toggle off the app makes no request unless you press the button. With a read-only settings file the toggle is disabled. This has not been checked by hand.

## If the app cannot start

At start the app checks its history database and its connection name, and tells you instead of quitting silently:

- **A history from a newer Dark Clipboard:** a message explains it and nothing is changed. Install the newer version.
- **A damaged history:** a message offers to move it aside. Yes renames `history.db` (with its `-wal` and `-shm` files), `blobs` and any `history.db.pre-...` copies with the mark `damaged-<time>` and starts with an empty history; nothing is deleted and you can then restore a backup. The moved files keep all your old clips, including ones that would have expired, and retention and Clear history do not touch them: delete them yourself when you no longer need them. `--headless` only prints the explanation.
- **"Dark Clipboard could not start: another program is using its connection.":** another program holds the app's named pipe. Close it or end that process and start again. A previous Dark Clipboard that is still shutting down is waited for about a second.

## Start at login

Off by default. Switch it on in Settings > General or with the box in the welcome. It writes one value, `DarkClipboard`, under `HKCU\Software\Microsoft\Windows\CurrentVersion\Run` (the quoted path of the exe, no arguments). If you disable the entry in Task Manager > Startup, Settings shows it as off. If you start the app with `--data-dir`, it never changes the login entry.

## Where your data is

| What | Location |
|---|---|
| History (`history.db`), images (`blobs\`), log (`logs\dark-clipboard.log`) | `%LOCALAPPDATA%\DarkClipboard` |
| Settings | `%APPDATA%\DarkClipboard\settings.json` |
| Start at login | `HKCU\Software\Microsoft\Windows\CurrentVersion\Run`, value `DarkClipboard` |

With `--data-dir <folder>` the history, log and settings are all in that folder. A `--data-dir` that names the default folder (`%LOCALAPPDATA%\DarkClipboard`) is the normal profile: settings stay in `%APPDATA%\DarkClipboard\settings.json`. A second start with the same options forwards `--toggle` or `--quit` to the running app. At every start the app gives the data folder a protected access list (only your account and SYSTEM), but only for the default folder or a folder it created itself (it leaves a `.dark-clipboard-data` marker); a `--data-dir` folder that existed before 0.1.0 is never changed. It also removes itself from Windows Error Reporting for your account, so that crash reports do not carry clips; this has not been checked by hand.

## Uninstall and data removal

There is no installer, so there is nothing to uninstall. To remove everything:

1. Quit the app from the tray menu (Quit).
2. Turn off Start at login in Settings first, or delete the Run value afterwards: `reg delete HKCU\Software\Microsoft\Windows\CurrentVersion\Run /v DarkClipboard /f`.
3. Delete the folder `%LOCALAPPDATA%\DarkClipboard` (history, images, log).
4. Delete the folder `%APPDATA%\DarkClipboard` (settings).
5. Delete the exe and, if you cloned the repository for it, the build folder.

If you used `--data-dir`, delete that folder instead of steps 3 and 4. Deleting the folders removes your clipboard history permanently; nothing is stored anywhere else.

## Known limits

See "Known issues" in the [CHANGELOG](../../CHANGELOG.md). Notably: the Windows build has no console, so `--toggle`, `--quit` and `--help` do not set a shell exit code in an interactive terminal; if Explorer restarts the tray icon may disappear until you restart the app; a backup is not encrypted; and most UI behaviour described here is untested by hand.
