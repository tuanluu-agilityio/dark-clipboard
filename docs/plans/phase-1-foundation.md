# Phase 1: Foundation Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Run each task through `/next-task`. Steps in the task packages use checkbox (`- [ ]`) syntax.

**Goal:** An OS-independent, fully tested Rust workspace: domain core, privacy policy, sensitive detection, storage with FTS search and retention, a platform abstraction with a fake, and the capture actor and Application API running headless against the fake.

**Architecture:** Cargo workspace with layered crates (`docs/architecture/system-overview.md` §1). No OS adapter, no UI in this phase. Everything is driven by `FakePlatform` so that CI on Windows, macOS and Linux proves portability.

**Tech Stack:** Rust stable (1.100+), edition 2021, `serde`/`serde_json`, `sha2`, `thiserror`, `uuid` (v7), `rusqlite` (bundled), `png`.

**Spec:** `docs/product/mvp-scope.md`, `docs/architecture/system-overview.md`, `docs/architecture/data-model.md`, `docs/security/privacy-design.md`, ADR-0002…0004, ADR-0006.

## Global Constraints

`docs/plans/global-constraints.md` (GC-01…GC-18). Every task package repeats the IDs that matter most to it.

## Review Focus

Failure modes that no single task's happy-path tests cover. The owning task's package contains the pinning test.

1. **Burst of changes with identical sequence numbers** (Windows S5): the actor must store one clip, not N. Test `burst_same_seq_stores_once` → Task 010.
2. **OLE three-phase writes** (formats appear only on the 2nd signal; the owner disappears on the 3rd): the clip is stored once, with the source from the first signal. Test `ole_three_phase_keeps_first_source` → Task 010.
3. **Pause ends while the same content is still on the clipboard:** nothing is captured until the next change. Test `resume_does_not_backfill` → Task 010.
4. **Vietnamese search without accents** ("tieng viet" finds "Tiếng Việt") and a 1–2 character query: substring fallback returns results. Tests `search_diacritic_insensitive`, `search_short_query_fallback` → Task 008.
5. **Deleting a clip whose image blob is shared with another clip** keeps the blob. Test `delete_keeps_shared_blob` → Task 009.

## Task table

Branch base for all tasks: `main` (protected; merge via PR, see `CLAUDE.md`). Numbers are three-digit and start at 001. Check `git branch -r` before creating a new number.

| Task | Package | Depends on | Agent | Model (why) | Reviewer | Risk |
|---|---|---|---|---|---|---|
| 001 | `docs/tasks/001-workspace-bootstrap.md` | — | scaffolder | sonnet (many files, check script logic) | reviewer (sonnet) | Low |
| 002 | `docs/tasks/002-core-model-i18n.md` | 001 | builder | sonnet | reviewer (sonnet) | Low |
| 003 | `docs/tasks/003-core-policy.md` | 002 | specialist | opus (privacy-critical) | security-reviewer (opus) | **High** |
| 004 | `docs/tasks/004-core-classify-hash.md` | 002 | builder | sonnet | reviewer (sonnet) | Medium (dedupe correctness) |
| 005 | `docs/tasks/005-core-sensitive.md` | 002 | specialist | opus (detection quality, wiping) | security-reviewer (opus) | **High** |
| 006 | `docs/tasks/006-platform-traits-fake.md` | 002 | builder | sonnet | specialist (opus; contract for 3 adapters) | Medium |
| 007 | `docs/tasks/007-storage-schema.md` | 004 | specialist | opus (data integrity, migrations) | reviewer (opus) | **High** |
| 008 | `docs/tasks/008-storage-search.md` | 007 | builder | sonnet | reviewer (sonnet) | Medium |
| 009 | `docs/tasks/009-storage-retention.md` | 007 | builder | sonnet | reviewer (opus; deletion) | Medium |
| 010 | `docs/tasks/010-app-capture-actor.md` | 003, 005, 006, 007 | specialist | opus (concurrency, privacy gate) | security-reviewer (opus) | **High** |
| 011 | `docs/tasks/011-app-api-settings.md` | 008, 009, 010 | builder | sonnet | reviewer (sonnet) | Medium |
| 012 | (phase review) | 001–011 | reviewer | opus | — | — |

Parallelism: after 002, the tasks 003, 004, 005, 006 are independent. After 007, the tasks 008 and 009 are independent.

Model policy: never `fable` (inherited from Darkshot decision 154). `haiku` is never used for privacy, concurrency or data-integrity tasks.

## Phase exit checklist

- [ ] `scripts/check.ps1` prints `ALL CHECKS PASSED` on Windows; CI (`.github/workflows/ci.yml`) green on `windows-latest`, `macos-latest`, `ubuntu-latest`.
- [ ] `cargo test -p darkclip-app` runs the full capture pipeline against `FakePlatform`, including all Review Focus tests.
- [ ] Search bench: 10,000 synthetic clips, p95 ≤ 30 ms per query (printed by `cargo test -p darkclip-storage --release -- --ignored bench_search`).
- [ ] No crate in GC-01's list depends on a banned crate.
- [ ] Phase review (Task 012) verdict APPROVED; memory-bank updated.
