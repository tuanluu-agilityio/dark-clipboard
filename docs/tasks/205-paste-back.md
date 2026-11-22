# Task 205: Copy back and paste into the previous app

**Agent:** `specialist` · **Model:** opus · **Branch:** `task/205-paste-back` · **Review:** `security-reviewer` (opus)
**Global Constraints:** GC-03, GC-06, GC-10, GC-13. **Plan:** Phase 3 Task 205; Review Focus 3; owner UX prompt (D-036): restore vs auto-paste must be explicit.

## Requirements
1. **Restore to clipboard** (Enter) is the default: `ClipApi` copy-back through the actor (Task 104 write, self-write suppression); the popup closes; focus returns to the previously focused window.
2. **Auto-paste** (Ctrl+Enter, and a setting "Enter also pastes", default **off**): after copy-back, restore focus to the recorded window and send Ctrl+V with `SendInput`. Implemented as a `PasteInjector` in `darkclip-windows` (`PlatformBundle.paste`), all unsafe in `win.rs`.
3. **Never paste blind:** paste only if the recorded target window still exists, is visible, belongs to the same process id recorded at show time, and became foreground. Otherwise copy only and show a toast "Copied — paste manually".
4. **UIPI:** if the target process is elevated and we are not, do not send input; copy only + toast explaining why.
5. Capabilities: `auto_paste` becomes `Native` on Windows; UI hides Ctrl+Enter hints where unsupported.
6. Content never logged; the toast text is static.

## Tests
Pure decision tests (target checks → Paste / CopyOnly(reason)) with a fake window seam. `#[ignore = "sends input"]` integration tests are written but **never run by agents** (they inject keystrokes while the owner uses the machine). Manual matrix rows (Notepad, VS Code, Edge, Word if available, elevated Notepad) added to the checklist for the owner.
