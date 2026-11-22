# Phase 0 Blueprint (index)

Date: 2026-11-22. This is the approved-spec entry point once the owner approves Phase 0. It contains no new content; it pins which documents form the baseline.

| Area | Baseline document |
|---|---|
| Product | `docs/product/product-vision.md`, `feature-matrix.md`, `mvp-scope.md`, `user-journeys.md` |
| Architecture | `docs/architecture/system-overview.md`, `platform-compatibility.md`, `data-model.md`, `ai-integration.md`, `schemas/`, `examples/` |
| Security | `docs/security/threat-model.md`, `privacy-design.md` |
| Decisions | `docs/adr/0001`–`0006`, `memory-bank/decisions.md` D-001…D-015 |
| Evidence | `docs/spikes/windows-clipboard-monitor.md`, `docs/spikes/ui-shell-comparison.md` |
| Plans | `docs/plans/roadmap.md`, `global-constraints.md`, `bootstrap-and-quality.md`, `phase-1-foundation.md` … `phase-5-p1-release.md`, `docs/tasks/001`–`011` |

## Assumptions made without asking (the brief requested no routine questions)

1. Documentation in English (the brief is in English); conversation language follows the owner (Darkshot uses Vietnamese).
2. Crate prefix `darkclip-`, desktop binary `dark-clipboard`, CLI `darkclip`.
3. Windows-first release; macOS/Linux preview until verified.
4. Default shortcut Ctrl+Shift+V / Cmd+Shift+V (conflicts with "paste as plain text" in some apps; configurable, conflict detection in Task 203).
5. Sensitive clips are dropped by default rather than stored masked.
6. The `fable` model ban from Darkshot applies here too.
7. Data directory is local, not roaming, on Windows.
