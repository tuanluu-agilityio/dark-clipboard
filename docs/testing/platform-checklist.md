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

## macOS

All rows **untested** (no hardware, Q-04).

## Linux (X11, KDE Wayland, wlroots, GNOME)

All rows **untested** (no hardware, Q-04).
