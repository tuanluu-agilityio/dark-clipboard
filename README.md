# Dark Clipboard

A local-first clipboard manager for Windows, macOS and Linux, written in Rust. It is part of the Darkshot family.

> **Status: design phase (Phase 0 complete).** There is no usable build yet. This repository currently holds the product and architecture blueprint, measured feasibility spikes, and the implementation plan.

## What it will do (first release)

- Remembers what you copy (text, rich text, images, file lists) and finds it again as you type.
- Keyboard first: one shortcut, type, Enter.
- **Never stores** what your OS, your password manager or Darkshot marked private. Pause, private mode, excluded apps, retention limits.
- No account, no cloud, no telemetry. Works offline.
- AI tools (via MCP, later release) see nothing unless you grant access, and only what you grant.

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

Developer run (Windows only for now, no UI): `cargo run -p dark-clipboard -- --headless` captures the clipboard into `%LOCALAPPDATA%\DarkClipboard` until Ctrl+C. `--data-dir <path>` picks another folder. A content-free log is written to `<data dir>\logs\dark-clipboard.log`; on stop it records how many changes were skipped, per reason (names and counts only). Windows capture was tested on a real clipboard in Task 107 (Windows 11; see the [platform checklist](docs/testing/platform-checklist.md)).

## Licence

Not decided yet (open question Q-01 in `memory-bank/activeContext.md`).
