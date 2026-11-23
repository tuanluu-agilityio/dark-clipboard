# Task 406: Fuzzing of decoders and parsers

**Agent:** `specialist` · **Model:** opus · **Branch:** `task/406-fuzzing` · **Review:** `security-reviewer` (opus)
**Global Constraints:** GC-06, GC-09, GC-10, GC-16. **Plan:** Phase 5 Task 406; Review Focus 5.

## Requirements
1. `fuzz/` (cargo-fuzz, excluded from the workspace) targets: DIB → RGBA, PNG decode with the core limits, DROPFILES parser, CF_HTML header parser, sensitive scanners, tag normaliser and transforms (404), backup/import entry validation (405).
2. If nightly + libFuzzer cannot run on this Windows machine, use a stable deterministic fallback (a `#[ignore]` test harness with a seeded generator and structure-aware mutations, ≥ 1,000,000 iterations per target) and say so; never claim fuzzing that did not run.
3. Run each target ≥ 1 h (or the fallback budget); record command, iterations and result in `docs/testing/fuzzing.md`; minimised crashers become regression tests; small synthetic seed corpora committed.
4. Fix findings (bounded allocation, no panic) in the touched parsers; each fix gets a regression test.

## Not doing
Fuzzing in CI on every PR (document the manual command instead).
