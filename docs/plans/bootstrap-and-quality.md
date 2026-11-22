# Project Bootstrap and Quality-Check Strategy

Status: Phase 0, 2026-11-22.

## 1. Bootstrap sequence (owner + orchestrator)

1. **Owner decides D-013.** Proposed commands (run by the orchestrator once approved):
   ```
   git branch -m master main
   git add -A && git commit -m "docs: Phase 0 blueprint, spikes, agent workflow"
   gh repo create <owner>/dark-clipboard --private --source . --push   # or --public (Q-03)
   ```
   Then protect `main` as Darkshot does (ruleset: no deletion, no force-push, PR required, 0 approvals). Darkshot did this through the GitHub UI; decision 138 there.
2. **Task 001** creates the workspace and activates the cargo gates in `scripts/check.ps1`, `scripts/check.sh` and CI (it removes the `TASK-001` skip blocks).
3. Tasks 002–011 follow `docs/plans/phase-1-foundation.md` via `/next-task`.

## 2. Quality gates

| Gate | Where | What it proves | Active from |
|---|---|---|---|
| `cargo fmt --check` | check scripts, CI | formatting | Task 001 |
| `cargo clippy -D warnings` with `unwrap_used`/`expect_used`/`panic` denied in non-test code | check scripts, CI | no panic paths in release | Task 001 |
| `cargo test --workspace` | check scripts, CI (Windows, macOS, Linux) | behaviour; portability of core crates | Task 001 |
| Banned-crate check via `cargo tree` | check scripts | GC-01 layering | Task 001 |
| No logging macros in privacy modules | check scripts | GC-06 | now (no-op until the files exist) |
| Clipboard read/write only in `capture.rs` | check scripts | GC-03 | now |
| No window-title APIs | check scripts | SR-11 | now |
| Mandatory markers present in `markers.rs` | check scripts | GC-07 | Task 003 |
| JSON Schema validation: 14 valid examples, 10 invalid examples rejected | check scripts (node + ajv) | the data contracts are consistent | **now** |
| Required docs present; relative links resolve | check scripts | the blueprint stays navigable | **now** |
| `cargo deny` (advisories, licences, sources) | CI | supply chain (T-14) | Task 001 |
| Benchmarks (`#[ignore]`) | manual / phase exit | NFR-02 | Task 008 |
| Platform checklists | `platform-verifier` + owner | matrix evidence | Phase 2, 4 |
| Fuzzing | Phase 5 | decoders, parsers | Phase 5 |

The heuristics in the static rules (regexes over source) are deliberately simple, like Darkshot's. They catch accidental violations, not adversarial ones; reviewers remain the main control. A new rule is added whenever a review finds a class of bug that a regex can catch.

## 3. Review policy

- Every task: an independent reviewer (`reviewer`, `security-reviewer` or `specialist`), never the implementing agent.
- High-risk tasks (marked in plans): `opus` reviewer and the `clipboard-privacy-gate` skill checklist.
- Every phase: `/phase-review` (whole-phase, cross-task review) before the exit checklist is ticked.

## 4. Definition of done (task)

Package tests present and passing · check script `ALL CHECKS PASSED` · reviewer `APPROVED` · memory-bank updated · README/CHANGELOG updated if user-visible · merged via PR.
