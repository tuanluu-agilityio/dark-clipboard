# Platform verification checklist

Status legend: **T** tested and passed (date, app version), **F** tested and failed, **n/a** not available on the test machine, **untested**.
Rules: no row is marked T without a run recorded here. Clip content is never printed or stored in this file; only format names, kinds, sources and counts.

## Windows (Task 107, 2026-11-22)

Machine: Windows 11 Home 10.0.26300 x64 (the owner's machine). Build: `dark-clipboard` 0.0.1, release, commit `694600f` + the Task 107 test fix.
Method: the owner copied in each app while `dark-clipboard --headless --data-dir <scratch>` ran. A read-only watcher logged, for every clipboard sequence change, the **format names** and the owner process (no content). The scratch database was summarised with a query that selects kind, source, sensitivity, counts and MIME/format names only (no preview, no bytes). The scratch data was deleted afterwards. The verification was run interactively by the orchestrator, because it needed back-and-forth with the owner (D-041).

### Automated real-clipboard tests

Command: `cargo test -p darkclip-windows -- --ignored --test-threads=1` (with the owner's consent; it overwrites the clipboard).
First run: 9 passed, 1 failed. `real_clipboard_write_text_roundtrip_vietnamese` expected `"Tiế"` from a 6-byte cap; `"Tiế"` is 5 bytes and the next char `n` fits, so the code's `"Tiến"` was correct and the **test expectation** was wrong. Fixed the expectation; re-run of that test: passed. Result: **10/10 pass**, covering same/other-process signals, owner = `powershell.exe`, the Darkshot format set detected as `DarkshotExcluded`, PowerShell OLE phases, Vietnamese text round trip and capped read, non-ASCII HTML, a Windows-synthesised `CF_DIBV5`, copy-back sequence suppression (signal seq = returned post-close seq, no further signal within 300 ms), and an image copy-back readable by `read` with no marker formats written (D-012, D-038).

### Manual rows

| Row | Formats observed (names only) | Result in Dark Clipboard | Status |
|---|---|---|---|
| KeePassXC 2.8.0: copy password (Ctrl+C) and username (Ctrl+B) | `DataObject` phase, then `CF_UNICODETEXT`, `CF_TEXT`, `ExcludeClipboardContentFromMonitorProcessing`, `CanIncludeInClipboardHistory`, `CanUploadToCloudClipboard`, `Ole Private Data`, … ; owner `KeePassXC`. Auto-clear later leaves an empty clipboard | Not stored (0 clips) | **T** 2026-11-22 |
| Darkshot: Copy image | `CF_DIB`, `ExcludeClipboardContentFromMonitorProcessing`, `CanIncludeInClipboardHistory`, `CanUploadToCloudClipboard`, `CF_BITMAP`, `CF_DIBV5`; owner none | Not stored | **T** 2026-11-22 |
| Darkshot: Copy link (after R2 upload) | `CF_UNICODETEXT`, `ExcludeClipboardContentFromMonitorProcessing`, `CanIncludeInClipboardHistory`, `CanUploadToCloudClipboard`, …; owner none | Not stored | **T** 2026-11-22 |
| Notepad 11.2609 (text) | 3 sequence changes per copy: owner `Notepad` (with `EnterpriseDataProtectionId`), then owner none after the OLE flush | Stored as `text`, **source = None** (F-14) | **T** (capture); source attribution **F** |
| Chrome 158: page selection with Vietnamese text | `HTML Format`, `CF_UNICODETEXT`, Chromium internal formats | Stored as `rich_text` (HTML + plain), source `chrome.exe` | **T** 2026-11-22 |
| Explorer: copy 2 files | `CF_HDROP`, `FileNameW`, Shell formats, … | Stored as `files` (`text/uri-list` from `CF_HDROP`), source `explorer.exe` | **T** 2026-11-22 |
| Snipping Tool 11.2609 | `PNG`, `CF_DIB`, `CF_DIBV5`, `CanUploadToCloudClipboard`, `CanIncludeInClipboardHistory` (values allow), … | Stored as `image` (registered `PNG`), source `SnippingTool.exe`; the allowing DWORD values were read correctly and did not cause a skip | **T** 2026-11-22 |
| Word / Excel | — | — | **n/a** (not installed) |
| Bitwarden desktop, 1Password | — | — | **n/a** (not installed) |
| RDP clipboard | — | — | **n/a** |
| Elevated source app (source should resolve or be None) | — | — | **untested** (optional row skipped) |
| Copy-back from the UI, then immediate copy in another app (no stall, no recapture) | — | Covered by the automated copy-back test; manual check needs the Phase 3 UI | **untested** (manual part) |
| Win+V history after a Dark Clipboard copy-back (D-012) | — | Automated test confirms no marker formats are written; manual Win+V check needs the Phase 3 UI | **untested** (manual part) |
| Real Ctrl+C in a console | — | Shutdown measured via `--exit-after-ms` (Task 106) | **untested** |

### Findings

- **F-14 (source lost on OLE writers):** writers that use OLE with a flush (Notepad, PowerShell `Set-Clipboard`) bump the sequence number at each phase, and the last phase has no owner. Each new seq starts a fresh pending change (D-027), so the stored clip has `source = None`. No privacy leak for apps that set markers (KeePassXC, Darkshot), but an excluded app **without** markers that writes this way would not be matched by the excluded-app list. Needs a fix in the actor (carry the previous seq's source into an ownerless follow-up seq within the debounce window) with a security review. Scheduled as a follow-up task.
- KeePassXC 2.8.0 on Windows sets all three Windows markers: risk R-02 is closed for KeePassXC; still open for Bitwarden and 1Password (not tested).
- Darkshot's Copy link is marked as well, so Dark Clipboard never stores Darkshot links.

## macOS

All rows **untested** (no hardware, Q-04).

## Linux (X11, KDE Wayland, wlroots, GNOME)

All rows **untested** (no hardware, Q-04).
