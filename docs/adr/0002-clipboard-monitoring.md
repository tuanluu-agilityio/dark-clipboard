# ADR-0002: Clipboard monitoring strategy

- Status: **Accepted** for Windows (tested). **Proposed** for macOS and Linux (documented, untested).
- Date: 2026-11-22. Evidence: `docs/spikes/windows-clipboard-monitor.md`, `docs/architecture/platform-compatibility.md`

## Decision

Each platform adapter provides an event source producing `ChangeSignal { seq }` on its own thread. A single `CaptureActor` in `darkclip-app` owns every clipboard read and write.

| Platform | Mechanism | Sequence source |
|---|---|---|
| Windows | `AddClipboardFormatListener` on a message-only window, `WM_CLIPBOARDUPDATE` | `GetClipboardSequenceNumber()` |
| macOS | Poll `NSPasteboard.general.changeCount` every 300 ms on a background thread; read on the main thread only if AppKit requires it (verify) | `changeCount` |
| Linux X11 | XFixes `SelectSelectionInput(CLIPBOARD)` (PRIMARY optional, off by default) via `x11rb` | local counter per `XFixesSelectionNotify` |
| Linux Wayland | Probe `ext_data_control_manager_v1`, then `zwlr_data_control_manager_v1`, via `wayland-client` + `wayland-protocols(-wlr)` | local counter per `selection` event |
| Wayland without data-control | `Unsupported` capability. UI shows "capture unavailable". Optional XWayland X11 monitoring only when the user enables it, labelled "only sees X11 apps" | — |

Common rules: dedupe by sequence; 75 ms debounce; snapshot source at first signal; enumerate formats and check mandatory markers before reading; retries for busy clipboards; suppress self-writes by recorded sequence.

## Rejected alternatives

- **`arboard` / `clipboard-rs` / `copypasta` for monitoring:** these are read/write libraries. Some offer watchers that poll, and none exposes marker types before reading content in a uniform way. We may still use `arboard` for image encode/decode helpers, but not for policy-relevant reads.
- **`SetClipboardViewer` chain (Windows):** legacy, fragile chain semantics. `AddClipboardFormatListener` is the documented replacement.
- **Polling on Windows/X11:** wastes CPU where events exist.
- **Shelling out to `wl-paste --watch`:** an extra runtime dependency, and data would flow through a child process before policy checks. Acceptable only as a diagnostic tool.
- **GNOME Shell extension (CopyQ approach):** packaging limits (no Flatpak or AppImage) and a JavaScript extension to maintain. Postponed; reconsidered only if GNOME users are a stated priority and GNOME has no data-control support.

## Consequences

- Rapid successive copies faster than the debounce collapse into the last one (accepted; documented).
- macOS polling has a worst-case 300 ms capture delay plus a tiny constant CPU cost.
- Wayland support is per compositor and stated per compositor.
