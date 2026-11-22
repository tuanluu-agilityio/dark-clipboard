# Active context

Updated: 2026-11-22

## Where we are

- **Phase 0 (discovery and architecture): complete**, pending owner approval of the blueprint.
- **Phase 1 Foundation: COMPLETE (2026-11-22).** Tasks 001–011 and 013 merged; phase review 012 done (its one blocker, the bench, was a measurement artifact; fixed and re-measured in 013). Workspace: 263 tests pass, 2 benches ignored (median p95 17.5 ms worst-case corpus, 15.7 ms realistic).
- **Phase 2 Windows capture: IN PROGRESS.** Task packages `docs/tasks/101…108` + `103b` written (D-031). Order: 101 → 105 → 102 → 103 → 103b → 104 → 106 → 107 → 108. Tasks 101 (monitor thread, `ChangeSignal.source`), 105 (owner → exe name source) and 102 (`snapshot_meta`: formats + markers by id) 103 (`read` with caps; DIB→PNG; own DROPFILES parser) 103b (PNG decode → pixel hash, thumbnails, bomb caps) and 104 (copy-back write; post-close seq + owner check; per-write owner window, D-038) done. 106 (`dark-clipboard --headless`, content-free log, StorageFull retry, caps Native) done. 107 (Windows verification with the owner: Darkshot excluded by markers, KeePassXC not stored; Chrome/Explorer/Snipping Tool/Notepad captured; 10/10 real-clipboard tests) done. **Next: Task 109** (F-14: keep the source across an ownerless OLE follow-up seq), then 108 phase review. CI on GitHub (Windows, Linux, macOS, deny) green since the first push: the Unix permission/fsync paths now run in CI tests (F-06 partly closed). Real-clipboard-writing tests are `#[ignore]` and not run by agents.
- Git: remote `origin` = `github.com/tuanluu-agilityio/dark-clipboard` (public), pushed 2026-11-22 as is (owner, D-035). From Task 103b on, tasks merge via PR (`gh pr create` / `gh pr merge --merge --delete-branch`).

## Waiting on owner

| ID | Question | Blocks |
|---|---|---|
| Q-04 | Test hardware for Phase 4 (Mac, Linux KDE/GNOME/wlroots): **none yet, test later** (owner 2026-11-22). Phase 4 adapters can be built but every row stays `untested` | Phase 4 verification |

Q-07 done by the owner 2026-11-22: ruleset "Protect main" (no deletion, no force-push, PR with merge only, required checks windows/macos/ubuntu/deny). Answered 2026-11-22 (D-034): Q-01 GPLv3 · Q-02 Slint approved (ADR-0001 accepted) · Q-03 public repo exists · Q-05 left to the orchestrator: GNOME Wayland is **not** a must for 0.1.0.

## Follow-ups recorded (not yet scheduled to a task)
- F-08 (Task 101 review nits; resolver-on-restart part fixed in 105): if `post_stop` fails the monitor thread is detached with its listener; `start()` while running silently drops the new `tx`; `Win32_System_Memory` feature only used by test code. Non-Windows build of `darkclip-windows` unverified (no other target installed).
- F-09 (Task 105 review): `app_id` is not authenticated (any process can open the clipboard with another app's HWND or none), so excluded-app matching relies on cooperative apps; markers stay the primary defence. Add to threat-model T-01 residual risk in a later docs task. PID-reuse race between owner lookup and OpenProcess accepted. Handle-leak test slack is 200 because parallel monitor tests open handles. Task 105 OS code was not red-first (TDD lapse).
- F-10 (Task 102 review): Windows `size_hint` is always None (handle size needs GetClipboardData), so image caps apply only at read time (103). A marker DWORD with GlobalSize ≠ 4 → None → fails closed (over-skips); watch in 107. `caps.concealed_markers` flips to Native in 106.
- F-11 (Task 103 review): only the single best image format is planned (S-1); if it is an invalid PNG or unsupported DIB the image is lost even when another image format was offered (no fallback). Repeated-masks detection matches on content (3 pixels could be lost in a pathological image). `encode_png` output keeps worst-case capacity (memory only). Win32 read paths have never run on a real clipboard (3 ignored tests for Task 107, plus a manual Explorer file copy).
- F-12 (Task 104): `caps.write` still Unsupported in `lib.rs` (flip in 106). Unverified on a real clipboard: destroying the owner window does not bump the seq; the 300 ms no-extra-signal assert can fail if any app copies meanwhile. Manual: copy-back then copy in Notepad immediately (no delay, no recapture).
- F-13 (Task 106): real Ctrl+C in a console untested (owner); log-rotation reopen failure path untested; S-10 word list may false-positive on future harmless lines (safe direction). Idle measurement included owner copies; scratch data dirs deleted and verified.
- F-14 (Task 107): OLE writers with a flush (Notepad, PowerShell) bump the seq per phase and the final phase has no owner, so the clip is stored with source None and (privacy gap) an unmarked excluded app writing this way would not match the excluded list. Fix in Task 109 (actor; security review). Untested rows: Bitwarden, 1Password, Office, RDP, elevated source, manual Win+V/copy-back stall check (Phase 3 UI), real console Ctrl+C.
- F-01 (done in 101): put `source` into `ChangeSignal`, resolved on the monitor thread at OS-signal time (Phase 2 Task 101/105; platform interface change). Closes the dequeue-time race for excluded apps.
- F-02: StorageFull → retention then retry once (system-overview §3) is not implemented.
- F-03: a `sequence()` error at resume leaves the current seq unmarked (possible backfill if a stale signal follows).
- F-04 (done in 106): `upsert_clip` took `NewClip` by value and cannot wipe the reps on error; consider `&NewClip` or a zeroize-on-drop byte type in core.
- F-05: images are hashed without decoded RGBA and without thumbnails (no `png` decode in app yet); DIB vs PNG copies of one image do not dedupe. Phase 2 Task 103 should decode in the adapter or app.
- F-06: unix-only permission and fsync paths in storage have never run (no Unix machine).
- F-07: search bench p95 ≈ 24–25 ms against a 30 ms budget.

## Top risks (full list: `docs/plans/roadmap.md`)

R-01 GNOME data-control unknown · R-02 password managers without markers (mitigated for KeePassXC 2.8.0 on Windows: built-in exclusion + markers seen; re-verify with skip-reason output; open for Bitwarden/1Password) · R-03 macOS pasteboard privacy prompt · R-04 Slint latency/IME · R-08 licence.

## Phase 0 evidence

- Windows clipboard spike: `docs/spikes/windows-clipboard-monitor.md` (8 scenarios, all run).
- UI spikes: `docs/spikes/ui-shell-comparison.md` (Tauri 160 MiB private / 7 processes / 1.7 s setup vs Slint 49 MiB / 1 process / 0.2 s).
- macOS and Linux: nothing tested (no hardware in Phase 0).
