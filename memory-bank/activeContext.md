# Active context

Updated: 2026-11-22

## Where we are

- **Phase 0 (discovery and architecture): complete**, pending owner approval of the blueprint.
- **Current phase:** Phase 1 Foundation (`docs/plans/phase-1-foundation.md`). Task 001 done (merged 2026-11-22). Tasks 001–007 done. **In progress: 008 and 009 in parallel worktrees.** Then 010 (capture actor), 011, 012 phase review.
- Git: local only (D-013). Branch `main`; Phase 0 is the first commit. No remote yet: tasks merge locally with `git merge --no-ff`.

## Waiting on owner

| ID | Question | Blocks |
|---|---|---|
| Q-01 | Licence: GPLv3, or permissive app licence + Slint Royalty-free licence (attribution) | Phase 5 Task 409; nothing earlier |
| Q-02 | Approve ADR-0001 (Slint over the preferred Tauri, based on measured memory and startup) | Phase 3 start |
| Q-03 | GitHub repo: public or private, name `dark-clipboard`, protected `main` like Darkshot | Nothing now (local merges per D-013) |
| Q-04 | Test hardware for Phase 4: Mac (macOS 26), Linux with KDE, GNOME and a wlroots compositor | Phase 4 |
| Q-05 | Is GNOME Wayland a must for 0.1.0? (affects whether to build a Shell extension) | Phase 4 scope |

## Top risks (full list: `docs/plans/roadmap.md`)

R-01 GNOME data-control unknown · R-02 password managers without markers · R-03 macOS pasteboard privacy prompt · R-04 Slint latency/IME · R-08 licence.

## Phase 0 evidence

- Windows clipboard spike: `docs/spikes/windows-clipboard-monitor.md` (8 scenarios, all run).
- UI spikes: `docs/spikes/ui-shell-comparison.md` (Tauri 160 MiB private / 7 processes / 1.7 s setup vs Slint 49 MiB / 1 process / 0.2 s).
- macOS and Linux: nothing tested (no hardware in Phase 0).
