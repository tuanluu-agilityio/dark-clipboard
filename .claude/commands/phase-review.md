---
description: Whole-phase review of Dark Clipboard before declaring a phase complete
---

Run this before every release (CLAUDE.md, D-077), not only at the end of a phase.

1. Read `memory-bank/activeContext.md` and the current phase plan in `docs/plans/`.
2. Confirm every task of the phase is `done` in `memory-bank/progress.md`.
3. Dispatch `specialist` (model `opus`, never `fable`) with: "Phase review of <phase>. Read `docs/plans/<plan>.md` (exit checklist and Review Focus), `docs/plans/global-constraints.md`, `docs/security/threat-model.md`. Review `git log` and the code of the phase as a whole for cross-task defects: contract drift between tasks, privacy-gate bypasses, untested Review Focus items, platform claims without evidence. Claims about build configuration, library defaults or runtime behaviour must be backed by a probe or test you ran; otherwise mark them UNVERIFIED and do not base a blocking item on them alone. Read-only. Report APPROVED or CHANGES REQUIRED with blocking items." A re-review after fixes resumes the same agent (SendMessage) with the list of fix commits.
4. Run the quality gate.
5. Tick the exit checklist items that are proven, with evidence. Record the verdict and tokens in `progress.md`, and update `activeContext.md` (phase complete or the remaining blockers).
6. Report to the owner and propose the exact next phase command.
