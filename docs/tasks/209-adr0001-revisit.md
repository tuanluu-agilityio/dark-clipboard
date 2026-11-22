# Task 209: ADR-0001 revisit check (measurements)

**Agent:** `platform-verifier` · **Model:** sonnet · **Branch:** `task/209-adr0001-revisit` · **Review:** `reviewer` (opus)
**Plan:** Phase 3 Task 209; NFR-01, NFR-04, NFR-05; ADR-0001 revisit triggers.

## Requirements
Measure on this machine, release build, `--data-dir <tmp>` with `--seed-demo-clips 10000`, and record raw numbers, commands and date in `docs/testing/perf-windows.md`:
1. Idle CPU over 10 min with the popup hidden (NFR-04).
2. Private bytes hidden and after 20 popup shows (NFR-05 ≤ 40 MiB target; the Slint spike measured 49 MiB, so report the gap honestly).
3. Popup latency via `--toggle` over IPC, 50 trials (NFR-01), using `scripts/measure-popup.ps1`.
4. Search latency in the UI path (keystroke → results model updated) on 10k clips.
5. Narrator smoke: list which controls announce names (manual-ish, read-only).
Any ADR-0001 revisit trigger hit → report it; the orchestrator decides (no silent pass). Never write the real clipboard; never simulate input; delete the temp data dir afterwards.
