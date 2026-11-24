# Active context

Updated: 2026-11-23

## Where we are

- **Phase 0 (discovery and architecture): complete**, pending owner approval of the blueprint.
- **Phase 1 Foundation: COMPLETE (2026-11-22).** Tasks 001–011 and 013 merged; phase review 012 done (its one blocker, the bench, was a measurement artifact; fixed and re-measured in 013). Workspace: 263 tests pass, 2 benches ignored (median p95 17.5 ms worst-case corpus, 15.7 ms realistic).
- **Phase 2 Windows capture: COMPLETE (2026-11-23).** Tasks 101–107, 103b, 109, 110 merged (PRs #1–#6); phase review 108 → fixes 110 (D-043), security-reviewer APPROVED. Real capture on Windows works headless (`dark-clipboard --headless`); markers verified with Darkshot 0.6.0 (end to end) and KeePassXC 2.8.0 (not stored); Bitwarden/1Password not available on the test machine (R-02 open for them). Real-clipboard rechecks pending for F-14 (Notepad) and the Task 110 image fixes (4K DIB, over-cap copy-back): owner manual rows in Phase 3.
- **Phase 3 Desktop shell (Slint): COMPLETE (2026-11-23).** Tasks 200–213 merged (PRs #7–#20): capture/ module split, Slint app (single instance + per-session pipe IPC), tray, global shortcut, popup search (filters, highlight, undo delete), opt-in paste back (T-19 checks), previews + detail pane, history window, settings window, first-run welcome (capture off until Start), start at login, software renderer default (7 MiB hidden, 0 % idle CPU, popup p95 18 ms), settings BOM fix. Windows alpha docs + unsigned artifact SHA-256 in `docs/testing/alpha-artifact.md` (version 0.0.1; no tag, no release). **Every desktop UI row in `docs/testing/platform-checklist.md` is still untested by hand (GC-19: agents cannot click, type or screenshot).**
- **Phase 5 (P1 + hardening, Windows scope): IN PROGRESS (started 2026-11-23, D-057).** Phase 4 deferred (no hardware, Q-04). Order: 401 → 402 → 404 → 411 → 403 → 405 → 406 → 412 → 407 (`docs/plans/phase-5-p1-release.md` Execution). Done: 401, 402, 404, 411, 403, 405, 406, 412, 407 (`docs/security/review-0.1.0.md`: 1 high, 4 medium). 413 (release blockers) done. **Next: Task 408 release workflow**, then 410, then the release per D-068. Then 408 release workflow, 410 0.1.0 docs and the release itself (owner authorized releases and public tags, D-058). The owner can test the Windows alpha meanwhile (`docs/user/windows-alpha.md`).
- Git: remote `origin` = `github.com/tuanluu-agilityio/dark-clipboard` (public), pushed 2026-11-22 as is (owner, D-035). From Task 103b on, tasks merge via PR (`gh pr create` / `gh pr merge --merge --delete-branch`).

## Waiting on owner

| ID | Question | Blocks |
|---|---|---|
| Q-04 | Test hardware for Phase 4 (Mac, Linux KDE/GNOME/wlroots): **none yet, test later** (owner 2026-11-22). Phase 4 adapters can be built but every row stays `untested` | Phase 4 verification |

Q-07 done by the owner 2026-11-22: ruleset "Protect main" (no deletion, no force-push, PR with merge only, required checks windows/macos/ubuntu/deny). Answered 2026-11-22 (D-034): Q-01 GPLv3 (applied in Task 201, D-045) · Q-02 Slint approved (ADR-0001 accepted) · Q-03 public repo exists · Q-05 left to the orchestrator: GNOME Wayland is **not** a must for 0.1.0.

## Follow-ups recorded (not yet scheduled to a task)
- F-08 (Task 101 review nits; resolver-on-restart part fixed in 105): if `post_stop` fails the monitor thread is detached with its listener; `start()` while running silently drops the new `tx`; `Win32_System_Memory` feature only used by test code. Non-Windows build of `darkclip-windows` unverified (no other target installed).
- F-09 (Task 105 review): `app_id` is not authenticated (any process can open the clipboard with another app's HWND or none), so excluded-app matching relies on cooperative apps; markers stay the primary defence. Add to threat-model T-01 residual risk in a later docs task. PID-reuse race between owner lookup and OpenProcess accepted. Handle-leak test slack is 200 because parallel monitor tests open handles. Task 105 OS code was not red-first (TDD lapse).
- F-10 (Task 102 review): Windows `size_hint` is always None (handle size needs GetClipboardData), so image caps apply only at read time (103). A marker DWORD with GlobalSize ≠ 4 → None → fails closed (over-skips); watch in 107. `caps.concealed_markers` flips to Native in 106.
- F-11 (Task 103 review): only the single best image format is planned (S-1); if it is an invalid PNG or unsupported DIB the image is lost even when another image format was offered (no fallback). Repeated-masks detection matches on content (3 pixels could be lost in a pathological image). `encode_png` output keeps worst-case capacity (memory only). Win32 read paths have never run on a real clipboard (3 ignored tests for Task 107, plus a manual Explorer file copy).
- F-12 (Task 104): `caps.write` still Unsupported in `lib.rs` (flip in 106). Unverified on a real clipboard: destroying the owner window does not bump the seq; the 300 ms no-extra-signal assert can fail if any app copies meanwhile. Manual: copy-back then copy in Notepad immediately (no delay, no recapture).
- F-13 (Task 106): real Ctrl+C in a console untested (owner); log-rotation reopen failure path untested; S-10 word list may false-positive on future harmless lines (safe direction). Idle measurement included owner copies; scratch data dirs deleted and verified.
- F-14 (Task 107; fixed in 109, D-042; real-clipboard re-check with Notepad/PowerShell pending in Phase 3 manual tests): OLE writers with a flush (Notepad, PowerShell) bump the seq per phase and the final phase has no owner, so the clip is stored with source None and (privacy gap) an unmarked excluded app writing this way would not match the excluded list. Fix in Task 109 (actor; security review). Untested rows: Bitwarden, 1Password, Office, RDP, elevated source, manual Win+V/copy-back stall check (Phase 3 UI), real console Ctrl+C.
- F-15 (Task 200 review): the GC-03 call-pattern grep only matches variables named access/acc/clipboard_access; real calls go through retry closures (`|a| a.read(..)`), so the `ClipboardAccess`/`.access` type rule is what confines them. Consider a closure-aware pattern in a later hardening.
- F-16 (CI, Task 200 PR): `excluded_app_ole_flush_without_markers_is_skipped` flaked on Ubuntu: the shared ownerless sentinel could land inside D-042 rule 3's 500 ms window (signal vs actor clock) and be skipped. Fixed by the orchestrator: the sentinel helper is owned (`sentinel.exe`); test-only, no product change. Watch other timing-based capture tests on CI.
- F-17 (Task 202 review, for Phase 6): `set_capture_mode` does not range-check `Paused { until }`; a future CLI/MCP caller passing a value past FAR_FUTURE would apply the mode but fail the save with Internal. Clamp or return InvalidArgument when CLI/MCP arrive. Explorer restart (TaskbarCreated) does not re-add the tray icon yet.
- F-18 (Task 207 review nits, for Task 210 cleanup): built-in removal confirmation text says copies will be stored again, which is wrong for the Darkshot entry (its markers still exclude it); excluded-app ids from a hand-edited settings file are listed without bidi replacement.
- F-19 (CI, Task 208 PR): `fresh_install_does_not_capture_until_start` flaked on macOS: the policy update reaches the actor asynchronously, so a copy right after Start can be dropped (never backfilled). Test now re-copies until captured (assertions unchanged); orchestrator fix. Also a non-Windows dead_code error in `autostart.rs` fixed with `cfg_attr(not(windows), allow(dead_code))`.
- F-20 (done in 411, D-062): contentless FTS kept tokens of deleted rows until segment merge. Nits left: user-delete/masked merges do not reset the pending count (one redundant merge later); a failed merge drops the pending count (best effort).
- F-21 (Task 401; re-run in 407): search bench p95 over 30 ms on a busy machine (407: worst-case 29.1–32.0 ms in 5 runs, realistic 22.2–30.4 ms; CPU 26–71 % busy); still needs an idle run and the in-app `perf what=search_done` timing (owner row). Listed as a known issue for 0.1.0.
- F-22 (Task 403 review nits): with a picker dialog open, typed letters go to the hidden search box; the note loads the whole clip on each selection; an open Tags dialog keeps the tag list it started with (a change from another window is overwritten). All organization UI rows in the checklist are owner-untested.
- F-23 (Task 405 review nits): the actor's post-message drain is unbounded (cap it, e.g. at the queue length seen at entry); a restored `sqlite_stat1` keeps attacker-chosen planner stats (no effect on results); the manifest `clip_count` is not cross-checked; a restored DB is not re-scanned for sensitive content (documented). All Task 405 checklist rows (file dialogs are compiled but never run) are owner-untested.
- F-24 (Task 406): libFuzzer targets in `fuzz/` never built or run (no nightly); owner may run them (`docs/testing/fuzzing.md`) and report the `png` 0.18.1 palette panic upstream (input `fuzz/corpus/png_to_dibs/seed-06`).
- F-25 (Task 413 review nits): after an elevated first run the data folder is owned by Administrators, so later normal runs log `data_dir_acl_failed result=not_owner` although the DACL is private (log only); `created_now` is decided before `create_dir_all` (negligible); a default folder that is a junction is followed. The owner's machine has `HKCU\...\Windows Error Reporting\ExcludedApplications\dark-clipboard.exe=1` from a test run (same value the app writes).
- F-01 (done in 101): put `source` into `ChangeSignal`, resolved on the monitor thread at OS-signal time (Phase 2 Task 101/105; platform interface change). Closes the dequeue-time race for excluded apps.
- F-02 (done in 106): StorageFull → retention then retry once.
- F-03 (done in 101: `sequence()` never fails on Windows): a `sequence()` error at resume leaves the current seq unmarked (possible backfill if a stale signal follows).
- F-04 (done in 106): `upsert_clip` took `NewClip` by value and cannot wipe the reps on error; consider `&NewClip` or a zeroize-on-drop byte type in core.
- F-05 (done in 103b): images were hashed without decoded RGBA and without thumbnails (no `png` decode in app yet); DIB vs PNG copies of one image do not dedupe. Phase 2 Task 103 should decode in the adapter or app.
- F-06 (partly closed): unix-only permission and fsync paths now run in CI tests on Linux and macOS (green since the first push); no manual run on a real Unix machine.
- F-07 (resolved in 013): search bench p95 ≈ 24–25 ms against a 30 ms budget.

## Top risks (full list: `docs/plans/roadmap.md`)

R-01 GNOME data-control unknown · R-02 password managers without markers (mitigated for KeePassXC 2.8.0 on Windows: built-in exclusion + markers seen; re-verify with skip-reason output; open for Bitwarden/1Password) · R-03 macOS pasteboard privacy prompt · R-04 Slint latency/IME · R-08 licence.

## Phase 0 evidence

- Windows clipboard spike: `docs/spikes/windows-clipboard-monitor.md` (8 scenarios, all run).
- UI spikes: `docs/spikes/ui-shell-comparison.md` (Tauri 160 MiB private / 7 processes / 1.7 s setup vs Slint 49 MiB / 1 process / 0.2 s).
- macOS and Linux: nothing tested (no hardware in Phase 0).
