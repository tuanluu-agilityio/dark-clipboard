# Project brief

**Dark Clipboard**: a local-first, AI-native clipboard manager for Windows, macOS and Linux, written in Rust. Repository `dark-clipboard`. Darkshot ecosystem.

## Goals
- A fast, keyboard-first clipboard history that is useful with no account, no cloud and no AI provider.
- Privacy that can be tested: OS/app markers and Darkshot exclusions always respected; content-free logs; retention by default.
- One Application API for the desktop UI, the CLI and MCP, so AI access is a permission model rather than a side door.
- Honest per-platform and per-compositor support claims, backed by tests.

## Non-goals
- Cloud services run by us; mandatory accounts; telemetry by default.
- Scripting or plugin engines; window-title capture; OCR (Darkshot owns it).
- GNOME Wayland support at any cost (decided per evidence in Phase 4).

## Must avoid
- Reading or storing content marked private by the OS, password managers or Darkshot.
- Copying Darkshot's Windows-specific code into portable crates.
- Claiming platform support without a test.
- P2 (CLI/MCP/AI) work before 0.1.0.

## Product levels
P0 core manager · P1 organization · P2 CLI, MCP, AI, automation · P3 sync and ecosystem. Details: `docs/product/feature-matrix.md`.
