# Personas and User Journeys

Status: Phase 0, 2026-11-22. Personas are assumptions derived from the reference products' audiences and the Darkshot owner's context, not from user research.

## Personas

| Persona | Context | Needs | Fears |
|---|---|---|---|
| **Minh, developer** | Windows + WSL at work, Linux laptop at home. Copies commands, stack traces, JSON, tokens. | Fast retrieval of recent snippets, plain-text paste, pinned commands | Tokens and keys ending up in a history file |
| **Lan, technical writer** | macOS, copies text and screenshots between docs, browser and Darkshot | Images in history, collections per project (P1), notes | Losing a clip she copied an hour ago |
| **Huy, privacy-conscious user** | Linux (KDE Wayland), uses KeePassXC | History without any cloud, clear indication of what is captured | Password manager copies being stored |
| **Agent user** | Uses Claude Code or another MCP client | Let the agent search "what I copied about X today" | The agent reading secrets, or modifying the clipboard behind their back |

## Core journeys (P0)

### J1. Find and reuse a recent clip
1. User presses the shortcut. Popup appears near the cursor (or centered on the active monitor), focused, search box empty, recent clips listed, pinned first.
2. User types "kubectl". The list filters on each keystroke.
3. Arrow down to the second result, Enter. The clip goes to the clipboard; the popup closes; focus returns to the previous app.
4. Optional: Ctrl+Enter pastes immediately (Windows; macOS with Accessibility permission; Linux X11).
Acceptance: no mouse needed; total time from shortcut to paste < 2 s for a practiced user.

### J2. Copy a password from a password manager
1. User copies a password in KeePassXC / 1Password / Bitwarden.
2. Dark Clipboard sees the change notification, enumerates formats, finds a concealed marker (or the app is on the excluded list), and stores nothing.
3. Nothing appears in history. The diagnostics log records at most "change skipped: concealed" with no content and no app name, unless debug logging is on, in which case the app id may be logged.
Acceptance: verified per password manager per OS (matrix in `platform-compatibility.md`).

### J3. Copy from Darkshot
1. User captures a region in Darkshot and chooses Copy image or Copy link.
2. Darkshot writes `ExcludeClipboardContentFromMonitorProcessing`, `CanIncludeInClipboardHistory=0`, `CanUploadToCloudClipboard=0` (observed in `darkshot-windows/src/clipboard.rs`).
3. Dark Clipboard skips it, exactly as Windows clipboard history does.
Acceptance: verified on Windows by the Phase 0 spike (scenario S2) with the same format set; to be re-verified with a real Darkshot build in Phase 2.

### J4. Pause before sensitive work
1. User clicks tray → Pause for 1 hour (or the shortcut for private mode).
2. Tray icon changes; tooltip shows "Paused until 15:30".
3. Copies during pause are not read. When the pause ends, capture resumes from the next change (no backfill).

### J5. Clean up
1. User opens Settings → History, sets "Keep 30 days", clicks "Clear history" (pinned kept by default).
2. Rows, FTS entries and blob files are deleted; storage stats update.

### J6. First run
1. Installer or unzip; first launch shows a one-screen welcome: shortcut, what is never captured, where data lives, "no account, no cloud". One button: Start.
2. On macOS: explains optional Accessibility permission (for auto-paste only) and the possible pasteboard privacy prompt.
3. On Linux: shows detected capture backend (X11 / ext-data-control / wlr-data-control / unavailable) and shortcut backend.

## Later journeys

- **J7 (P1) Organize:** add a clip to collection "Project X", tag "sql", add a note; later filter by collection.
- **J8 (P2) CLI:** `darkclip search "invoice" --limit 5 --json | jq .data[0].id`.
- **J9 (P2) Agent:** the user enables MCP in Settings, approves client "claude-code" for scope `clips:read-metadata` + `clips:read-content` (excluding sensitive), the agent calls `search_clips`, and Settings → AI access shows the audit entry (tool name, time, count, no content).
- **J10 (P2) Privileged agent action:** the agent requests `copy_clip_to_clipboard`. This requires the scope `clipboard:write`, which is off by default; when granted, the desktop shows a toast "Claude Code put a clip on your clipboard".
