# Dark Clipboard for Windows: alpha user guide

This is the guide for the internal Windows alpha (0.1.0-alpha). It is built from source and is not released. The view-models behind the screens below (their logic, not the screens themselves) are covered by automated tests; the screens have **not yet been checked by hand on a real desktop**: the rows are listed as untested in the [platform checklist](../testing/platform-checklist.md). If something here does not match what you see, the guide is wrong, not you. macOS and Linux have no app yet.

## Install and start

There is no installer. Build and run it from the repository (see the [README](../../README.md#build-and-run)):

    cargo build --release -p dark-clipboard
    target\release\dark-clipboard.exe

The app starts hidden. Look for its icon in the notification area (you may need to open the overflow arrow). The build that was hashed for this alpha is described in [alpha-artifact.md](../testing/alpha-artifact.md).

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

The app has no network access and sends nothing anywhere. There is no telemetry. The log file contains event names and counts, never clip content.

Limits of this protection: app identity on the clipboard is cooperative, so the markers are the main defence. Verified by hand so far: KeePassXC 2.8.0 and Darkshot. Bitwarden, 1Password, Office and RDP are untested.

## Excluded apps

Settings > Privacy lists apps whose copies are never stored. The built-in list contains password managers such as KeePassXC and Darkshot. You can add your own apps and remove built-in ones after a confirmation (removing the Darkshot entry does not make its clips storable, because Darkshot's markers still exclude them).

## Settings

Settings has four sections: **General** (shortcut, language, start at login, popup behaviour, "Enter also pastes"), **Privacy** (capture mode, the privacy markers note, sensitive-data handling, the kinds of content captured, excluded apps, a "Start capturing..." button if capture is off), **History** (retention limits for items, days and storage size; 0 means unlimited; lowering a limit asks first because old clips can then be removed) and **About** (version, licence notices, show the welcome again). One Save applies the whole form. If the settings file cannot be written, you see a message and the app does not claim it saved.

English and Vietnamese are available; the first run follows your Windows language.

## Start at login

Off by default. Switch it on in Settings > General or with the box in the welcome. It writes one value, `DarkClipboard`, under `HKCU\Software\Microsoft\Windows\CurrentVersion\Run` (the quoted path of the exe, no arguments). If you disable the entry in Task Manager > Startup, Settings shows it as off. If you start the app with `--data-dir`, it never changes the login entry.

## Where your data is

| What | Location |
|---|---|
| History (`history.db`), images (`blobs\`), log (`logs\dark-clipboard.log`) | `%LOCALAPPDATA%\DarkClipboard` |
| Settings | `%APPDATA%\DarkClipboard\settings.json` |
| Start at login | `HKCU\Software\Microsoft\Windows\CurrentVersion\Run`, value `DarkClipboard` |

With `--data-dir <folder>` the history, log and settings are all in that folder. A `--data-dir` that names the default folder (`%LOCALAPPDATA%\DarkClipboard`) is the normal profile: settings stay in `%APPDATA%\DarkClipboard\settings.json`. A second start with the same options forwards `--toggle` or `--quit` to the running app.

## Uninstall and data removal

There is no installer, so there is nothing to uninstall. To remove everything:

1. Quit the app from the tray menu (Quit).
2. Turn off Start at login in Settings first, or delete the Run value afterwards: `reg delete HKCU\Software\Microsoft\Windows\CurrentVersion\Run /v DarkClipboard /f`.
3. Delete the folder `%LOCALAPPDATA%\DarkClipboard` (history, images, log).
4. Delete the folder `%APPDATA%\DarkClipboard` (settings).
5. Delete the exe and, if you cloned the repository for it, the build folder.

If you used `--data-dir`, delete that folder instead of steps 3 and 4. Deleting the folders removes your clipboard history permanently; nothing is stored anywhere else.

## Known limits

See "Known issues" in the [CHANGELOG](../../CHANGELOG.md). Notably: the Windows build has no console, so `--toggle`, `--quit` and `--help` do not set a shell exit code in an interactive terminal; if Explorer restarts the tray icon may disappear until you restart the app; and most UI behaviour described here is untested by hand.
