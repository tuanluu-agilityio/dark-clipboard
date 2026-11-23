# Dark Clipboard

A local-first clipboard manager for Windows, macOS and Linux, written in Rust. It is part of the Darkshot family.

> **Status: design phase (Phase 0 complete).** There is no usable build yet. This repository currently holds the product and architecture blueprint, measured feasibility spikes, and the implementation plan.

## What it will do (first release)

- Remembers what you copy (text, rich text, images, file lists) and finds it again as you type.
- Keyboard first: one shortcut, type, Enter.
- **Never stores** what your OS, your password manager or Darkshot marked private. Pause, private mode, excluded apps, retention limits.
- No account, no cloud, no telemetry. Works offline.
- AI tools (via MCP, later release) see nothing unless you grant access, and only what you grant.

On first start a one-screen welcome explains what is never captured and where your data lives. **Nothing is captured until you press Start**; "Not now" keeps capture off, and the welcome stays reachable from Settings > About and the tray. Start at login is off unless you tick it.

## Platform status

Only Windows has been tested so far. See `docs/architecture/platform-compatibility.md` for every capability with its evidence (tested, documented, reported or unknown). macOS and Linux are designed but untested. GNOME on Wayland may not support background capture.

## Documentation

- Product: [vision](docs/product/product-vision.md), [features](docs/product/feature-matrix.md), [MVP scope](docs/product/mvp-scope.md), [journeys](docs/product/user-journeys.md)
- Architecture: [overview](docs/architecture/system-overview.md), [platforms](docs/architecture/platform-compatibility.md), [data model](docs/architecture/data-model.md), [AI interfaces](docs/architecture/ai-integration.md)
- Security: [threat model](docs/security/threat-model.md), [privacy design](docs/security/privacy-design.md)
- Decisions: [ADRs](docs/adr/)
- Plans: [roadmap](docs/plans/roadmap.md), [phase plans](docs/plans/), [task packages](docs/tasks/)
- Spikes: [Windows clipboard](docs/spikes/windows-clipboard-monitor.md), [UI shell comparison](docs/spikes/ui-shell-comparison.md)

## Development

Contributors and AI agents: start with `CLAUDE.md` / `AGENTS.md` and `memory-bank/activeContext.md`. Quality gate: `powershell -File scripts/check.ps1` (Windows) or `bash scripts/check.sh`.

Windows app: `cargo run -p dark-clipboard` starts the app hidden with a tray icon; `Ctrl+Shift+V`, a tray click or `--toggle` opens the popup near the cursor: type to search, arrows to move, Enter to copy back (the focus returns to the app you were in), Ctrl+Enter to copy and paste into that app (never into an elevated app or the taskbar: then it only copies and says so), Esc to close (a second start, `--toggle` or `--quit` talks to the running one through a per-user named pipe). To try a large history without touching your own: `cargo run -p dark-clipboard -- --seed-demo-clips 10000 --data-dir <empty folder>`, then `cargo run -p dark-clipboard -- --data-dir <same folder>` (synthetic text only; refused for the default data folder). Developer run (Windows only for now, no window): `cargo run -p dark-clipboard -- --headless` captures the clipboard into `%LOCALAPPDATA%\DarkClipboard` until Ctrl+C. `--quit` does not stop a `--headless` run (it has no instance pipe): stop it with Ctrl+C, or end the process in Task Manager. `--data-dir <path>` picks another folder. On Windows the UI draws with Slint's software renderer (much less memory and no idle CPU than the GPU renderer); setting the environment variable `SLINT_BACKEND` (for example `SLINT_BACKEND=winit-femtovg`) overrides that choice, and the start line in the log records `renderer=software` (requested), `default` or `env`. If selecting the software renderer fails the log has `renderer_fallback`, but the app may then not start at all. A content-free log is written to `<data dir>\logs\dark-clipboard.log`; on stop it records how many changes were skipped, per reason (names and counts only). Windows capture was tested on a real clipboard in Task 107 (Windows 11; see the [platform checklist](docs/testing/platform-checklist.md)).

## Licence

GPL-3.0-only (see [LICENSE](LICENSE)). The UI uses Slint under its GPLv3 option.
