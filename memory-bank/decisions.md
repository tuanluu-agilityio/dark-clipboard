# Decision log

Do not re-open a settled decision unless the owner asks. Supersede it with a new entry instead.

| ID | Date | Decision | Source | By |
|---|---|---|---|---|
| D-001 | 2026-11-22 | Rust Cargo workspace with portable crates `darkclip-core`, `-platform`, `-storage`, `-app` and cfg-gated adapters `darkclip-windows`, `-macos`, `-linux`; desktop app in `apps/desktop` | system-overview §1 | Phase 0 |
| D-002 | 2026-11-22 | UI framework Slint (conditional); Tauri 2 rejected for P0 on measured memory (160 vs 49 MiB private) and startup (1.7 vs 0.2 s); Tauri remains the fallback | ADR-0001 | Phase 0, owner approval pending (Q-02) |
| D-003 | 2026-11-22 | Monitoring: Win32 listener; macOS changeCount poll 300 ms; X11 XFixes; Wayland ext-data-control → wlr-data-control → unavailable | ADR-0002 | Phase 0 |
| D-004 | 2026-11-22 | Storage: SQLite (rusqlite bundled), WAL, FTS5 contentless with diacritic removal, content-addressed blobs, embedded migrations | ADR-0003 | Phase 0 |
| D-005 | 2026-11-22 | Platform traits plus capability report; adapters never decide policy | ADR-0004 | Phase 0 |
| D-006 | 2026-11-22 | AI/MCP: off by default, stdio only, scoped grants created in the desktop UI, sensitive never grantable, content-free audit | ADR-0005 | Phase 0 |
| D-007 | 2026-11-22 | Process model: desktop is the only clipboard user and DB writer; CLI/MCP read-only, mutations via IPC | ADR-0006 | Phase 0 |
| D-008 | 2026-11-22 | Mandatory markers: Windows Exclude…/CanIncludeInClipboardHistory=0/Clipboard Viewer Ignore; macOS nspasteboard Concealed; Linux x-kde-passwordManagerHint=secret; Darkshot's three-format set. Never configurable | privacy-design §3 | Phase 0 |
| D-009 | 2026-11-22 | Sensitive detection is secondary; default handling `drop`; email/phone off by default | privacy-design §4 | Phase 0 |
| D-010 | 2026-11-22 | Favorites = system collection `favorites` (one mechanism with collections) | data-model §3 | Phase 0 |
| D-011 | 2026-11-22 | Window titles are never captured (schema rejects them; check rule) | threat model T-17 | Phase 0 |
| D-012 | 2026-11-22 | Copy-backs carry no exclusion markers; self-writes are suppressed by sequence number | phase-2 plan | Phase 0 |
| D-013 | 2026-11-22 | Local only for now: rename `master` → `main`, Phase 0 is the first commit, no GitHub remote yet; tasks merge locally with `git merge --no-ff`. Remote + protected `main` later (Q-03 stays open) | bootstrap plan | Owner |
| D-014 | 2026-11-22 | Model policy inherited from Darkshot decision 154: never `fable`; `haiku` never for privacy/concurrency/integrity | CLAUDE.md | Owner (Darkshot) |
| D-016 | 2026-11-22 | `deny.toml` ignores private workspace crates (`[licenses.private] ignore = true`) while the licence is undecided (Q-01) | Task 001 review | Orchestrator |
| D-015 | 2026-11-22 | Task numbering: three digits from 001; Phase 2 uses 1NN, Phase 3 2NN, Phase 4 3NN, Phase 5 4NN | roadmap | Phase 0 |
