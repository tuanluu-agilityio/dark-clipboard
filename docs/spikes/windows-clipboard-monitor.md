# Spike: Windows clipboard monitoring

Date: 2026-11-22 · Machine: Windows 11 Home 10.0.26300 x64 · Rust 1.100.0 · `windows` 0.62.2
Code: `spikes/win-clipboard-monitor/` (throwaway, not a workspace member)
Raw output: `spikes/win-clipboard-monitor/spike-output-2026-11-22.txt` (no clipboard content is printed; lengths and format names only)
Run: `cargo run` in the spike directory. **Side effect:** it overwrites the clipboard during the run and then restores the previous text. Non-text content is lost.

## Questions and answers

| # | Question | Answer (observed) |
|---|---|---|
| 1 | Does a message-only window (`HWND_MESSAGE`) receive `WM_CLIPBOARDUPDATE` after `AddClipboardFormatListener`? | Yes, for writes from this process and from another process (PowerShell) |
| 2 | Can Darkshot's exclusion formats be detected before reading content? | Yes. `EnumClipboardFormats` lists `ExcludeClipboardContentFromMonitorProcessing`, `CanIncludeInClipboardHistory`, `CanUploadToCloudClipboard`; DWORD values are readable; content need not be touched |
| 3 | Notification latency | 0.35–4.7 ms from `SetClipboardData` to handler entry (same process) |
| 4 | Burst behaviour | 20 writes in 239 ms → 20 notifications, but only 2 distinct sequence numbers visible when handled. Notifications queue; data does not. Intermediate contents are unrecoverable |
| 5 | OLE writers (.NET `Set-Clipboard`) | 3 notifications per copy: (a) only `DataObject` and `Ole Private Data` (delayed rendering), (b) real formats, (c) after `OleFlushClipboard` with no owner window |
| 6 | Contention | The listener needed 4 `OpenClipboard` attempts (20 ms apart) while PowerShell held the clipboard |
| 7 | Source app | `GetClipboardOwner` → `GetWindowThreadProcessId` → `QueryFullProcessImageNameW` gave `powershell.exe`; the foreground window was `WindowsTerminal.exe`. Owner is the better signal. It is absent on the post-flush notification |
| 8 | Self-write suppression | The sequence number read right after our own write equals the one seen in the resulting notification |
| 9 | Image | `CF_DIB` write → OS synthesizes `CF_BITMAP`, `CF_DIBV5` |
| 10 | Files | `CF_HDROP`, `FileNameW`, `FileName` present |

## Design consequences (carried into ADR-0002 and the Windows adapter task)

1. Dedupe by `GetClipboardSequenceNumber`, not by notification count.
2. Debounce about 75 ms, and snapshot the owner at the first notification of a burst.
3. Skip snapshots that contain only `DataObject`/`Ole Private Data`.
4. Retry `OpenClipboard` up to 10 × 20 ms, then drop the change.
5. Mandatory exclusion check on the enumerated formats, before any `GetClipboardData` for content.
6. Store our own post-write sequence number to ignore copy-backs.

## Not tested (open)

- Real Darkshot build writing to the clipboard (same format set was replicated exactly from `darkshot-windows/src/clipboard.rs`).
- KeePassXC, 1Password, Bitwarden on Windows (expected to set `ExcludeClipboardContentFromMonitorProcessing` or `CanIncludeInClipboardHistory=0`; unverified).
- Office, browsers (HTML Format), RDP clipboard (`rdpclip.exe`), elevated source apps, and Windows 10.
