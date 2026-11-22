# Owner demo: popup and search (after Task 204)

Build: `cargo build --release -p dark-clipboard` (exe: `target\release\dark-clipboard.exe`, about 9.5 MiB).

## Option A: try it with synthetic data (no real clipboard history)

```powershell
$d = "$env:TEMP\dc-demo"; New-Item -ItemType Directory -Force $d | Out-Null
.\target\release\dark-clipboard.exe --seed-demo-clips 10000 --data-dir $d
.\target\release\dark-clipboard.exe --data-dir $d
```

Press `Ctrl+Shift+V`. Type to search (`viet` finds `Việt`), arrows to move, Enter copies the selected clip to the clipboard, Shift+Enter copies as plain text, Alt+1…9 copies the n-th result, Ctrl+P pins, Delete removes (Ctrl+Z or the Undo button within 5 s), Esc closes. Quit from the tray icon. Remove the folder afterwards: `Remove-Item -Recurse $d`.

Note: with `--data-dir`, the app captures your real clipboard into that folder while it runs (capture is on until the first-run welcome of Task 208 exists).

## Option B: real use

`.\target\release\dark-clipboard.exe` (data in `%LOCALAPPDATA%\DarkClipboard`, settings in `%APPDATA%\DarkClipboard`). Copy a few things, then press `Ctrl+Shift+V`.

## What to check (rows in `platform-checklist.md`, section "Windows: popup (Task 204)")

1. Vietnamese IME (Telex, VNI, Unikey) in the search box.
2. Popup position on the monitor under the cursor; mixed-DPI monitors.
3. Shortcut pressed twice quickly, and while the popup is open.
4. 10,000-clip scroll and search speed.
5. Real-shortcut timing: 50 presses, then `powershell -File scripts/measure-popup.ps1 -LogPath <data dir>\logs\dark-clipboard.log -Source hotkey`.
6. Tray menu: pause 5 min, until resumed, private mode, quit.

Not yet available: paste into the previous app (Task 205), image thumbnails and detail pane (206), settings window (207), first-run welcome (208), history window (211).
