# UI redesign verification (Task 708): P1 to P6 measurements

Date: 2026-11-25. Machine: Windows 11 Home Single Language, build 26300, Intel Core Ultra 7 155H. Tools: cargo 1.100.0, rustc 1.100.0. Budgets: `docs/specs/ui-redesign/05-acceptance.md` section 2. Renderer: Slint software renderer (default since Task 212). GC-19: no screenshots, no input simulation, the real clipboard was never written or read (`capture_enabled: false`), the apps only ran on temp `--data-dir`s with `--no-global-shortcut --exit-after-ms`; show and hide only with `--toggle` over IPC.

- **Baseline build:** tag `v0.1.2` (commit `558f81f`, the last release before Phase 7), built in a scratch git worktree outside the repo (`git worktree add <scratch>/v012 v0.1.2`, removed afterwards) with `cargo build --release -p dark-clipboard`.
- **Phase 7 build:** branch `task/708-verification` at commit `72a6bb2` (Tasks 700 to 707), `cargo build --release -p dark-clipboard`.
- Both builds were measured in the same session on the same machine with the same throwaway scripts (kept in the session scratchpad, not in the repo).

## Method

Per run: fresh temp `--data-dir`; a BOM-less `settings.json` written with .NET `UTF8Encoding($false)`: `{settings_version: 1, onboarding_done: true, privacy: <docs/architecture/examples/privacy-policy.default.json with capture_enabled = false>}`; then `dark-clipboard.exe --seed-demo-clips 10000 --data-dir <dir>`; then the app `--data-dir <dir> --no-global-shortcut --exit-after-ms <ms>`; `--toggle` and `--quit` over IPC; the dir deleted (deletion confirmed). `settings.json.corrupt*` count was 0 after every run.

- **P1:** `scripts/measure-popup.ps1` logic unchanged (50 trials = 50 `--toggle` shows, `perf what=popup_shown src=ipc`, nearest-rank p95). The scratch copy `measure-popup-ns.ps1` only adds two things to it: writing the settings file above before seeding, and printing the `settings.json.corrupt*` count. Command: `powershell -File measure-popup-ns.ps1 -Exe <exe> -Trials 50 -SeedClips 10000`. The Task 212 timing source works on the software renderer (first winit `RedrawRequested` after the show), 50 events per run.
- **P3, P4, P5, P6:** one script, `measure-mem.ps1 -Exe <exe> -Runs 3`: after the log says `running`, wait 8 s, read `Process.PrivateMemorySize64` (P3, hidden t = 8 s); read `TotalProcessorTime` before and after a 60 s hidden wait (P5: delta CPU ms over wall ms); `--toggle` (show), wait 2 s, read (P4 popup shown); `--toggle` (hide), wait 2 s, read (hidden after popup); `--quit`. P6 is the byte length of the release exe.

## Raw numbers

### P1: popup open p95 (ms), 50 trials per run, IPC source

Order of the runs: runs 1 to 3 were the three planned runs (baseline and Phase 7 alternating). Runs 1 to 3 were **disturbed by system load**: bursts of 55 to 105 ms values appear inside the raw series of both builds (see the raw lists below), at times unrelated to the build (for example baseline run 2 and 3, Phase 7 run 1 and 2). The cause was not identified (another process on the owner's live machine, not touched by this task). Runs 4 and 5 were added afterwards (alternating, same method) when the machine was quiet.

| Run | Baseline v0.1.2 p50 / p95 / max | Phase 7 p50 / p95 / max | Machine |
|---|---|---|---|
| 1 | 17.7 / **25.7** / 61.7 | 14.4 / **97.4** / 105.0 | disturbed (Phase 7: last 6 values 80 to 105) |
| 2 | 31.6 / **75.8** / 321.4 | 56.8 / **105.1** / 240.8 | disturbed |
| 3 | 11.4 / **65.2** / 84.2 | 12.4 / **19.2** / 42.9 | baseline disturbed in the first third, Phase 7 quiet |
| 4 | 10.4 / **18.7** / 24.0 | 13.1 / **22.8** / 28.3 | quiet |
| 5 | 10.8 / **14.5** / 39.1 | 13.1 / **21.7** / 28.3 | quiet |

Mean p95, all five runs: baseline 40.0 ms, Phase 7 53.2 ms (not meaningful, the load dominates). Mean p95 of the quiet runs 4 and 5: baseline 16.6 ms, Phase 7 22.3 ms (**+5.7 ms**). The task package's old figure (21.1 ms, `perf-windows.md`) was from a different session.

Raw series (ms):

- Baseline run 1: 61.7, 16.8, 16.6, 19.8, 15.9, 13.7, 12.5, 13.0, 18.1, 17.0, 18.7, 11.4, 17.7, 15.0, 12.3, 11.9, 20.4, 16.8, 21.1, 16.9, 21.4, 18.5, 23.4, 24.7, 26.6, 13.5, 15.3, 17.0, 25.7, 20.7, 17.7, 21.0, 17.3, 22.8, 19.9, 18.7, 17.5, 11.0, 19.2, 23.8, 17.3, 20.7, 20.6, 14.6, 20.1, 19.3, 13.4, 16.3, 18.6, 20.0
- Phase 7 run 1: 43.4, 16.4, 9.9, 14.6, 12.9, 13.8, 11.1, 11.3, 22.6, 12.7, 13.7, 10.6, 11.1, 19.7, 13.9, 13.3, 12.7, 14.3, 14.4, 14.7, 14.8, 10.3, 10.1, 13.3, 12.8, 13.0, 17.9, 12.0, 16.1, 10.6, 11.7, 16.8, 12.0, 14.4, 15.6, 16.4, 16.2, 16.4, 12.5, 16.6, 15.6, 14.6, 12.0, 45.6, 105.0, 104.1, 81.2, 97.4, 80.3, 88.0
- Baseline run 2: 321.4, 68.8, 58.4, 75.8, 56.5, 73.2, 62.9, 65.1, 63.1, 28.8, 56.1, 16.0, 39.7, 14.8, 20.1, 11.2, 28.2, 17.7, 17.6, 31.6, 35.8, 12.1, 13.0, 12.3, 11.2, 23.1, 13.1, 15.4, 12.6, 15.5, 15.3, 17.3, 25.6, 11.2, 18.1, 13.5, 18.2, 54.3, 69.7, 81.7, 65.4, 60.9, 59.0, 57.0, 61.5, 52.8, 69.4, 63.7, 57.2, 57.5
- Phase 7 run 2: 240.8, 105.1, 105.4, 82.7, 82.1, 84.4, 82.4, 66.0, 37.7, 58.6, 78.7, 64.8, 42.1, 46.7, 82.5, 68.0, 78.5, 36.6, 45.7, 45.9, 44.1, 43.9, 55.5, 69.5, 75.4, 31.6, 86.2, 80.2, 55.6, 17.9, 19.0, 13.5, 15.7, 23.3, 19.2, 22.0, 23.0, 17.7, 18.7, 15.6, 24.3, 38.7, 56.8, 80.1, 82.5, 89.7, 88.9, 73.1, 62.2, 76.8
- Baseline run 3: 36.0, 13.1, 15.5, 13.9, 12.1, 13.4, 10.1, 15.4, 13.7, 11.9, 67.7, 61.1, 65.2, 62.4, 84.2, 46.5, 64.5, 64.5, 64.6, 43.3, 54.1, 17.6, 7.7, 10.9, 10.1, 9.8, 10.1, 12.0, 8.9, 11.4, 12.5, 10.8, 11.6, 9.6, 8.2, 9.1, 8.5, 10.7, 9.8, 9.3, 8.7, 12.8, 9.9, 9.5, 8.7, 7.9, 8.6, 8.8, 9.8, 10.3
- Phase 7 run 3: 42.9, 15.4, 11.3, 11.9, 15.7, 11.9, 11.5, 11.6, 14.4, 10.7, 11.7, 13.1, 12.3, 12.4, 9.9, 14.3, 13.0, 11.3, 13.3, 10.9, 10.1, 17.2, 10.9, 12.2, 14.5, 11.5, 11.8, 11.7, 13.4, 11.8, 13.3, 13.5, 11.5, 12.3, 14.6, 10.6, 15.7, 12.2, 13.9, 13.6, 19.2, 12.9, 13.9, 19.3, 7.5, 16.9, 14.2, 12.4, 15.5, 18.9
- Phase 7 run 4: 28.3, 17.0, 13.7, 13.2, 12.7, 13.5, 11.0, 6.8, 7.1, 13.5, 11.3, 11.6, 17.2, 12.8, 15.9, 10.4, 11.7, 15.9, 13.4, 13.6, 15.3, 22.8, 11.1, 11.3, 11.8, 9.1, 14.7, 13.1, 14.0, 25.4, 11.2, 9.7, 12.8, 14.9, 19.7, 11.5, 11.8, 16.7, 13.7, 14.3, 13.1, 13.8, 11.0, 7.8, 13.3, 12.0, 12.4, 11.5, 12.7, 13.1
- Baseline run 4: 24.0, 21.9, 13.2, 7.9, 10.2, 12.4, 8.2, 11.4, 7.3, 8.4, 9.6, 15.7, 8.0, 16.8, 11.2, 13.4, 12.1, 15.5, 9.7, 12.7, 9.5, 8.7, 9.7, 7.8, 10.5, 13.4, 18.7, 11.5, 10.6, 13.3, 12.3, 9.6, 12.3, 8.8, 12.4, 8.5, 7.6, 11.3, 12.3, 6.2, 10.4, 8.2, 7.8, 10.3, 8.4, 9.5, 8.2, 13.2, 9.4, 14.9
- Phase 7 run 5: 28.3, 14.0, 19.4, 13.7, 13.4, 12.3, 12.8, 12.4, 23.0, 10.6, 9.7, 11.4, 13.4, 12.5, 14.9, 14.2, 9.1, 13.1, 12.1, 10.6, 14.2, 14.5, 10.3, 12.3, 11.9, 13.9, 9.4, 11.3, 12.1, 12.7, 15.5, 13.4, 18.5, 17.2, 21.7, 12.5, 14.7, 13.9, 17.3, 12.3, 12.7, 15.0, 15.5, 16.2, 10.7, 16.7, 12.0, 15.9, 12.9, 9.0
- Baseline run 5: 39.1, 10.8, 18.8, 7.5, 13.9, 13.1, 14.4, 14.5, 11.4, 11.6, 9.9, 9.9, 11.2, 11.6, 9.1, 13.8, 12.0, 11.5, 10.7, 10.7, 12.8, 10.4, 8.8, 11.0, 8.6, 11.2, 9.6, 12.2, 11.7, 10.0, 10.2, 8.9, 10.2, 13.9, 7.3, 12.6, 9.0, 9.8, 11.2, 9.6, 12.2, 12.8, 10.9, 9.8, 10.6, 14.5, 9.1, 7.8, 10.2, 9.6

### P3, P4, P5: private bytes and idle CPU (10,000 synthetic clips)

| Build | Run | hidden t = 8 s | popup shown | hidden after popup | idle CPU 60 s hidden |
|---|---|---|---|---|---|
| baseline | 1 | 10,153,984 | 12,627,968 | 12,648,448 | 0.0 ms (0 %) |
| baseline | 2 | 10,211,328 | 12,374,016 | 12,374,016 | 0.0 ms (0 %) |
| baseline | 3 | 10,121,216 | 12,496,896 | 12,496,896 | 0.0 ms (0 %) |
| Phase 7 | 1 | 9,953,280 | 12,640,256 | 12,640,256 | 0.0 ms (0 %) |
| Phase 7 | 2 | 10,027,008 | 13,361,152 | 13,361,152 | 0.0 ms (0 %) |
| Phase 7 | 3 | 10,174,464 | 13,250,560 | 13,250,560 | 15.6 ms (0.026 %) |

Means (bytes, MiB): hidden baseline 10,162,176 (9.69), Phase 7 10,051,584 (9.59); popup shown baseline 12,499,627 (11.92), Phase 7 13,083,989 (12.48); hidden after popup baseline 12,506,453 (11.93), Phase 7 13,083,989 (12.48). The three Phase 7 runs ran after the P1 runs of the same session; run-to-run spread is up to 0.2 MiB hidden and 0.7 MiB after the popup, so differences below about 0.7 MiB are not real.

### P6: release exe size

| Build | Bytes |
|---|---|
| baseline v0.1.2 | 13,105,664 (12.50 MiB) |
| Task 700 (`ui-redesign-baseline.md`) | 13,142,528 |
| Phase 7 (`72a6bb2`) | 14,079,488 (13.43 MiB) |

## Result table

| # | Criterion | Budget | Baseline v0.1.2 | Task 700 | Phase 7 now | Verdict |
|---|---|---|---|---|---|---|
| P1 | Popup open p95 (IPC) | <= 30 ms | quiet runs 18.7, 14.5 (mean 16.6 ms); disturbed 25.7, 75.8, 65.2 | n/a | quiet runs 22.8, 21.7 (mean 22.3 ms); disturbed 97.4, 105.1, 19.2 | **Pass on the quiet runs (4 and 5, and run 3), not demonstrated by the three planned runs**: runs 1 and 2 of Phase 7 exceed 30 ms under load, but the baseline exceeds it in 2 of 3 of the same runs. Phase 7 is about +5.7 ms slower than baseline on a quiet machine. Needs a clean re-run (owner or idle machine) before it is called proven. |
| P2 | Search keystroke to list update p95 | no regression > 10 % | n/a | n/a | not measured | **Not measurable here**: `perf what=search_done` needs typed queries, which cannot be produced without simulated input (GC-19). Owner row 10 in the Phase 7 demo checklist. |
| P3 | Private bytes, hidden, t = 8 s | <= 9 MiB | 9.69 MiB | 9,748,480 to 10,039,296 B (9.30 to 9.57 MiB, `ui-redesign-baseline.md`) | 9.59 MiB | **Fail against the absolute 9 MiB budget** (9,437,184 B): both builds exceed it with 10,000 seeded clips; Phase 7 is 0.11 MiB below baseline (inside the spread). The 9 MiB figure came from a smaller data set (6.9 MiB, `perf-windows.md`); the budget needs an owner decision. Not caused by Phase 7. |
| P4 | Private bytes after popup shown and hidden | baseline + 4 MiB | 11.92 MiB (shown), 11.93 MiB (hidden after) | 12.18 to 12.31 MiB | 12.48 MiB (both) | **Pass**: +0.56 MiB over baseline (budget +4 MiB). Limit: popup only; History and Settings cannot be opened without input, owner row 11. |
| P5 | Idle CPU, hidden, 60 s | 0 % | 0 / 0 / 0 % | n/a | 0 / 0 / 0.026 % | **Pass with a caveat**: 2 of 3 Phase 7 runs 0.0 ms; run 3 used 15.6 ms of CPU in 60 s, exactly one Windows scheduler tick (15.625 ms), which cannot be told apart from a late start-up task. Not strictly 0 in that run. |
| P6 | Release exe size | baseline + 300 KiB (13,412,864 B) | 13,105,664 | 13,142,528 | 14,079,488 | **Fail**: +973,824 B (951 KiB) over baseline, +936,960 B since Task 700. Cause not investigated here (Tasks 701 to 707 added screens, icons and strings; the growth was not broken down); the orchestrator decides. |
| P7 | No animation > 120 ms; no timer while hidden | n/a | n/a | n/a | review only | Not part of this task (review + P5); the idle CPU result above is the only measured input. |
| P8 | Smooth scrolling with 10k clips | n/a | n/a | n/a | not measured | Owner row 12. |

## What was not measurable and why

- **P2** needs typed queries (no input simulation, GC-19).
- **P4 with History and Settings**: those windows open only from the tray menu or the popup, by input. Popup only was measured.
- **P8, P7 animation review**: owner and review.
- **P1 in a clean state**: three planned runs were disturbed by load on the owner's live machine; quiet runs 4 and 5 are the usable numbers. Re-run `scripts/measure-popup.ps1 -Trials 50 -SeedClips 10000` (the script writes no settings file, so on a fresh folder the welcome window shows and capture stays off) on an idle machine for a final answer.
- Visual checks (icons, DPI, Narrator, Vietnamese) are owner rows: `docs/testing/platform-checklist.md`, section "Phase 7 demo checklist (Task 708, Windows)".

## Task 711: exe size (F-39)

Release exe (`cargo build --release -p dark-clipboard`, Windows, one shared `--target-dir`), built at each `main` merge commit of Phase 7. The v0.1.2 row reproduces the recorded 13,105,664 B and the Task 707 row reproduces the 14,079,488 B of D-086.

| Merge commit | Task | Exe bytes | vs v0.1.2 (KiB) | Step (KiB) |
|---|---|---|---|---|
| `558f81f` | v0.1.2 | 13,105,664 | 0.0 | |
| `d314c55` | 700 kit and tokens | 13,142,528 | +36.0 | +36.0 |
| `c67b9ba` | 710 app icon | 13,353,984 | +242.5 | +206.5 |
| `d521ed7` | 701 popup layout | 13,526,528 | +411.0 | +168.5 |
| `072500c` | 703 history layout (on 701) | 13,617,152 | +499.5 | +88.5 |
| `69aea22` | 702 popup preview (on 701, 703) | 13,733,376 | +613.0 | +113.5 |
| `474fbad` | 705 settings | 13,902,336 | +778.0 | +165.0 |
| `9efe38a` | 704 history interaction | 13,981,696 | +855.5 | +77.5 |
| `4b69ad1` | 707 accessibility (includes 706) | 14,079,488 | +951.0 | +95.5 |

Task 706 was merged inside the 707 branch, so its share is in the last row.

Section sizes (PE headers) v0.1.2 -> Task 707: `.text` 7,763,770 -> 8,471,194 (+691 KiB), `.rdata` 5,033,214 -> 5,199,254 (+162 KiB), `.pdata` +29 KiB, `.rsrc` 0 -> 58,096 (the icon, +57 KiB), `.reloc` +12 KiB. Generated Slint code (`out/app.rs`): 8.75 MB -> 12.14 MB of Rust source, 3,440 bindings (median 1.1 KB each), 230 item-geometry and layout-info functions, 226 accessibility functions.

### Root causes

1. **PNG window icon (Task 710): -116 KiB.** `icon: @image-url("../res/window-64.png")` in four windows makes Slint link its raster image decoders (`image-default-formats`). Measured by removing the four lines: 14,079,488 -> 13,960,192 B. No other `.slint` file names a raster image.
2. **Generated Slint code (Tasks 701-707): about +600 KiB of `.text`.** The growth follows the number of elements, bindings and accessibility entries per window (settings window alone: 0.6 MB of Rust for the window struct plus 1.7 MB in its sub-components). It is spread over thousands of small bindings; there is no single large item. Nothing in it can be removed without changing the UI.
3. **Data (Task 710): +57 KiB icon resource (`.ico`, 8 sizes) and +36 KiB tray images (16 raw RGBA files), +16 KiB window icon as raw RGBA.** Required by the feature.

### Tried

| Experiment | Exe bytes | Result |
|---|---|---|
| Remove `ui/kit/gallery.slint` (`KitGallery`, 208 KB of generated source) | 14,079,488 (unchanged) | No size effect (the linker already dropped it); removed anyway (dead code, slower builds); scans now cover the real windows |
| Window icon from raw RGBA set in `theme::apply` instead of the PNG | 13,977,088 | **-102,400 B (-100 KiB)** net of the 16 KiB of pixel data |
| `opt-level = "s"` for the `dark-clipboard` package (`--config`) | 14,597,632 | +620 KiB: worse; `"z"` stays |
| Every kit component read for per-instance inlining (from the compiler rules, not measured) (root `opacity`, `visible`, animations) | | Only the `DialogScrim` family inlines (5 instances, one each); the `Icon` 49-way chain is a single sub-component; no duplication found |

### Result

13,977,088 B = **+851 KiB** over v0.1.2 (was +951 KiB). The +300 KiB budget is not reached: the remaining +851 KiB is about 600 KiB of generated UI code for the redesigned windows, 57 KiB of icon resource, 52 KiB of icon pixel data, plus smaller growth of `.rdata`, `.pdata` and `.reloc`. Reaching +300 KiB would need removing UI (elements, accessibility properties) or changing the Slint feature set or renderers, which is a behaviour change and out of scope.

Commands: `git worktree add --detach <scratch> <commit>`, `cargo build --release -p dark-clipboard --target-dir <shared>`, size with `stat -c %s <target>/release/dark-clipboard.exe`; sections read from the PE header; generated code from `<target>/release/build/dark-clipboard/*/out/app.rs`.
