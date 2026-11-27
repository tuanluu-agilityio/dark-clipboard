# Spike: translate-in-place hotkey (throwaway, P2 AI feasibility)

Question: can one shortcut replace the text you just typed (Vietnamese) with its English
translation, in any app, using the user's own Anthropic API key, without touching Ctrl+C?

Not part of the product. Never import or copy into `crates/` or `apps/` (spikes rule).
A real feature needs its own design, plan and security review (P2, `ai-integration.md` §4).

## Run

```powershell
$env:ANTHROPIC_API_KEY = "sk-ant-..."
cargo build --release
.\target\release\translate-hotkey-spike.exe
```

Then type in any text field and press **Ctrl+Alt+T**. **Esc** cancels while it waits. Ctrl+Z in
the app brings the original back. Ctrl+C in the console window quits the spike.

Optional settings: `DCT_MODEL` (default `claude-opus-5-5`), `DCT_EFFORT` (default `low`),
`DCT_TARGET` (`auto` = Vietnamese to English, anything else to Vietnamese; `en`; `vi`),
`DCT_RESTORE_MS` (default 400, wait before the old clipboard text goes back).

## What it does

1. Waits until Ctrl, Alt, Shift, Win and T are released.
2. Saves the clipboard text, sends Ctrl+C. If the clipboard does not change in 400 ms (nothing
   selected), sends Ctrl+A then Ctrl+C.
3. Sends the text to `POST /v1/messages` (raw HTTP; Rust has no official SDK) with a
   translation-only system prompt and the text in `<text>` tags.
4. If the same window is still in front, writes the translation and sends Ctrl+V; otherwise it
   leaves the translation on the clipboard and beeps.
5. Puts the previous clipboard text back.

Every clipboard write by the spike carries the OS exclusion markers
(`ExcludeClipboardContentFromMonitorProcessing`, `CanIncludeInClipboardHistory=0`,
`CanUploadToCloudClipboard=0`), so Dark Clipboard and Win+V do not store the translation or
the restored text. The console prints lengths and timings only, never the text.

## Known limits (spike)

- The **Ctrl+C sent in step 2 is written by the target app**, so a running Dark Clipboard stores
  the Vietnamese source as a normal clip. The product IPC has no pause command; the real
  feature lives inside the app and would skip that one change.
- Clipboard **text only**: an image or file list on the clipboard before the shortcut is lost.
- No "Translating..." overlay; feedback is the console and a beep on failure.
- Apps running as administrator ignore the synthetic keys (UIPI).
- Ctrl+A in a single-line chat box selects the box; in some apps (e.g. a browser page with no
  focused field) it selects the page.

## Owner test rows (not run by an agent, GC-19)

| App | Selection | Whole field (no selection) | Ctrl+Z restores | Latency (console) |
|---|---|---|---|---|
| Notepad | not tested | | | |
| Chrome/Edge textarea (Gmail, ChatGPT box) | not tested | | | |
| Zalo | yes | yes | yes | see below |
| Slack / Teams | Slack yes | Slack yes | Slack yes | see below |
| VS Code | yes | yes | yes | see below |
| Word | not tested | | | |

Owner result 2026-11-27 (G1 passed): works in Zalo, Slack and VS Code; selection is translated,
no selection translates the whole field, Ctrl+Z restores the original. Console lines reported
(default model `claude-opus-5-5`, effort low): short texts (37-57 chars) API 1.0-1.6 s, total
2.0-2.5 s (one 3.8 s); long texts (585-713 chars) API 2.9-5.0 s, total 3.8-7.1 s. Also seen:
Esc cancel, "focus moved: left on the clipboard", and "the app did not copy anything" (about
1.1 s, no text field focused). Overhead beyond the API is the copy wait and the 400 ms restore
wait; the product shows "done" at paste time and restores in the background.
