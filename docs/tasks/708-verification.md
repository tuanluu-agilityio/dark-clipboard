# Task 708: Verification (P1-P6 measurements and the Phase 7 demo checklist)

**Agent:** `platform-verifier` · **Model:** sonnet · **Branch:** `task/708-verification` (from `main`, after 707) · **Review:** `reviewer` (sonnet) · **Risk:** Low
**Global Constraints:** GC-06, GC-18, **GC-19**. **Source:** `docs/plans/phase-7-ui-redesign.md` (row 708, Review Focus 5), `docs/specs/ui-redesign/05-acceptance.md` section 2.
Scope: measure and document. **Never edit product code**; if a number misses its budget, record it as failed and stop there (the orchestrator decides).

## Read first
`CLAUDE.md`, `05-acceptance.md` sections 2 and 4, `docs/testing/ui-redesign-baseline.md` (method and Task 700 numbers), `docs/tasks/212-renderer-memory.md` item 6, `docs/testing/perf-windows.md` (P1 21.1 ms, 6.9 MiB hidden, `scripts/measure-popup.ps1`), `docs/testing/platform-checklist.md` (all Phase 7 sections).

## Requirements
Method for every run: `cargo build --release -p dark-clipboard`; fresh temp `--data-dir` with a BOM-less `settings.json` (`onboarding_done: true`, default policy, `capture_enabled: false`: the real clipboard is never read); `--seed-demo-clips 10000`; app started with `--no-global-shortcut --exit-after-ms <ms>`; show/hide only with `--toggle` over IPC; `--quit`; delete the dir. Build the 0.1.x baseline exe from the commit before Task 700 (scratch worktree) and measure it in the same session on the same machine. Scripts are throwaway files in the scratchpad, not the repo (except the existing `scripts/measure-popup.ps1`, unchanged).
1. **P1** popup open p95: `powershell -File scripts/measure-popup.ps1 -Trials 50 -SeedClips 10000`, 3 runs, baseline (21.1 ms) and current. Budget <= 30 ms. If 0 `perf` events appear (the software renderer has no rendering notifier, see the Task 212 note in `perf-windows.md`), use the timing source Task 212 added; if none works, mark P1 "not measurable" and make it an owner row; never estimate.
2. **P2** search p95: only if `perf what=search_done` can be produced without typing; otherwise "not measurable (needs input), owner row".
3. **P3** private bytes hidden at t = 8 s, 3 runs (budget <= 9 MiB). Compare against the baseline exe of the same session, not across documents (the 700 session saw 9.7-10.0 MB with 10k clips).
4. **P4** private bytes after one popup show + hide, 3 runs (budget baseline + 4 MiB). History and settings cannot be opened without input: owner row.
5. **P5** idle CPU hidden, 60 s (`Process.TotalProcessorTime` delta over wall time), budget 0 %.
6. **P6** release exe size in bytes against the baseline exe and Task 700's 13,142,528 (budget <= baseline + 300 KiB).
7. **Record** in new `docs/testing/ui-redesign-verification.md`: date, machine, commit hashes, exact commands, every raw number (all runs), means, and a table budget / baseline / Task 700 / now / **pass or fail** / limits. Failed or unmeasurable items are stated plainly.
8. **Demo checklist**: in `docs/testing/platform-checklist.md` add one section "Phase 7 demo checklist" (before `## macOS`) with the most important manual checks first, in this order: popup (open, search, keys, row menu, preview), history, settings (every setting saves, Always protected inert), app icon (taskbar, tray, Alt+Tab), Vietnamese, DPI (100/150/200 % and mixed), Narrator and Tab walk, then the P2 / P4 (history, settings) / P8 owner measurements. Each row names the task section it condenses; **no existing row is deleted or edited**; all rows **untested**.

## Files this task may edit
`docs/testing/ui-redesign-verification.md` (new), `docs/testing/platform-checklist.md` (one new section), `CHANGELOG.md` only if owner-facing text needs it (normally not).

## Known pitfalls
- Same machine, same session, same script for both builds; run-to-run spread is about 0.5 MiB: say so before calling a difference real.
- `--headless` ignores `--quit`; use the windowed start with `--exit-after-ms`, and kill leftovers by PID only for processes you started.
- Settings file without BOM and `capture_enabled: false`; check for `settings.json.corrupt*` after each run.
- Never record clip content; demo clips are synthetic. No screenshots, input simulation or real-clipboard writes (GC-19).

## Acceptance
Every P1-P6 has raw numbers or an explicit "not measurable" with the reason; commands reproducible from the doc; checklist section present and existing rows intact; `powershell -File scripts/check.ps1` prints `ALL CHECKS PASSED`.

## Not doing
Fixing a missed budget, product code, new scripts in the repo, owner-only measurements.
