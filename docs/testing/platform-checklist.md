# Platform verification checklist

Status legend: **T** tested and passed (date, app version), **F** tested and failed, **n/a** not available on the test machine, **untested**.
Rules: no row is marked T without a run recorded here. Clip content is never printed or stored in this file; only format names, kinds, sources and counts.

## Windows (Task 107, 2026-11-22)

Machine: Windows 11 Home 10.0.26300 x64 (the owner's machine). Build: `dark-clipboard` 0.0.1, release, commit `694600f` + the Task 107 test fix.
> Since Task 208 (D-054) a fresh data folder captures nothing: `--headless` logs `capture_off` and prints that capture is off until Start is pressed in the app. For any run that needs capture, first run the UI once with the same `--data-dir` (`dark-clipboard.exe --data-dir <scratch>`), press Start in the welcome, quit from the tray, then start `--headless` on that folder. There is no flag that skips the welcome.

Method: the owner copied in each app while `dark-clipboard --headless --data-dir <scratch>` ran. A read-only watcher logged, for every clipboard sequence change, the **format names** and the owner process (no content). The scratch database was summarised with a query that selects kind, source, sensitivity, counts and MIME/format names only (no preview, no bytes). The scratch data was deleted afterwards. The verification was run interactively by the orchestrator, because it needed back-and-forth with the owner (D-041).

### Automated real-clipboard tests

Command: `cargo test -p darkclip-windows -- --ignored --test-threads=1` (with the owner's consent; it overwrites the clipboard).
First run: 9 passed, 1 failed. `real_clipboard_write_text_roundtrip_vietnamese` expected `"Tiế"` from a 6-byte cap; `"Tiế"` is 5 bytes and the next char `n` fits, so the code's `"Tiến"` was correct and the **test expectation** was wrong. Fixed the expectation; re-run of that test: passed. Result: **10/10 pass** (the fixed test now hits a 6-byte cap exactly on a char boundary; mid-character back-off is covered by the pure test `utf16_truncates_on_char_boundary`), covering same/other-process signals, owner = `powershell.exe`, the Darkshot format set detected as `DarkshotExcluded`, PowerShell OLE phases, Vietnamese text round trip and capped read, non-ASCII HTML, a Windows-synthesised `CF_DIBV5`, copy-back sequence suppression (signal seq = returned post-close seq, no further signal within 300 ms), and an image copy-back readable by `read` with no marker formats written (D-012, D-038).

### Manual rows

| Row | Formats observed (names only) | Result in Dark Clipboard | Status |
|---|---|---|---|
| KeePassXC 2.8.0: two copies (password Ctrl+C, username Ctrl+B) | Seq 3991 listed `ExcludeClipboardContentFromMonitorProcessing`, `CanIncludeInClipboardHistory`, `CanUploadToCloudClipboard` (owner `KeePassXC`); seq 3981 listed only `DataObject` when the change was signalled; seq 3992 = auto-clear (no formats) | Neither copy stored. The skip reason was not recorded, and `keepassxc.exe` is also on the built-in excluded list, so this row does not show which path skipped | **T** (not stored) 2026-11-22; marker path for the password copy unproven |
| Darkshot 0.6.0 (local release build of the Darkshot repo): Copy image | `CF_DIB`, `ExcludeClipboardContentFromMonitorProcessing`, `CanIncludeInClipboardHistory`, `CanUploadToCloudClipboard`, `CF_BITMAP`, `CF_DIBV5`; owner none | Not stored. Owner was none, so the excluded-app list could not match: this proves the marker path end to end | **T** 2026-11-22 |
| Darkshot 0.6.0: Copy link (after R2 upload) | `CF_UNICODETEXT`, `ExcludeClipboardContentFromMonitorProcessing`, `CanIncludeInClipboardHistory`, `CanUploadToCloudClipboard`, …; owner none | Not stored | **T** 2026-11-22 |
| Notepad 11.2609 (text), copied twice | Copy 1 (seq 4007): one change, owner `Notepad` (with `EnterpriseDataProtectionId`). Copy 2 (seqs 4008–4010 within ~160 ms): three changes, the last with owner none after the OLE flush | One `text` clip with `copies=3`; stored **source = None**. A bump overwrites the source, so None reflects the last copy only; copy 1 may have been stored with `notepad.exe` first | **T** (capture) 2026-11-22; stored source **F** for the ownerless-flush case (F-14) |
| Chrome 158: page selection with Vietnamese text | `HTML Format`, `CF_UNICODETEXT`, Chromium internal formats | Stored as `rich_text` (HTML + plain), source `chrome.exe` | **T** 2026-11-22 |
| Explorer 10.0.26100.10068: copy 2 files | `CF_HDROP`, `FileNameW`, Shell formats, … | Stored as `files` (`text/uri-list` from `CF_HDROP`), source `explorer.exe` | **T** 2026-11-22 |
| Snipping Tool 11.2609 | `PNG`, `CF_DIB`, `CF_DIBV5`, `CanUploadToCloudClipboard`, `CanIncludeInClipboardHistory` (values allow), … | Stored as `image` (registered `PNG`), source `SnippingTool.exe`; the allowing DWORD values were read correctly and did not cause a skip | **T** 2026-11-22 |
| Word / Excel | — | — | **n/a** (not installed) |
| Bitwarden desktop, 1Password | — | — | **n/a** (not installed) |
| RDP clipboard | — | — | **n/a** |
| Elevated source app (source should resolve or be None) | — | — | **untested** (optional row skipped) |
| Copy-back from the UI, then immediate copy in another app (no stall, no recapture) | — | Covered by the automated copy-back test; manual check needs the Phase 3 UI | **untested** (manual part) |
| Win+V history after a Dark Clipboard copy-back (D-012) | — | Automated test confirms no marker formats are written; manual Win+V check needs the Phase 3 UI | **untested** (manual part) |
| Real Ctrl+C in a console | — | Shutdown measured via `--exit-after-ms` (Task 106) | **untested** |

### Findings

- Unidentified: the log starts with an ownerless plain-text change (seq 3974, no markers) before the session rows; it predates the first row and is not attributed.

- **F-14 (source lost on OLE writers; also a privacy gap):** writers that use OLE with a flush (Notepad sometimes, PowerShell `Set-Clipboard`) can bump the sequence number at each phase, and the last phase has no owner. Each new seq starts a fresh pending change (D-027), so the stored clip has `source = None`. No privacy leak for apps that set markers (KeePassXC, Darkshot), but an excluded app **without** markers that writes this way would not be matched by the excluded-app list. Fix: `docs/tasks/109-ole-source-carry.md` (exclusion check over every superseded source in the burst, always; promotion of the stored source only in the narrow OLE-flush case).
- KeePassXC 2.8.0 on Windows set all three Windows markers on at least one copy and is also on the built-in excluded list: risk R-02 is **mitigated** for KeePassXC; re-verify with skip-reason output (Phase 3 diagnostics). Still open for Bitwarden and 1Password (not tested).
- Darkshot's Copy link is marked as well, so Dark Clipboard never stores Darkshot links.

## Windows: tray and console (Task 202)

All rows **untested** until the owner runs them (Windows 11, `cargo build`, run `target\debug\dark-clipboard.exe`).

| Row | Expected | Result |
|---|---|---|
| Start the app from Explorer (double-click) | No console window; a tray icon appears | **untested** |
| Start from a terminal | No new console window; the terminal prompt returns at once; `--help` and errors still print in that terminal | **untested** |
| `--headless` in a terminal, then Ctrl+C | "running" line shows; Ctrl+C stops it cleanly | **untested** |
| Tray icon at 100% and 150% display scale, dark and light taskbar | Glyph readable (clipboard outline) | **untested** |
| Tooltip on hover | "Dark Clipboard: capturing" (or "capture is off" before the first-run Start) | **untested** |
| Menu: Pause > 5 minutes | Icon shows pause bars; tooltip "paused until HH:MM" (local time, +5 min); Resume enabled | **untested** |
| Wait for a 5 minute pause to end | Icon and tooltip return to normal by themselves | **untested** |
| Menu: Pause > Until resumed, then Resume | Tooltip "paused"; Resume returns to normal | **untested** |
| Menu: Private mode (check item) | Checked, icon changes, tooltip "private"; click again unchecks | **untested** |
| Left-click the tray icon | Window toggles; right-click opens the menu | **untested** |
| Menu: Settings... | Window shows (settings content arrives in Task 207) | **untested** |
| Menu: Quit | Icon disappears at once; process ends within 2 s | **untested** |
| Vietnamese Windows UI language (first run) | Menu and tooltip in Vietnamese | **untested** |
| `dark-clipboard --toggle`, `--quit`, `--help` from interactive cmd or PowerShell | Known limit (GUI subsystem): the shell does not wait, so output may appear after the next prompt and `%ERRORLEVEL%` / `$LASTEXITCODE` are not the real exit code. Use `start /wait` or `Start-Process -Wait -PassThru` to get it | **untested** |
| `--headless` started from Explorer or a shortcut | Known limit: no console and no tray; it stops only with `--quit` (from another start) or Task Manager | **untested** |
| Explorer restarts (taskbar crash) | Known limit: the icon may vanish until the app restarts (not handled in this task) | **untested** |

## Windows: popup (Task 204)

All rows **untested** until the owner runs them. Use a scratch history: `dark-clipboard --seed-demo-clips 10000 --data-dir <empty folder>`, then `dark-clipboard --data-dir <same folder>` (synthetic text only). Delete the folder afterwards.

| Row | Expected | Result |
|---|---|---|
| Press `Ctrl+Shift+V` | Popup near the cursor, search box focused and empty, recent clips listed, pinned first | **untested** |
| Shortcut pressed twice quickly (Review Focus 1) | Opens then closes (or stays open if the second press comes within the 150 ms shortcut debounce); never two popups, never stuck | **untested** |
| Shortcut while the popup is open | Closes it | **untested** |
| Click outside the popup | Popup closes | **untested** |
| Mixed-DPI monitors (Review Focus 2): e.g. a 100 % and a 150 % monitor; open the popup on each | Opens on the monitor under the cursor, inside its work area (not under the taskbar), text crisp (not blurry), size about 420 × 560 at 100 % | **untested** |
| Vietnamese IME (Review Focus 4): Windows Vietnamese Telex and VNI keyboards, and Unikey (Telex), type `tieesng vieejt` / `tie61ng vie65t` | Search box shows `tiếng việt`; results update; the composition is not cut by the 60 ms debounce | **untested** |
| 10,000 clips (Review Focus 5): hold Down arrow, Page Down, End; drag the scrollbar; mouse wheel | List scrolls without visible jank; more clips load as you reach the end; End goes to the last loaded clip | **untested** |
| Type a search with 10,000 clips | Results follow the typing without a visible stall; matched words highlighted, accents ignored (`viet` highlights `Việt`) | **untested** |
| Enter on a clip, then paste in Notepad | The clip is pasted; the popup closed after Enter | **untested** |
| Shift+Enter on a rich-text clip, then paste in Word | Plain text only | **untested** |
| Delete, then Undo (button or Ctrl+Z) within 5 s | Clip disappears, then comes back; without Undo it is gone after 5 s or when the popup closes | **untested** |
| Ctrl+P on a clip | Pinned clips move to the top with an accent bar; again unpins | **untested** |
| Alt+1 .. Alt+9 | Copies the n-th listed clip and closes | **untested** |
| Tab / Shift+Tab | Focus moves search → chips → list and back; Enter or Space on a chip toggles it | **untested** |
| Tab quickly through several chips, then press Enter on one | Enter toggles that chip; it never copies a clip or closes the popup | **untested** |
| Tab to a chip, press Enter, then Enter again | The chip turns on, then off; it keeps the focus ring both times | **untested** |
| Narrator on the popup | Search box, chips (checkbox, checked state) and rows (list items) are announced | **untested** |
| Real-shortcut NFR-01 timing: 50 presses, then `scripts/measure-popup.ps1 -LogPath <log> -Source hotkey` | p95 ≤ 100 ms (record the numbers in `docs/testing/perf-windows.md`) | **untested** |

## Windows: copy back and paste (Task 205)

All rows **untested** until the owner runs them (agents never send input, GC-19). Use a scratch history as above and the shortcut `Ctrl+Shift+V` unless a row says otherwise. Before each row put the caret in an empty document of the target app. The log `<data dir>\logs\dark-clipboard.log` gets one `paste` line per Ctrl+Enter (`result=ok|refused|elevated|failed`).

| Row | Expected | Result |
|---|---|---|
| Notepad: Enter on a clip | Popup closes, Notepad has the focus again (caret blinking), nothing typed; Ctrl+V pastes the clip | **untested** |
| Notepad: Esc in the popup | Popup closes, Notepad has the focus again, nothing typed | **untested** |
| Notepad: Ctrl+Enter on a clip | Popup closes, the clip is pasted once at the caret; `paste result=ok` | **untested** |
| Notepad: Ctrl+Enter, keep Ctrl held a moment, release | Pasted once; no stray characters; Ctrl not stuck afterwards (type a letter: no shortcut fires) | **untested** |
| Notepad: Ctrl+Shift+Enter, keep Shift held over 1 s | Not pasted; popup stays with "Copied — paste manually"; `result=refused`; Ctrl+V pastes the clip | **untested** |
| VS Code (editor focused): Ctrl+Enter | Pasted once into the editor | **untested** |
| Edge (address bar, then a page text field): Ctrl+Enter | Pasted once into the field that had the focus | **untested** |
| Word (if available): Ctrl+Enter on a rich-text clip; Shift+Enter then Ctrl+V | Formatted text pasted; Shift+Enter gives plain text after manual Ctrl+V | **untested** |
| Elevated Notepad (Run as administrator), app not elevated: Ctrl+Enter | Nothing typed into Notepad; popup stays with the "runs as administrator" toast; `result=elevated`; Ctrl+V in Notepad pastes the clip | **untested** |
| Elevated Notepad: Enter | Popup closes, focus returns to the elevated Notepad (no input sent) | **untested** |
| Popup opened from the tray icon: Ctrl+Enter | Never pastes into the taskbar; popup stays with "Copied — paste manually"; `result=refused` | **untested** |
| Popup opened from the tray icon: Enter | Popup closes; nothing is typed anywhere | **untested** |
| Close the target app while the popup is open, then Ctrl+Enter | "Copied — paste manually"; nothing typed anywhere | **untested** |
| Target on another virtual desktop (Win+Ctrl+Left/Right after opening the popup), Ctrl+Enter | No paste into a hidden window; toast or popup closes with the clip copied | **untested** |
| Set `"enter_pastes": true` under `ui` in `<data dir>\settings.json`, restart, Enter in Notepad | Hint line reads "Enter paste ..."; Enter pastes like Ctrl+Enter; the Copy button and Alt+1..9 still only copy | **untested** |
| Press Ctrl+Enter twice quickly while Shift is held (the first paste is still waiting) | The second press shows "The clipboard is busy"; at most one paste happens | **untested** |
| Known limit: a paste refused after the target already took the focus (it did not stay in front, or a key went down during the switch) | The popup has closed, so its toast is not seen; the clip is still on the clipboard (Ctrl+V works); the log shows `paste result=refused`. Note how often this happens | **untested** |
| Hint line (default settings) | Shows "Ctrl+Enter paste" | **untested** |
| Owner-only test: open an empty Notepad, run `cargo test -p darkclip-windows --lib real_paste_into_focused_window -- --ignored --nocapture`, click into Notepad within 3 s | The current clipboard is pasted once into Notepad; the test passes | **untested** |

## Windows: history window (Task 211)

Setup: `--seed-demo-clips 10000 --data-dir <temp>` then run with the same `--data-dir` (see `demo-popup.md`). Open the window from the tray ("History...") or with Ctrl+H in the popup. Visual and input checks only the owner can do (GC-19).

| Check | Expected | Result |
|---|---|---|
| Tray menu entry "History..." (and Vietnamese: "Lich su...") | Opens the window; opening it again brings it to the front, no second window | **untested** |
| Ctrl+H in the popup (search box empty, then with text) | The popup closes and the history window opens. Report if Ctrl+H does nothing: Slint may deliver it as Backspace's control code, and then it only works with an empty search box | **untested** |
| Look | Dark title bar and dark body, no white flash; left filters, centre list, right details; readable at 100/150/200 % scale | **untested** |
| Resize, then close and reopen | Same size and position. With two monitors: move it to the other monitor, resize, close; open it with the cursor on each monitor: each shows its own last rectangle | **untested** |
| Old settings file (no `history_windows`) | The app starts, the window opens centred at 980 x 640 | **untested** |
| Filters: All, Pinned, each kind, Today, 7 days, 30 days, a source app | The list narrows; All clears every filter but keeps the search text; the filter keeps the keyboard focus when toggled with Space/Enter | **untested** |
| Click a row | The details show on the right (text, image, files); a masked row shows only the "hidden" note | **untested** |
| Ctrl+click, Shift+click, Shift+Up/Down, Space, Ctrl+A | Rows are tinted; the footer says "N selected"; the right pane says the same and shows no detail; Esc clears the marks | **untested** |
| Enter, Shift+Enter, the Copy buttons | The clip is on the clipboard (check by pasting in Notepad); the footer says "Copied"; with several rows selected the copy buttons are hidden | **untested** |
| Ctrl+P, Pin/Unpin with several rows | All unpinned rows get pinned; when all are pinned they are unpinned | **untested** |
| Delete key or button on one row, then on several | A dialog asks first ("Delete N clips?"); Esc or Cancel deletes nothing; Enter or Delete confirms; the rows go and the footer says how many; other keys do nothing while the dialog is open | **untested** |
| Right click on an unselected row, on a selected row of a multi-selection, and the Menu key | Menu with Copy, Copy as plain text, Pin/Unpin, Delete (opens the dialog), Select all; acts on the row, or on all marked rows | **untested** |
| Context menu with the keyboard | Known limit: the Menu key opens the menu, but its entries are not keyboard-focusable (mouse only; Esc or a click outside closes it). Every action has a key (Enter, Shift+Enter, Ctrl+P, Delete, Ctrl+A); note whether the menu is still reachable enough | **untested** |
| Ctrl+Up/Down/Home/End, Space, Shift+Home/End | Ctrl+arrows move the cursor and keep the marks; Space marks the row under the cursor (rows 2 and 4 stay marked after Ctrl+Down twice); Shift+Home/End extend the range | **untested** |
| Scroll to the end of 10,000 clips | More pages load; scrolling stays smooth | **untested** |
| Close the window with the X, Alt+F4 and Esc | Closes each time; reopening shows an empty search, no selection and no detail from before | **untested** |
| Vietnamese IME (Telex) in the search box | Diacritics type and search as in the popup | **untested** |
| Screen reader (Narrator): list, filters, dialog | Rows are read with their text and details; the dialog text is read | **untested** |

## Windows: welcome and start at login (Task 208)

Use a throwaway data folder for the welcome rows: `dark-clipboard.exe --data-dir <temp folder>` (settings live in that folder too). Never use `--data-dir` for the start-at-login rows: they change the real login items of this account.

| Row | Expected | Result |
|---|---|---|
| Fresh data folder, start the app | The welcome window opens by itself (dark, no white flash): shortcut, never-captured list, data folder, "no account, no cloud", "Nothing is captured until you press Start", an unchecked "Start with Windows" box, Not now and Start. Tray tooltip says capture is off | **untested** |
| Copy some text before pressing anything | Nothing is added to the history | **untested** |
| Press Start (box unchecked) | The window closes; the tray tooltip says capturing; a copy now appears in the popup; restart the app: no welcome and capture still on | **untested** |
| Fresh folder, press Not now | The window closes; capture stays off (tray: capture is off; copies are not stored); restart: no welcome | **untested** |
| Fresh folder, close the welcome with X or Esc | Same as Not now | **untested** |
| After Not now: Settings > About > Show welcome > Start | Capture turns on; the welcome opens again only from that button | **untested** |
| Existing settings file (copy of a real one, no `onboarding_done`) | No welcome; capture state exactly as before | **untested** |
| Language: Vietnamese OS language on a fresh folder | The welcome is in Vietnamese; the shortcut and folder path are filled in | **untested** |
| Start with the box checked, then look at `reg query HKCU\Software\Microsoft\Windows\CurrentVersion\Run /v DarkClipboard` | One value: the quoted path of dark-clipboard.exe, no arguments. Log out and in: the app starts (this row changes your login items; remove the entry afterwards with Settings or `reg delete`) | **untested** |
| Settings > General > Start at login: switch on, Save, then off, Save | The Run value appears and disappears; the status line says Saved; no "not wired" note is shown | **untested** |
| Run value made unwritable (for example a policy), then toggle | The status line says the change failed, and the toggle returns to the real state | **untested** |
| Tray menu after Not now | The icon is the paused glyph (not the crossed one used for failures); "Start capturing..." is the first menu entry and opens the welcome; it disappears once capture is on | **untested** |
| Settings > Privacy after Not now | A yellow note says capture is off, with a "Start capturing..." button that opens the welcome; gone after Start | **untested** |
| Start at login drift: switch the entry off in Task Manager > Startup, reopen Settings | The toggle shows off, with a note that the real state differs from the saved setting; Save with the toggle on re-enables it | **untested** |
| App started with `--data-dir` | The welcome check box and the Settings toggle are disabled with a note that a custom data folder cannot change the login entry | **untested** |
| `--headless` on a fresh `--data-dir` | The console says capture is off until Start is pressed in the app; the log has `capture_off`; copies are not stored | **untested** |
| Welcome opened again from About while capture is on | No "Nothing is captured until you press Start" line; Esc or X changes nothing | **untested** |
| Screen reader (Narrator) on the welcome | Texts, the check box and both buttons are read | **untested** |
| Task Manager → Startup → disable Dark Clipboard, then open Settings | The login toggle shows Off with the "differs from the saved setting" note (StartupApproved odd-byte rule is an observed convention, not a documented contract) | **untested** |

The registry round trip also exists as an ignored test: `cargo test -p darkclip-windows -- --ignored autostart` (uses the value name `DarkClipboardTest`, removed at the end).

## Windows: Phase 3 review fixes (Task 213)

All rows **untested** until the owner runs them (agents never click, type or take screenshots, GC-19). Use a scratch history (`--seed-demo-clips 200 --data-dir <temp>`, then run with the same `--data-dir`) unless a row says otherwise.

| Row | Expected | Result |
|---|---|---|
| Open the popup, note a clip, close it. Delete that clip in the history window (Delete, confirm), then open the popup | The deleted clip never appears, not even for one frame; the list fills from empty | **untested** |
| Open the popup, close it. Settings > History > Clear history (keep pinned off), then open the popup | No clip from before is drawn, not even for one frame; the empty-history state shows | **untested** |
| Open the popup, close it, delete a clip in History, open the popup and press Enter (or Alt+1) at once | Only a clip of the new list is copied (paste in Notepad to check); never the deleted one | **untested** |
| Set "Enter also pastes" on (Settings > General, Save). Notepad focused, open the popup, double-click a row | The clip is copied and Notepad gets the focus back, but nothing is typed; Ctrl+V pastes it. The log has no `paste` line for it | **untested** |
| Same with "Enter also pastes" off | Same: copied, focus returned, nothing typed | **untested** |
| Default profile (no `--data-dir`), start at login off: open Settings and leave it open; open the welcome (Settings > About > Show welcome), check "Start with Windows", press Start; back in Settings change only the language, Save | `reg query HKCU\Software\Microsoft\Windows\CurrentVersion\Run /v DarkClipboard` still shows the value; reopen Settings: the toggle is on. (This row changes your login items; remove the entry afterwards) | **untested** |
| Notepad focused, press the shortcut to open the popup, then press the shortcut again to close it | Notepad has the keyboard focus again (type a letter: it appears in Notepad) | **untested** |
| Start without `--data-dir`, change a setting and Save | The change is in `%APPDATA%\DarkClipboard\settings.json`; nothing named `settings.json` appears in `%LOCALAPPDATA%\DarkClipboard` | **untested** |
| Start with `--data-dir <temp>`, change a setting and Save | The change is in `<temp>\settings.json`; `%APPDATA%\DarkClipboard\settings.json` is unchanged | **untested** |
| Start with `--data-dir "%LOCALAPPDATA%\DarkClipboard"` (the default folder, also written in other case) | Treated as the normal profile: settings from `%APPDATA%\DarkClipboard\settings.json`, `--toggle` without `--data-dir` reaches it, the start-at-login toggle is enabled | **untested** |
| Hand-edit `settings.json` (app quit) to give an excluded app a name containing U+202E (for example `"name": "Pay‮roll"`), start, Settings > Privacy | The name is shown with a replacement mark (U+FFFD) instead of reversing the text | **untested** |
| Settings > Privacy: Remove on the built-in Darkshot entry, then on KeePassXC (Cancel both) | Darkshot's question says Darkshot marks its own copies so they stay excluded; KeePassXC's says its copies will be stored again. Same in Vietnamese | **untested** |
| Default profile, start at login on, entry disabled in Task Manager > Startup: open Settings (toggle off, drift note), switch the toggle on, press Revert, wait a moment, Save | After Revert the toggle shows off again with the drift note; after Save the entry is still disabled in Task Manager | **untested** |
| Same disabled entry: Settings > About > Show welcome | The "Start with Windows" box shows off; Start without touching it leaves the entry disabled in Task Manager; ticking it and pressing Start enables it (remove the entry afterwards) | **untested** |
| Open the popup, history, settings and welcome windows, then tray > Quit | Every window disappears at once, before the icon goes; the process ends within 2 s | **untested** |

## Windows: text transforms and edited copies (Task 404)

All rows **untested** until the owner runs them (agents never click, type, take screenshots or write the real clipboard, GC-19). Use a scratch history (`--seed-demo-clips 50 --data-dir <temp>`, or copy a few texts into a `--data-dir <temp>` instance). Paste into Notepad to check what is on the clipboard.

| Row | Expected | Result |
|---|---|---|
| Popup: select a text clip, press Ctrl+T | A panel lists the 13 transforms with a preview of the first one; the list behind it does not react to keys | **untested** |
| In the panel press Down/Up, then click a transform | The highlight moves, the preview updates; Up at the top and Down at the bottom stop | **untested** |
| Choose "UPPER CASE", press Enter | The popup closes, focus returns to the previous window; Ctrl+V pastes the upper-case text. History has no new clip (open the history window and count) | **untested** |
| Choose "JSON: pretty print" on a clip that is not JSON | The preview is empty and a red note says the text is not valid JSON; Copy is not shown and Enter does nothing | **untested** |
| Copy `{"a":[1,2,{"b":null}]}`, then Copy as > JSON pretty > Enter, paste | Two-space indented JSON; Copy as > JSON minify gives the compact form again | **untested** |
| Copy `<p>Hi <b>there</b></p><script>alert(1)</script><ul><li>a</li><li>b</li></ul>`, Copy as > HTML to text | `Hi there`, `a`, `b` on separate lines; no script text anywhere | **untested** |
| Copy as > Base64 encode on a clip with `Xin chào 😀`, paste, copy the pasted text and Copy as > Base64 decode; also Base64 decode on a text that is not Base64 | The round trip gives back `Xin chào 😀`; decoding non-Base64 shows a red "not valid Base64" note | **untested** |
| Copy a text longer than 32 KiB, Copy as > Upper case | The preview ends with a note that only the first 32 KiB is shown; pasting gives the whole text in upper case | **untested** |
| Copy a text over 1 MiB (the default text limit is 1 MiB, so raise it in Settings first), Copy as | A red note says it is too large to transform | **untested** |
| Select an image clip, a file clip and a masked clip (copy a fake API key with the sensitive setting "Store masked") | No "Copy as..." button, Ctrl+T does nothing, in popup and history; the same for Edit in history | **untested** |
| Esc in the panel | The panel closes, the popup stays; the preview text is gone when it is reopened | **untested** |
| Hide the popup with the panel open (click elsewhere), show it again | The panel is closed and no output text is visible | **untested** |
| History window: select one text clip, press Ctrl+E | An editor opens with the clip's text and a note that the original stays unchanged; Esc closes it without saving | **untested** |
| Edit the text, press "Save as new clip" (or Ctrl+Enter) | The editor closes, a footer message says it was saved, a new clip appears at the top; the original clip is unchanged | **untested** |
| Edit the text to contain a fake API key (type `sk-` followed by 40 random letters and digits), save, with Settings > Privacy "sensitive handling" at the default | The editor stays open with a red note that it was not saved because it looks like a secret; no new clip appears | **untested** |
| Same with sensitive handling "Store masked" | The clip is saved and shows as masked in the list | **untested** |
| Select a clip with more than 256 KiB of text, press Ctrl+E | A footer message says it is too large to edit here; no editor opens | **untested** |
| Edit, then clear the text and save | The editor stays open with "There is nothing to save" | **untested** |
| Switch the language to Vietnamese (Settings), open Copy as and Edit | Labels, hints and notes are in Vietnamese | **untested** |
| Screen reader (Narrator) on the panel and the editor | The panel, transform names, result and buttons are read | **untested** |
| History: edit a clip, change nothing, Save as new clip | The editor closes, "Already in history" shows and the original clip is selected; its time and counts did not change | **untested** |
| Tray > Private mode on, then edit and save in the history window | The editor stays open with "Not saved: private mode is on."; with Pause instead, the save works | **untested** |

## Windows: data management (Task 411)

All rows **untested** until the owner runs them (agents never click, type, take screenshots or write the real clipboard, GC-19). Use a scratch history (`--seed-demo-clips 200 --data-dir <empty folder>`, then `--data-dir <same folder>`), never the real one.

| Row | Expected | Result |
|---|---|---|
| Settings > History | The storage lines show clips, pinned, favorites, database size, file size and clips per kind (in the language chosen) | **untested** |
| Pin one clip and favorite another (history window), Clear history... with the box ticked | The dialog names the number of clips that will go (all except the pinned and favorite ones); after Delete the two stay | **untested** |
| Clear history..., untick "Keep pinned and favorite clips" | The number in the question changes to the total; after Delete the history is empty | **untested** |
| Type 30 in "older than", Delete old clips... | The dialog names a count; clips copied less than 30 days ago, pinned clips and favorites stay (seeded demo clips are all recent: the count is 0) | **untested** |
| Type 0, abc or an empty value, Delete old clips... | A red note asks for 1 to 3650 days; no dialog | **untested** |
| Remove orphaned files... on a clean folder | The dialog says 0 files; Delete reports 0 files removed | **untested** |
| With the app closed put an unreferenced 64-letter hex file in `blobs\ab\`, start, Remove orphaned files... | The dialog counts it (the startup sweep may already have removed it: then 0); the status line reports files removed and the space freed | **untested** |
| Compact database... after deleting many clips | The dialog names the database size; afterwards the status shows the size before and after; search still finds the remaining clips | **untested** |
| Fill the disk volume of the data folder so that free space is below twice the database size, Compact database... | The status says there is not enough free disk space; nothing changes | **untested** |
| Delete a clip whose text had a unique word, then search for the word and look in `history.db` with a hex viewer (app closed) | Not found in the list; the word is not in `history.db` or `history.db-wal` | **untested** |
| Vietnamese interface, open the History section and each dialog | All labels, questions and results are Vietnamese | **untested** |
| Esc with a cleanup dialog open | The dialog closes without deleting | **untested** |

## Windows: backup, restore, export and import (Task 405)

All rows **untested** until the owner runs them (agents never click, type, take screenshots or write the real clipboard, GC-19). Use scratch histories only (`--seed-demo-clips 200 --data-dir <empty folder A>`, then `--data-dir <A>`; a second empty folder B for restores and imports), never the real one. Keep the backup and export files in a scratch folder too.

| Row | Expected | Result |
|---|---|---|
| Settings > History, Backup and transfer | Heading, note and three buttons: Back up now..., Restore from backup..., Import clips... | **untested** |
| Back up now... | The Windows save dialog opens with `dark-clipboard-<date>.dcbackup` and the file type "Dark Clipboard backup"; the settings window stays usable; Cancel does nothing | **untested** |
| Save the backup | The buttons grey out, the status line says "Working...", then "Backup saved: N clips"; the file exists; no `.partial` file is left next to it | **untested** |
| Open the backup with 7-Zip or Explorer | It contains `manifest.json`, `history.db` and `blobs` files only | **untested** |
| A masked clip in A (sensitive setting "store masked", copy a synthetic token from a scratch file), then Back up now... | The count leaves it out; after restoring into B it is not there | **untested** |
| Start the app on B, Restore from backup..., choose A's backup | A question says the whole history will be replaced and the current one kept as a safety copy; Cancel and Esc change nothing | **untested** |
| Same, press Restore | The status says "History restored: N clips"; the history window and popup show A's clips, collections, tags, notes, favorites and images; `history.db.pre-restore-<number>` exists in B | **untested** |
| Quit and start the app on B again | `history.db.pre-restore-*` is gone; the restored history is still there | **untested** |
| Restore a file that is not a backup (rename any .zip or .txt to `.dcbackup`) | Red status: not a valid Dark Clipboard file, nothing was changed; the history is unchanged | **untested** |
| Restore a backup whose `manifest.json` was edited to `"format": 2` (7-Zip, scratch copy) | Red status: made by a newer version, nothing was changed | **untested** |
| Restore a backup with one blob file edited (scratch copy) | Red status: not valid or damaged; history unchanged | **untested** |
| History window on A: select a collection in the left pane, `Ctrl+Shift+E` | Save dialog with `dark-clipboard-export-<date>.dcexport`; saving shows "Exported N clips" | **untested** |
| Mark 3 clips (one of them masked), `Ctrl+Shift+E` | "Exported 2 clips. Masked or sensitive clips left out: 1" | **untested** |
| On B: Import clips..., choose the export | "Imported N new clips; M were already in your history"; a collection with the export's name holds them | **untested** |
| Import the same file again | Nothing new: all N are counted as already in history; no duplicate rows | **untested** |
| Private mode on, Import clips... | Red status: private mode is on, nothing is imported | **untested** |
| Vietnamese interface: the buttons, dialog titles, question and every result | All Vietnamese | **untested** |
| Open Back up now... and, while the dialog is open, press Restore or Import | The buttons are greyed out; only one file dialog at a time | **untested** |
| On a large scratch history (`--seed-demo-clips 5000`), start Back up now... and at once choose Private mode in the tray, then copy a line from a scratch file | The tray switches to private at once; after "Backup saved" the copied line is not in the history | **untested** |
| Restore with the data folder on a nearly full volume (less free space than twice the backup) | Red status: not enough free disk space, nothing was changed | **untested** |
| With a scratch tool, put a synthetic text on the clipboard with only the `CanUploadToCloudClipboard` = 0 format (no other privacy marker: the clip is captured), then mark it and press `Ctrl+Shift+E` | The message counts it as left out; the export does not contain it | **untested** |
| Kill the app (Task Manager) while a large backup is being written | A `<file>.dark-clipboard-partial` may remain next to the chosen name; no file with the chosen name appears | **untested** |

## Windows: collections, tags, notes and favorites (Task 403)

All rows **untested** until the owner runs them (agents never click, type, take screenshots or write the real clipboard, GC-19). Use a scratch history (`--seed-demo-clips 200 --data-dir <empty folder>`, then `--data-dir <same folder>`), never the real one. Keyboard rows: use no mouse.

| Row | Expected | Result |
|---|---|---|
| History window, select a clip, `Ctrl+D` | A star appears on the row and in the detail strip; the strip button now says "Remove from Favorites"; the left pane shows "Favorites (1)" | **untested** |
| `Ctrl+D` again | The star goes; Favorites (0) | **untested** |
| `Ctrl+Shift+N`, type `Work`, Enter | Toast "Collection created"; "Collections" heading with "Work (0)" in the left pane | **untested** |
| `Ctrl+Shift+N`, press Enter with an empty box; type `work` (existing name) | The Save button is hidden; the box shows "A collection with this name already exists." for a duplicate; a name of 81 characters shows the too-long message | **untested** |
| Select a clip, `Ctrl+L`, Down/Up, Enter | The clip joins the chosen collection; the left pane count goes up; selecting the collection in the left pane lists the clip | **untested** |
| With no collection yet, `Ctrl+L` | The dialog says there are no collections and offers Cancel only; Esc closes | **untested** |
| Collection selected in the left pane, `Ctrl+Shift+L`, Enter | The clip leaves the collection; the clip stays in the history | **untested** |
| Collection selected in the left pane, `Ctrl+Shift+R`, change the name, Enter | Toast "Collection renamed"; the left pane shows the new name | **untested** |
| Collection selected in the left pane, `Ctrl+Shift+Del` | A dialog names the collection and says its N clips stay; Esc cancels and nothing changes | **untested** |
| Same, press Enter | The collection disappears from the pane, the list shows all clips again, the clips are still in the history; Favorites cannot be renamed or deleted (the keys do nothing on it) | **untested** |
| Select a clip, `Ctrl+N`, type a note, Ctrl+Enter | Toast "Saved"; the strip shows the note | **untested** |
| `Ctrl+N`, type more than 4,000 characters (paste a long text) | The counter turns red and a message appears; Save is hidden; Esc cancels and the note is unchanged | **untested** |
| `Ctrl+N`, edit, Esc | The old note stays | **untested** |
| `Ctrl+G`, type `Todo`, Enter | A chip `todo` appears (lowercase); the box clears; the left pane lists "todo (1)" under Tags | **untested** |
| `Ctrl+G`, type `two words` or `a/b` | The box shows "A tag may use letters, digits, - and _ only."; no chip is added | **untested** |
| `Ctrl+G` with the box empty, Backspace | The last chip is removed | **untested** |
| In the detail strip click the x of a tag | The tag is removed (mouse) | **untested** |
| Add 20 tags to one clip, then a 21st | The message says a clip can have 20 tags at most | **untested** |
| Click "todo" in the left pane | The list shows the clips with that tag; click again to clear; with a collection also selected both filters apply | **untested** |
| Mark 3 clips (Space), `Ctrl+D` | A confirmation says "Add 3 clips to Favorites?"; Esc cancels and nothing changes; Enter applies and the toast says 3 clips updated | **untested** |
| Mark 3 clips, `Ctrl+L`, choose a collection, Enter | The confirmation names the count and the collection; Enter adds them | **untested** |
| Mark 3 clips, `Ctrl+G`, type `bulk`, Enter | The confirmation says "Add the tag "bulk" to 3 clips?"; Enter tags them; a clip that already had the tag is not counted | **untested** |
| A masked clip (copy a synthetic token such as one made by the seed tool or a password-manager-style secret from a scratch file; shown "Masked") selected: `Ctrl+D`, `Ctrl+N`, `Ctrl+G`, `Ctrl+L` | None of them acts: a toast says masked clips cannot be favorited, tagged or added; the strip has no edit buttons | **untested** |
| Mark a masked clip with two normal clips, `Ctrl+L` | The confirmation counts 2 (not 3) and the result toast mentions that 1 masked clip was skipped | **untested** |
| Popup (`Ctrl+Shift+V`): a favorite row | A star shows on the row; a Favorites chip and one chip per collection follow the Pinned chip | **untested** |
| Popup: Tab to the Favorites chip, Enter | Only favorites are listed; Enter again clears it | **untested** |
| Popup: select a row, `Ctrl+D` | The star appears (or disappears) within a moment; the key hint line mentions Ctrl+D | **untested** |
| Popup: masked row, `Ctrl+D` | A toast says masked clips cannot be favorited; no star | **untested** |
| Close and reopen the history window and the popup | Collection and tag names, notes and dialogs from before are not visible until the lists reload | **untested** |
| Keyboard in each text dialog (new collection, note, tags, bulk tag, and the Ctrl+E editor of Task 404): type, then Enter / Esc; in the note and the editor Ctrl+Enter, and Backspace in the empty tag box | Enter submits (a newline in the note and the editor), Esc cancels, Ctrl+Enter saves the note or the editor, Backspace in the empty tag box removes the last chip; the keys act with the cursor in the text box, no mouse | **untested** |
| Close any dialog or the editor | The search box has the keyboard focus again (type to search at once, arrows move the list) | **untested** |
| Remove marked clips from a collection when some are not in it | The confirmation says "Remove the N marked clips ... Clips not in it are unchanged."; the toast says "Done" without a number | **untested** |
| Close the history window with a dialog open, reopen | No chip, name or note from the dialog is visible | **untested** |
| Vietnamese interface: repeat the dialogs | Every label, hint, error and confirmation is Vietnamese | **untested** |
| Screen reader (Narrator): the dialog, the pane strip, a favorite row | The dialog title is announced; the row description ends with "Favorite" ("Yêu thích") | **untested** |

## Windows: opt-in update check (Task 412)

All rows **untested** until the owner runs them (agents never launch the UI or make a network request, GC-19). Use a scratch folder: `dark-clipboard.exe --data-dir <empty folder>`. Rows that talk to GitHub need a published release newer than the running build; before one exists the expected result is "You are up to date" or "Could not check" (no release yet answers 404).

| Row | Expected | Result |
|---|---|---|
| Settings > About, fresh scratch folder | A "Updates" heading, a note about what is sent, a "Check for updates" button and an unchecked "Automatically check weekly" box; no result line | **untested** |
| Leave the app running 1 minute with the toggle off and no button press (watch the network, for example with the firewall prompt or a resource monitor for `dark-clipboard.exe`) | No connection is made | **untested** |
| Press "Check for updates" | The button greys out, "Checking..." shows, then one result line; the window stays usable meanwhile | **untested** |
| Press it with the network off (Wi-Fi off) | "Could not check" within about 10 seconds; no other text, no crash | **untested** |
| A newer release exists | "New version available: X.Y.Z" and an "Open the release page" button; the button opens that release page in the default browser and nothing is downloaded | **untested** |
| Same version or older | "You are up to date" and no open button | **untested** |
| Turn the weekly toggle on, close and reopen the window and the app | The box is still checked; `settings.json` has `updates.auto_check: true` and a `last_check_ms` after the first check | **untested** |
| Weekly on, restart the app within a week | No second request (the time in `last_check_ms` does not change) | **untested** |
| Turn the toggle off, restart | No request; the box stays unchecked | **untested** |
| Vietnamese interface | Every label, the note and every result are Vietnamese | **untested** |

## Windows: release blockers (Task 413)

All rows **untested** until the owner runs them (agents never launch the UI, change the real data folder's ACL or the real WER registry from a test, GC-19). Use a scratch folder for the database rows: `dark-clipboard.exe --data-dir <empty folder>`.

| Row | Expected | Result |
|---|---|---|
| Start the app once (any mode, for example `dark-clipboard.exe --help`), then `reg query "HKCU\Software\Microsoft\Windows\Windows Error Reporting\ExcludedApplications"` | A value named `dark-clipboard.exe` (the running file name) exists; the log of a normal start has `wer excluded=ok no_heap=ok` | **untested** |
| Start the app normally (default folder), then `icacls "%LOCALAPPDATA%\DarkClipboard"` and `icacls "%LOCALAPPDATA%\DarkClipboard\history.db"` | The folder lists exactly your account and `NT AUTHORITY\SYSTEM` with `(OI)(CI)(F)` and no `(I)` entries from the parent; the file lists the same two with `(I)(F)`; no other principal (no Administrators, no extra groups). The log has `data_dir_acl result=applied ms=<n>` the first time (note `ms` on an install with many images) and `already_private` afterwards | **untested** |
| Run the app as administrator (Run as administrator) on a fresh scratch folder it creates, then `icacls` on it | The folder is restricted as above (owner `BUILTIN\Administrators` is accepted for an elevated run); the log has `result=applied` | **untested** |
| `--data-dir` pointing at an existing folder made by hand that holds another file; run it twice | `icacls` shows that folder unchanged after both runs although it now holds `history.db`; the log says `data_dir_acl result=not_ours` twice; there is no `.dark-clipboard-data` file in it | **untested** |
| After moving a damaged history aside, use the app, then Settings > History > Clear history | The `*.damaged-*` files and `blobs.damaged-*` folder are still there (the message said so); deleting them by hand works | **untested** |
| Scratch folder: start once, quit, replace `history.db` with a text file, start again | A message window names the folder and says the database file is damaged, with Yes and No; No quits and changes nothing | **untested** |
| Same, choose Yes | The app starts with an empty history; the folder now has `history.db.damaged-<ms>` (and `-wal`/`-shm` if they existed) and `blobs.damaged-<ms>`; nothing was deleted; Settings > History > Restore from backup then works with a backup made before | **untested** |
| Scratch folder with many clips (`--seed-demo-clips 2000`), overwrite 4 KiB in the middle of `history.db` with any bytes, start | The damaged message appears (the `quick_check` at open catches it); the app does not run on the damaged file | **untested** |
| Scratch folder: set the database's `user_version` to 999 (for example with `sqlite3 history.db "PRAGMA user_version=999"`), start | A message says the history was made by a newer version, with only OK; the file's bytes are unchanged afterwards (compare a hash before and after) | **untested** |
| `--headless --data-dir <damaged scratch folder> --exit-after-ms 3000` | Prints the damaged explanation with the folder (no question), exit code 1, nothing renamed | **untested** |
| Squatted pipe: from a second local account (or a small tool) create `\\.\pipe\dark-clipboard-<hash>-s<session>` first, then start the app as the first account (the name is in the code; easiest with a `--data-dir` scope and a test tool) | A message says "Dark Clipboard could not start: another program is using its connection."; the log has `start_failed stage=ipc reason=squatted` | **untested** |
| Install KeePass 2 (classic, `KeePass.exe`), copy a password from an entry with Dark Clipboard capturing | The copy is not stored (excluded app or its marker); on a fresh scratch folder Settings > Privacy lists KeePass as built-in, and removing it asks the password-manager confirmation | **untested** |
| Settings > History > Backup and transfer, English and Vietnamese | The note says a backup is a plain unencrypted zip that keeps no-sync clips and that a synced folder uploads it; after a backup the message repeats "not encrypted" | **untested** |
| Popup (Task 414): look at the buttons right of the search box at 100 %, 150 % and 200 % display scaling | The settings (gear) and close (X) icons are visible, clear and centered in each button; they brighten on hover; clicking opens Settings and closes the popup | **untested** |
| Popup and History (Task 414): favorite a clip (Ctrl+D) | A star icon shows in the row's meta line and in the organize strip of the History window | **untested** |
| Settings > Privacy (Task 414) | A lock icon shows before the "Privacy markers" label | **untested** |
| Popup footer (Task 414; superseded by Task 701, the footer no longer exists: see the Task 701 rows below): select a clip in English, then switch the language to Vietnamese (also a pinned clip and a text clip that allows Copy as...) | Every footer button (Copy, plain text, Copy as..., Pin/Unpin, Delete) is fully visible, none cut off; hovering a button and the screen-reader name still give the full text; the Undo toast row shows its message and the Undo button | **untested** |
| UI kit (Task 700): popup settings (gear) and close (X) icons at 100 %, 150 % and 200 % display scaling, and on a second monitor with a different scale | The new Lucide icons are crisp (no blur, no jagged edges), the same size on both monitors, and use the new palette (grey-blue, `text-2`; brighter on hover) | **untested** |
| UI kit (Task 700): focus ring on every kit control. The kit is not on screen yet except the two popup buttons, which are kept out of the Tab order until Task 701; check each control again when the redesign task that first uses it lands (IconButton, Button, Switch, Menu items) | A 2 px light ring (`focus`) shows one pixel outside the focused control and is visible on the window background | **untested** |
| UI kit (Task 700): popup, hover the settings button and the close button and wait 0.5 s; move the mouse away | A small tooltip below the button after about 0.5 s ("Settings"; "Close" with an Esc key cap, in Vietnamese "Cài đặt", "Đóng"), right-aligned inside the popup, gone when the pointer leaves; never two tooltips at once; the search box keeps the keyboard focus (typing still works) | **untested** |
| UI kit (Task 700): tooltip on keyboard focus (from the first redesign task that makes a kit IconButton focusable; until then n/a) | Tab to an icon button: its tooltip shows after 0.5 s and disappears when the focus moves on | **untested** |
| UI kit (Task 700): the app (popup, History, Settings, Welcome) after the palette change | Every window renders in the new colours (darker canvas, blue-violet accent), no text is hard to read, no layout moved | **untested** |
| Popup layout (Task 701): open the popup at 100 %, 150 % and 200 % display scaling, and on a second monitor with a different scale | Icons are crisp, rows are 48 px high with one preview line and a meta line, no text is clipped (header, segment, status line, rows), the toast floats above the status line and hides no control | **untested** |
| Popup layout (Task 701): Tab walk from the search box | The focus visits the search field, the preview, history, settings and close buttons or the filter segment and More filters button in reading order, then the list; no trap; each stop shows a focus ring; Left and Right move inside the filter segment, Enter and Space toggle a tab, and Enter on a focused header button does not copy a clip | **untested** |
| Popup layout (Task 701): every new key: Ctrl+K, Ctrl+, (Settings), Shift+F10 and the Menu key, Ctrl+1 to Ctrl+9, Ctrl+Shift+F, F1 and Ctrl+/ | Ctrl+K focuses the search and selects its text; Ctrl+, opens Settings; Shift+F10 and the Menu key open the row menu under the selected row; Ctrl+1 is All, 2 to 6 the kinds, 7 Pinned, 8 Favorites, 9 does nothing; Ctrl+Shift+F opens More filters; F1 and Ctrl+/ do nothing at all; none of them types a character into the search box | **untested** |
| Popup layout (Task 701): row menu by right click, the row's more button and by keys | The menu has Copy, Copy as plain text, Copy as... (text clips only), Pin or Unpin, Favorite or Unfavorite, Delete, with their keys; no paste item; Up, Down, Home, End, a letter, Enter or Space choose; Esc closes the menu first and then, on a second press, the popup; copying from the menu returns the focus and never pastes, also with "Enter also pastes" on | **untested** |
| Popup layout (Task 701): More filters by mouse and keys | Today, Last 7 days, Rich text, Color, Collections and Apps (the last two only when they have entries); Right opens Collections or Apps, Left goes back; a chosen filter shows as a removable chip right of the segment and the same chip or menu entry turns it off | **untested** |
| Popup layout (Task 701): hover and select rows | The hovered or selected row shows copy, pin and more buttons (the Alt number and badges hide meanwhile); they stay visible while the pointer is on a button; clicking copy only copies; clicking pin pins that row; double-click copies | **untested** |
| Popup layout (Task 701): Narrator (Win+Ctrl+Enter) on the popup | It reads each row (text, source, age, Pinned, Favorite, Masked), the four header buttons with their keys, each filter tab with its checked state, the toast text and the status line | **untested** |
| Popup layout (Task 701): Vietnamese pass | The header, filter segment (the active tab name), status line, hints, row menu and More filters menu are fully visible with no cut-off text | **untested** |
| Popup layout (Task 701): a masked clip (copy a fake secret while capturing, then open the popup) | The row shows only the fixed "Hidden" line with an eye-off badge: no characters of the secret, in the row, its tooltip, the menus or Narrator | **untested** |
| Popup layout (Task 701): hints with Settings > "Enter also pastes" off and on | Off: Enter Copy, Ctrl+Enter Paste, Space Preview; on: Enter Paste, Space Preview with no Ctrl+Enter hint; in an app where paste is unavailable there is no paste hint | **untested** |
| Popup layout (Task 701): click another window with "Close on focus loss" on, and hover a header button for 0.5 s then type | The popup closes on focus loss as before; the search box keeps typing while a tooltip shows; no tooltip flashes when the popup opens again | **untested** |
| UI kit (Task 700, P4): start the app on a scratch folder, open the popup, the History window and Settings once each, close them, then read private bytes of `dark-clipboard.exe` in Task Manager (Details, Memory (private working set) or `Get-Process`) | At most 4 MiB above the same steps on the 0.1.2 build (baseline numbers for the popup alone: `docs/testing/ui-redesign-baseline.md`) | **untested** |
| App icon (Task 710): the exe in Explorer at small, medium and large icon views; after a rebuild the Windows icon cache may need an Explorer restart | The new clipboard icon on the dark tile with the blue-violet board, clear at every size | **untested** |
| App icon (Task 710): the running app on the taskbar, in Alt+Tab, and in the title bars of History, Settings and Welcome | The same icon in all four places, not the default Windows icon | **untested** |
| App icon (Task 710): tray icon in each state (capturing, paused, off, private, capture unavailable) on a light and a dark taskbar | Capturing: plain icon; paused and off: amber badge with pause bars; private: grey badge with a struck eye; unavailable: red badge with "!"; the clipboard, clip and lines stay distinct | **untested** |
| App icon (Task 710): the tray icon at 100 % and at 150 % display scaling (sign out and in after changing the scale) | Crisp at both (16 px image at 100 %, 24 px at 150 %), no blur or wrong size | **untested** |

## Windows: history layout (Task 703)

All rows **untested** until the owner runs them (agents never click, type, take screenshots or write the real clipboard, GC-19). Use a scratch history (`--seed-demo-clips 200 --data-dir <temp>`, then run with the same `--data-dir`). Open the history window from the tray.

| Check | Expected | Status |
|---|---|---|
| History at 100 %, 150 % and 200 % display scaling, and with the window dragged between two monitors of different DPI | Icons are crisp, rows are 56 px, the sidebar (216 px) and the detail panel (340 px) keep their width, nothing is clipped | **untested** |
| Tab walk from the search field | The focus ring visits: search, date button, select toggle, sidebar, list, detail toolbar, tags +, note, collections + (report the actual order if it differs) | **untested** |
| Sidebar: click or Enter on All, Pinned, Favorites, a type, a collection, a tag, an app | One item per group is on; the list filters as the old left pane did; selecting it again clears it; counts show on Favorites, collections and tags only | **untested** |
| Sidebar: collapse and expand a group (click the header, or Enter, Left and Right on it); close and reopen the window | Apps starts collapsed; every group's state is back to the default after reopening | **untested** |
| Sidebar: + on the Collections header, then rename and delete (pencil and bin appear while a collection is the filter), and Ctrl+Shift+N, Ctrl+Shift+R, Ctrl+Shift+Delete | The same dialogs as before; the keys still work | **untested** |
| Date menu by mouse and by keys (Enter or Space on the focused button, Up, Down, Home, End, a letter, Enter, Esc) | The menu lists Any time, Today, Last 7 days, Last 30 days with a check on the active one; the button shows it; Esc closes the menu first and the window stays open; typing in the search box keeps working after it closes | **untested** |
| Select toggle (list-checks button) | It marks the row under the cursor like Space and shows as on; pressed again it clears the marks; Ctrl+A and Esc still work; the panel says "N clips" with several marked | **untested** |
| Every key of the history window: arrows, Shift and Ctrl with arrows, Home, End, Space, Enter, Shift+Enter, Delete, Menu, Ctrl+P, Ctrl+T, Ctrl+E, Ctrl+D, Ctrl+N, Ctrl+G, Ctrl+L, Ctrl+Shift+L, Ctrl+Shift+N, Ctrl+Shift+R, Ctrl+Shift+Delete, Ctrl+Shift+E, Ctrl+A, Esc | Each does what it did before this task | **untested** |
| Detail toolbar: select a text clip, an image, a file clip, a masked clip and several marked clips | Copy and Copy as plain text only for one clip; Copy as... and Edit a copy... only for one unmasked text clip; Pin, Favorite and Delete follow the marks; disabled buttons look dim and do nothing; Left and Right move between the buttons and Enter presses one | **untested** |
| Detail panel sections: Tags (+, x), Note (click, Enter), Collections (+, x) | Each opens the existing dialog or changes the clip; the panel updates; the Collections chips list the real collections of the clip | **untested** |
| A masked clip (copy a fake API key with the sensitive setting "Store masked"), in English and Vietnamese | The row, the detail panel, the tooltips, Narrator and the delete confirmation show only "Hidden: may contain a secret" or the fixed texts, never the first characters of the secret; a search for its first letters does not highlight it | **untested** |
| Focus on the date button, select toggle, detail toolbar, Tags, Note, Collections or the sidebar, then Ctrl+P, Del, Ctrl+A, Ctrl+Shift+E | Only Esc reaches the history key map while one of them has focus (same rule as the old filter chips); the others do nothing until focus returns to the search box or list. Also check focus is released when a focused section disappears (select a masked clip, or mark several clips) and the keys work again | **untested** |
| Narrator on the window | It reads rows (kind, source, age, tag, Pinned, Favorite, Masked), sidebar items with their counts, the toolbar buttons and the detail toolbar buttons by name | **untested** |
| Vietnamese pass | The toolbar, the date button, the sidebar, the detail toolbar tooltips, the sections and the status line fit with nothing clipped at 1000 px | **untested** |
| Copy something so a toast shows | The toast floats above the status line and hides no control | **untested** |

## macOS

All rows **untested** (no hardware, Q-04).

## Linux (X11, KDE Wayland, wlroots, GNOME)

All rows **untested** (no hardware, Q-04).
