# Active context

Updated: 2026-11-22

## Where we are

- **Phase 0 (discovery and architecture): complete**, pending owner approval of the blueprint.
- **Phase 1 Foundation: COMPLETE (2026-11-22).** Tasks 001–011 and 013 merged; phase review 012 done (its one blocker, the bench, was a measurement artifact; fixed and re-measured in 013). Workspace: 263 tests pass, 2 benches ignored (median p95 17.5 ms worst-case corpus, 15.7 ms realistic).
- **Phase 2 Windows capture: IN PROGRESS.** Task packages `docs/tasks/101…108` + `103b` written (D-031). Order: 101 → 105 → 102 → 103 → 103b → 104 → 106 → 107 → 108. Tasks 101 (monitor thread, `ChangeSignal.source`) and 105 (owner → exe name source) done. **Current: Task 102** (`task/102-windows-snapshot-meta`). Task 107 needs the owner at the machine. Real-clipboard-writing tests are `#[ignore]` and not run by agents.
- Git: local only (D-013). Branch `main`; Phase 0 is the first commit. No remote yet: tasks merge locally with `git merge --no-ff`.

## Waiting on owner

| ID | Question | Blocks |
|---|---|---|
| Q-01 | Licence: GPLv3, or permissive app licence + Slint Royalty-free licence (attribution) | Phase 5 Task 409; nothing earlier |
| Q-02 | Approve ADR-0001 (Slint over the preferred Tauri, based on measured memory and startup) | Phase 3 start |
| Q-03 | GitHub repo: public or private, name `dark-clipboard`, protected `main` like Darkshot | Nothing now (local merges per D-013) |
| Q-04 | Test hardware for Phase 4: Mac (macOS 26), Linux with KDE, GNOME and a wlroots compositor | Phase 4 |
| Q-05 | Is GNOME Wayland a must for 0.1.0? (affects whether to build a Shell extension) | Phase 4 scope |

## Follow-ups recorded (not yet scheduled to a task)
- F-08 (Task 101 review nits; resolver-on-restart part fixed in 105): if `post_stop` fails the monitor thread is detached with its listener; `start()` while running silently drops the new `tx`; `Win32_System_Memory` feature only used by test code. Non-Windows build of `darkclip-windows` unverified (no other target installed).
- F-09 (Task 105 review): `app_id` is not authenticated (any process can open the clipboard with another app's HWND or none), so excluded-app matching relies on cooperative apps; markers stay the primary defence. Add to threat-model T-01 residual risk in a later docs task. PID-reuse race between owner lookup and OpenProcess accepted. Handle-leak test slack is 200 because parallel monitor tests open handles. Task 105 OS code was not red-first (TDD lapse).
- F-01 (done in 101): put `source` into `ChangeSignal`, resolved on the monitor thread at OS-signal time (Phase 2 Task 101/105; platform interface change). Closes the dequeue-time race for excluded apps.
- F-02: StorageFull → retention then retry once (system-overview §3) is not implemented.
- F-03: a `sequence()` error at resume leaves the current seq unmarked (possible backfill if a stale signal follows).
- F-04: `upsert_clip` takes `NewClip` by value and cannot wipe the reps on error; consider `&NewClip` or a zeroize-on-drop byte type in core.
- F-05: images are hashed without decoded RGBA and without thumbnails (no `png` decode in app yet); DIB vs PNG copies of one image do not dedupe. Phase 2 Task 103 should decode in the adapter or app.
- F-06: unix-only permission and fsync paths in storage have never run (no Unix machine).
- F-07: search bench p95 ≈ 24–25 ms against a 30 ms budget.

## Top risks (full list: `docs/plans/roadmap.md`)

R-01 GNOME data-control unknown · R-02 password managers without markers · R-03 macOS pasteboard privacy prompt · R-04 Slint latency/IME · R-08 licence.

## Phase 0 evidence

- Windows clipboard spike: `docs/spikes/windows-clipboard-monitor.md` (8 scenarios, all run).
- UI spikes: `docs/spikes/ui-shell-comparison.md` (Tauri 160 MiB private / 7 processes / 1.7 s setup vs Slint 49 MiB / 1 process / 0.2 s).
- macOS and Linux: nothing tested (no hardware in Phase 0).
