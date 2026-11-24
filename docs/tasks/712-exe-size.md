# Task 712: Release exe size after Phase 7 (F-39)

**Agent:** `builder` · **Model:** sonnet · **Branch:** `task/712-exe-size` (from `main`) · **Review:** `reviewer` (sonnet)
**Global Constraints:** GC-14, GC-18, GC-19. **Source:** owner request 2026-11-25 ("xử lý F-39"); D-086 (P6 missed: release exe 14,079,488 B vs 13,105,664 B at v0.1.2, +951 KiB; budget +300 KiB; Task 700 alone was +36 KiB).

## Goal
Find what made the release exe grow in Tasks 701–707 and bring the growth vs v0.1.2 back to **≤ +300 KiB** without any behaviour, look or accessibility change. If that is impossible without a behaviour change, stop at the largest safe reduction and document the root cause with numbers.

## Facts
- `[profile.release]` already has `opt-level = "z"`, `lto = true`, `codegen-units = 1`, `panic = "abort"`, `strip = true`. Do not change the profile for the workspace without measuring; never turn off `panic = "abort"` or `strip`.
- `apps/desktop/ui/kit/gallery.slint` exports `KitGallery` (re-exported from `ui/app.slint`) only to force compilation of kit components that were unused in Task 700. All kit components are now used by real windows, so the gallery may be dead generated code. Check, then remove it if nothing needs it (tests that scan it must be updated to scan the real windows instead, keeping their coverage).
- Other suspects: duplicated large inline components instantiated in many places (Slint generates code per sub-component), large `for` bodies, very long ternary chains (e.g. `Icon` name → `@image-url` chain), images embedded more than once, string tables.

## Steps
1. Measure: release-build the exe at these `main` merge commits (one scratch worktree outside the repo, reused, removed afterwards; `--target-dir` per build is not needed, sizes only): v0.1.2 (`558f81f`), Task 700 merge, 710, 701, 702, 703, 705, 704, 706, 707 (use `git log --merges --oneline` to find them). Record the size of each in a table: which task added how much.
2. For the biggest contributors, find the cause (e.g. compare the size of the generated Slint code in `target/release/build/dark-clipboard-*/out/*.rs` between commits; try removing `KitGallery`; try `@image-url` image count). No new tools or crates; `cargo bloat` is not installed and must not be installed.
3. Apply the safe fixes (no visible or behavioural change; all existing tests keep passing; scans keep their coverage). Measure again.
4. Record before/after numbers and commands in `docs/testing/ui-redesign-verification.md` (new section "Task 712: exe size"), a decision in `memory-bank/decisions.md` (next free number after D-086), and update `CHANGELOG.md` `[Unreleased]` Known issues (remove the size line if fixed, or update the number).

## Files this task may edit
`apps/desktop/ui/**`, `apps/desktop/src/**` (only for size fixes with no behaviour change), `apps/desktop/build.rs`, `apps/desktop/tests/**` (keep coverage), `docs/testing/ui-redesign-verification.md`, `memory-bank/decisions.md`, `CHANGELOG.md`. `Cargo.toml` profile only with before/after numbers and a decision.

## Acceptance
Release exe growth vs v0.1.2 ≤ +300 KiB, or the documented maximum safe reduction with root cause; `cargo fmt --all`, then `powershell -File scripts/check.ps1` prints `ALL CHECKS PASSED`; no test weakened.

## Known pitfalls (from `memory-bank/systemPatterns.md`)
- Slint skips components no window instantiates; a kit component only used in the gallery would silently stop being compiled: check every kit component is used by a real window before removing the gallery.
- `#[path]` modules in integration tests only at the top level (F-37).
- Windows-only code must lint on macOS/Linux (CI clippy `-D warnings`).
- Never run the app with a visible window except with `--data-dir <temp>` and `--exit-after-ms` (GC-19).
