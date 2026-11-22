# Task 208: Autostart and first-run welcome (consent)

**Agent:** `builder` · **Model:** sonnet · **Branch:** `task/208-autostart-welcome` · **Review:** `reviewer` (opus; consent semantics)
**Global Constraints:** GC-06, GC-10, GC-14. **Plan:** Phase 3 Task 208; journey J6; owner prompt item "capture disabled until the user explicitly enables it" (D-036, decided in D-044).

## Requirements
1. **First run:** capture is **off** (`capture_enabled = false`) until the user presses **Start** on a one-screen welcome: shortcut, what is never captured (password managers, OS-concealed, Darkshot-excluded), where data lives, "no account, no cloud". Buttons: Start (enables capture, persists), Not now (keeps capture off; tray shows "not capturing"). The welcome shows once; reachable again from About.
2. **Start at login:** `HKCU\Software\Microsoft\Windows\CurrentVersion\Run` value pointing to the current exe with no extra args; toggle in settings; default **off**; the welcome has a "Start with Windows" checkbox, unchecked by default.
3. Upgrading users with existing settings are not shown the welcome and keep their capture state.

## Tests
State tests (fresh settings → capture off + welcome; Start → enabled; Not now → off; existing settings → no welcome). Registry access behind a trait; the real registry write is an `#[ignore]` test not run by agents (it changes the owner's login items).
