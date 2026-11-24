# Task 417: Quality-gate rule for SQLite connections that skip `keep_temp_in_memory`

**Agent:** `builder` · **Model:** sonnet · **Branch:** `task/417-sqlite-connection-gate` (from `main`) · **Review:** `reviewer` (sonnet; low-risk static check, D-077)
**Global Constraints:** GC-06, GC-19. **Source:** Phase 5 retrospective, D-077 item 7.

## Why
Task 416 made every non-test SQLite connection on user data call `db::keep_temp_in_memory` right after it opens, because the bundled SQLite is built with `SQLITE_TEMP_STORE=1`. Nothing enforces this, so a future connection could quietly spill sorts to `%TEMP%`. A check in the gate costs less than relying on a reviewer to remember.

## Requirements
1. **New gate section in both `scripts/check.ps1` and `scripts/check.sh`**, with the same behaviour in each: `== SQLite connections keep temporary data in memory (D-073, D-077)`.
   - Scan non-test Rust code under `crates/` and `apps/` for `Connection::open(`, `Connection::open_with_flags(` and `rusqlite::Connection::open*(`.
   - Skip: files named `tests.rs` or under `tests/`, `#[cfg(test)]` modules (follow the existing gate sections' convention for excluding test code; if a section does this with a per-file allow-list, do the same), and `open_in_memory`.
   - For each hit, the same function must call `keep_temp_in_memory(` within the next 15 lines. If not, the line must carry the marker comment `// temp-store: <reason>` on the line itself or the line before.
   - On failure, print `file:line` and fail the gate.
2. **Self-test:** like the other gate sections, prove the rule fires. Add a fixture under `scripts/` (or a self-test block in the same style the release-script self-tests use) containing one bad open and one good open. The rule must flag only the bad one.
3. Current `main` passes the rule unchanged. If any real hit fails, report it; do not silence it with the marker unless the connection really is exempt.
4. Add a line to `docs/development.md` (or wherever the gate sections are documented; find it with `grep`) describing the rule.

## Known pitfalls (from `memory-bank/systemPatterns.md`)
- Windows-only code must lint on macOS/Linux: `check.sh` runs on macOS and Ubuntu CI, so use POSIX tools only (`grep -E`, `awk`). No GNU-only flags; macOS has BSD `sed` and `grep`.
- PowerShell scripts must not leak `$LASTEXITCODE`. End the self-test paths explicitly.
- Every new SQLite connection on user data calls `db::keep_temp_in_memory` first.

## Done when
- The self-test shows the rule flags the bad fixture and passes the good one.
- `powershell -File scripts/check.ps1` prints `ALL CHECKS PASSED`.
- CI passes on windows, macos and ubuntu.
