# Phase 4: macOS and Linux Adapters

> This phase **starts with spikes on real machines**. Task packages for 31x/32x are written only after the spikes (300, 301) report. A spike may change the plan; it must not change ADR decisions without a new ADR.

**Goal:** Capture, tray, shortcut and popup on macOS and Linux, with every platform-matrix row either **T** (tested) or explicitly unsupported in the UI and README.
**Spec:** `docs/architecture/platform-compatibility.md`, ADR-0002, ADR-0004, ADR-0001 revisit triggers.
**Hardware needed (owner):** a Mac (macOS 14 or 15 and 26 if possible); a Linux machine or VMs with KDE Plasma 6 Wayland, GNOME Wayland (Fedora or Ubuntu current), Sway or Hyprland, and an X11 session. Agents cannot run these; the owner or a CI runner with a desktop session executes the checklists.

## Review Focus
1. macOS: the pasteboard privacy setting "Ask" → what the app does on first read (no crash loop, clear onboarding).
2. Linux Wayland: the compositor restarts or data-control disappears mid-session → the app shows "unavailable" and recovers when it returns.
3. X11: Dark Clipboard owns CLIPBOARD after a copy-back and then quits → content handed to the clipboard manager or documented loss.
4. A KeePassXC copy with `x-kde-passwordManagerHint` on KDE Wayland → skipped.
5. Linux without a StatusNotifier host (vanilla GNOME) → no tray; the popup is still reachable via `--toggle`, which is documented.

## Tasks

| Task | Title | Depends | Agent / model | Reviewer | Risk | Acceptance criteria |
|---|---|---|---|---|---|---|
| 300 | Spike macOS: `changeCount` poll, marker types, frontmost app, pasteboard privacy behaviour on macOS 26 (setting Ask/Allow), `NSPanel` non-activating popup with Slint, `global-hotkey`, `tray-icon` | P3 | specialist / opus + owner | reviewer / opus | **High** | Written report `docs/spikes/macos.md` with measured results; ADR-0001 trigger 3 answered |
| 301 | Spike Linux: data-control probe on KDE, GNOME, Sway/Hyprland; X11 XFixes; `ksni` tray vs `tray-icon`; portal GlobalShortcuts via `ashpd`; Slint on Wayland and X11 | P3 | specialist / opus + owner | reviewer / opus | **High** | `docs/spikes/linux.md`; GNOME conflict (Mutter 49.2 listed vs CopyQ docs) resolved by test |
| 310 | `darkclip-macos`: monitor, snapshot_meta (nspasteboard markers), read/write (UTF-8, HTML, RTF, PNG/TIFF→PNG, file URLs), source (frontmost; `org.nspasteboard.source`) | 300 | specialist / opus | security-reviewer / opus | High | Matrix macOS column T |
| 311 | macOS shell integration: menu bar, `SMAppService` login item, Accessibility-permission flow for auto-paste (`CGEventPost`), onboarding for pasteboard privacy | 310 | builder / sonnet | reviewer / opus | Medium | J6 macOS path works |
| 320 | `darkclip-linux` X11 backend (`x11rb`, XFixes, selection owner thread, `SAVE_TARGETS` hand-over) | 301 | specialist / opus | reviewer / opus | High | Matrix X11 column T |
| 321 | `darkclip-linux` Wayland backend (`ext-data-control-v1`, fallback `zwlr-data-control-unstable-v1`), runtime probe, `Unavailable` state | 301 | specialist / opus | security-reviewer / opus | **High** | KDE + wlroots columns T; GNOME T or unsupported |
| 322 | Linux shell: tray (ksni), shortcut (portal → X11 grab → user-bound `--toggle`), XDG autostart, Unix-socket IPC for `--toggle` | 320, 321 | builder / sonnet | reviewer / sonnet | Medium | Each backend path exercised |
| 330 | CI: macOS and Linux build + unit tests already in Phase 1; add adapter unit tests (pure mapping), `cargo deny` | 310, 320 | scaffolder / sonnet | reviewer / sonnet | Low | Green CI |
| 331 | Packaging prototypes: macOS `.app` (unsigned) + `.dmg`, Linux AppImage + `.deb` (Flatpak deferred: sandbox blocks data-control/X11 access unless permissions are granted; evaluate later) | 311, 322 | builder / sonnet | reviewer / sonnet | Medium | Artifacts install and run on the test machines |
| 332 | Matrix update, README platform section, phase review | all | reviewer / opus | — | — | Every cell T or explicitly unsupported |
