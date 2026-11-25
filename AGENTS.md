# AGENTS.md

Instructions for any AI coding agent (Claude Code, Codex, others) working in this repository. Claude Code also reads `CLAUDE.md`, which is the authoritative rule set; this file summarizes it for other tools.

## Before you change anything

1. Read `memory-bank/activeContext.md` and `memory-bank/progress.md`.
2. Find your task package in `docs/tasks/`. If you have none, you are the orchestrator: follow `.claude/commands/next-task.md`.
3. Read only the files the package lists under "Read first".

## Hard rules

- Edit only the files listed in your task package. Anything else must be reported with a reason.
- Tests first (TDD). Never weaken, skip or `#[ignore]` a test to make it pass, except the tests the package itself marks `#[ignore]` (manual or benchmark tests).
- `darkclip-core`, `-platform`, `-storage`, `-app` stay free of OS, UI and async crates (`scripts/check.ps1` enforces this).
- Clipboard content never appears in logs, errors, `Debug` output or test output.
- OS and Darkshot privacy markers are mandatory and never configurable.
- Only the capture actor in `crates/darkclip-app/src/capture/` reads or writes the OS clipboard.
- No network access, no telemetry.
- Run `cargo fmt --all`, then `scripts/check.ps1` (Windows) or `scripts/check.sh` (macOS/Linux). Report done only when it prints `ALL CHECKS PASSED`.
- UI interaction changes in `apps/desktop` need a headless pointer or keyboard test (`apps/desktop/tests/menu_click.rs` pattern, GC-19: agents cannot click). UI design reference: `docs/specs/ui-redesign/`.
- Commit on your task branch with a short English message (`feat:`, `fix:`, `test:`, `docs:`, `chore:`). Do not create, merge or push branches unless you are the orchestrator.
- Do not invent results. Report the exact commands you ran and their real output.

## Roles

| Agent | File | Use for |
|---|---|---|
| builder | `.claude/agents/builder.md` | Well-specified Rust tasks |
| specialist | `.claude/agents/specialist.md` | FFI/unsafe, platform adapters, concurrency, storage integrity, privacy-critical code |
| scaffolder | `.claude/agents/scaffolder.md` | Mechanical work: workspace, config, docs |
| reviewer | `.claude/agents/reviewer.md` | Read-only review of a task branch |
| security-reviewer | `.claude/agents/security-reviewer.md` | Read-only review of privacy- and security-sensitive branches |
| platform-verifier | `.claude/agents/platform-verifier.md` | Runs and records platform checklists and measurements; never edits product code |
