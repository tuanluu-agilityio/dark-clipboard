# ADR-0006: Process model (desktop, CLI, MCP)

- Status: **Accepted**, 2026-11-22

## Decision

- The **desktop app** (`dark-clipboard`) is the only process that monitors or writes the OS clipboard, and the only writer to `history.db` while it runs.
- **CLI** (`darkclip`) and **MCP** (`darkclip mcp serve`) open `history.db` **read-only** (`SQLITE_OPEN_READ_ONLY`, WAL) for queries.
- **Mutations** from CLI/MCP (pin, tag, note, collections, copy to clipboard) are sent to the desktop over a local IPC channel: Windows named pipe `\\.\pipe\dark-clipboard-<user-sid>` with a DACL for the current user only; macOS/Linux Unix socket in the per-user runtime dir with mode 0600. Messages are JSON in the `api-envelope` shape. If the desktop app is not running, mutations return `APP_NOT_RUNNING` (P2). A headless `--daemon` mode may come later.
- The same IPC carries `--toggle` (open popup) from a second instance or a desktop-environment shortcut (Linux Wayland fallback).

## Why

- Keeps the single-clipboard-actor and single-DB-writer rules across processes.
- Read-only DB access for CLI/MCP needs no IPC and works when the app is closed (history browsing).

## Rejected

- CLI/MCP writing the DB directly: two writers means schema and migration races, and FTS consistency risk.
- Everything over IPC: the CLI would need the app running for every read.
