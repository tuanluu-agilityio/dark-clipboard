# Owner demo: popup and search (after Task 204)

Build: `cargo build --release -p dark-clipboard` (exe: `target\release\dark-clipboard.exe`, about 9.5 MiB).

## Option A: try it with synthetic data (no real clipboard history)

```powershell
$d = "$env:TEMP\dc-demo"; New-Item -ItemType Directory -Force $d | Out-Null
.\target\release\dark-clipboard.exe --seed-demo-clips 10000 --data-dir $d
.\target\release\dark-clipboard.exe --data-dir $d
```

Press `Ctrl+Shift+V`. Type to search (`viet` finds `Việt`), arrows to move, Enter copies the selected clip to the clipboard and returns the focus to the app you were in, Ctrl+Enter also pastes it there, Shift+Enter copies as plain text, Alt+1…9 copies the n-th result, Ctrl+P pins, Delete removes (Ctrl+Z or the Undo button within 5 s), Esc closes. Quit from the tray icon. Remove the folder afterwards: `Remove-Item -Recurse $d`.

Note: on a fresh `--data-dir` folder the first-run welcome opens and capture stays **off** until you press Start (Not now keeps it off; the welcome is reachable again from Settings > About). Pressing Start with that folder captures your real clipboard into it. Leave "Start with Windows" unchecked in demos: it changes your login items.

## Option B: real use

`.\target\release\dark-clipboard.exe` (data in `%LOCALAPPDATA%\DarkClipboard`, settings in `%APPDATA%\DarkClipboard`). On first run the welcome appears; press Start so that capture is on. Copy a few things, then press `Ctrl+Shift+V`.

## What to check (rows in `platform-checklist.md`, section "Windows: popup (Task 204)")

1. Vietnamese IME (Telex, VNI, Unikey) in the search box.
2. Popup position on the monitor under the cursor; mixed-DPI monitors.
3. Shortcut pressed twice quickly, and while the popup is open.
4. 10,000-clip scroll and search speed.
5. Real-shortcut timing: 50 presses, then `powershell -File scripts/measure-popup.ps1 -LogPath <data dir>\logs\dark-clipboard.log -Source hotkey`.
6. Tray menu: pause 5 min, until resumed, private mode, quit.

7. Paste into the previous app (Task 205): rows in section "Windows: copy back and paste (Task 205)".

Not yet available: image thumbnails and detail pane (206), settings window (207), history window (211). The welcome and start at login (208) are described in `platform-checklist.md`, section "Windows: welcome and start at login (Task 208)".
