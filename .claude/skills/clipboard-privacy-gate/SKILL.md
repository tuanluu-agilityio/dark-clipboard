---
name: clipboard-privacy-gate
description: Use before writing or reviewing any Dark Clipboard code that reads, writes, stores, logs, searches or exposes clipboard content - capture actor, platform adapters, policy, sensitive detection, storage deletion, CLI/MCP. Checklist that keeps the privacy promises enforceable.
---

# Clipboard privacy gate

Dark Clipboard's privacy promises (`docs/security/privacy-design.md`) hold only if every code path respects these rules. Go through the list for the code you are touching. Any "no" is a blocking defect.

## Capture path
- [ ] Content is read only in `crates/darkclip-app/src/capture/` (the capture actor module), and only after `policy::decide_pre_read` returned `Decision::Read`.
- [ ] The adapter's `snapshot_meta` enumerates formats and markers **without** reading content formats (Windows DWORD marker formats are the only data read).
- [ ] Mandatory markers (privacy-design §3) cannot be disabled by any setting, flag, env var, `cfg`, or debug mode.
- [ ] Pause and private mode drop the signal before `snapshot_meta`.
- [ ] Resume does not backfill the current clipboard.
- [ ] Self-writes are suppressed by sequence number. Copy-backs add no markers (D-012).

## Size and parsing
- [ ] Sizes are checked before allocating or copying (`GlobalSize`, data length, capped pipe reads).
- [ ] Decoders handle malformed input without panicking (`panic = "abort"` in release).

## Leakage
- [ ] No clip text, preview, file path from a `files` clip, search query or window title in logs, errors, `Debug`, panics or test output.
- [ ] New model types have a manual `Debug` without content, plus a test that asserts it.
- [ ] No logging macros in `capture/`, `image.rs`, `policy.rs`, `sensitive/` and the OS adapters (check scripts enforce this).

## Storage
- [ ] Masked clips: masked preview only, no FTS body, `expires_at` set, never exported, never shown to MCP.
- [ ] Deletion removes the row, FTS, unreferenced blobs, and checkpoints the WAL after bulk deletes.

## Exposure (P2)
- [ ] Scope checks happen in the SQL `WHERE`, not after fetching.
- [ ] No API, CLI command or MCP tool accepts a filesystem path from the caller.
- [ ] Every MCP call writes a content-free `OperationAudit` row.

Record in the task report which items you checked.
