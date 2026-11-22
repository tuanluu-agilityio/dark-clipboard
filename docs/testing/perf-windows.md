# Windows headless performance (Task 106)

Date: 2026-11-22. Machine: Windows 11 Home Single Language 10.0.26300, the owner's work machine (in use during the run).
Binary: `cargo build --release -p dark-clipboard` (release profile: opt-level z, lto, panic abort), 1,783,296 bytes.
The clipboard was never written by these runs; the monitor only listened.

> Since Task 208 (D-054) a fresh data folder captures nothing: `--headless` logs `capture_off` and prints that capture is off until Start is pressed in the app. For a run that needs capture, first run the UI once with the same `--data-dir` (`dark-clipboard.exe --data-dir <scratch>`), press Start in the welcome, quit from the tray, then start `--headless` on that folder. There is no flag that skips the welcome.

## 10 minute idle

Command (sampled by a PowerShell script, one `Get-Process -Id <pid>` per 60 s):

    dark-clipboard.exe --data-dir <tempdir> --headless --exit-after-ms 630000

| t (s) | CPU time (s) | Private bytes | Working set (bytes) | Threads | Handles |
|---|---|---|---|---|---|
| 60 | 0.015625 | 1,708,032 | 9,121,792 | 7 | 107 |
| 120 | 0.046875 | 2,232,320 | 10,125,312 | 10 | 122 |
| 180 | 0.046875 | 2,138,112 | 10,076,160 | 7 | 122 |
| 240 | 0.046875 | 2,138,112 | 10,076,160 | 7 | 122 |
| 300 | 0.046875 | 2,203,648 | 10,199,040 | 9 | 122 |
| 360 | 0.046875 | 2,203,648 | 10,199,040 | 9 | 122 |
| 420 | 0.046875 | 2,138,112 | 10,174,464 | 7 | 122 |
| 480 | 0.046875 | 2,138,112 | 10,174,464 | 7 | 122 |
| 540 | 0.046875 | 2,138,112 | 10,174,464 | 7 | 122 |
| 600 | 0.0625 | 2,170,880 | 10,149,888 | 8 | 122 |

Total CPU time over 600 s: 0.0625 s (about 0.01 % of one core). Private bytes stayed between 1.7 and 2.2 MB.
Caveat: this was not a pure idle clipboard. The log of that run shows `added=3 bumped=6` (the owner copied things on this machine during the run), so CPU includes those nine captures.

## Shutdown time

Command, 5 runs: `dark-clipboard.exe --data-dir <tempdir> --headless --exit-after-ms 500`. The `stop ms=` value in the log is the time `App::shutdown` took (stop request to actor and event pump joined). All exit codes were 0.

Raw `stop ms` values: 6, 7, 4, 4, 4 (the 10 minute run above: 2). Limit is 2000 ms.

Not measured: a real Ctrl+C in a console (the `ctrlc` handler feeds the same stop path as `--exit-after-ms`); the owner should test it by hand.

# Popup open time, NFR-01 (Task 204)

Date: 2026-11-23. Same machine (Windows 11 Home Single Language 10.0.26300, the owner's work machine, in use). Binary: `cargo build --release -p dark-clipboard`. Budget: popup visible ≤ 100 ms p95 after the trigger on a warm app.

What is measured: from the moment the running app receives the show request to the first frame rendered after `show()` (Slint rendering notifier, `AfterRendering`). The app logs it content-free as `perf what=popup_shown src=<ipc|hotkey|tray> us=<microseconds>`. For `src=ipc` the clock starts when the IPC thread receives `--toggle`, so the start-up of the `--toggle` client process is not included. No key or mouse input was simulated and the clipboard was never written. Each run used a fresh temp data dir (deleted afterwards) and `--no-global-shortcut`.

Command: `powershell -File scripts/measure-popup.ps1 -Trials 50 [-SeedClips 10000]` (50 × show + hide through `--toggle`, 400 ms shown, 250 ms hidden; the script prints every value and p50/p95 by nearest rank).

| Run | History | min | p50 | p95 | max |
|---|---|---|---|---|---|
| 1 | 10,000 synthetic clips (`-SeedClips 10000`) | 5.7 ms | 12.8 ms | 26.7 ms | 29.9 ms |
| 2 | empty | 6.1 ms | 12.2 ms | 23.5 ms | 27.1 ms |
| 3 | 10,000 synthetic clips, final Task 204 code (runs 1–2 used a build a few small fixes earlier: tray reopen guard, paging after results, in-place delete/undo; the show path was unchanged) | 4.7 ms | 13.8 ms | 21.9 ms | 28.1 ms |

Raw values, run 1 (ms, in order): 18.6, 10.2, 11.9, 10.9, 21.2, 23.0, 24.3, 6.6, 7.7, 19.7, 22.4, 19.2, 9.9, 8.0, 8.9, 12.2, 12.8, 17.6, 21.6, 5.7, 9.4, 7.3, 11.5, 8.1, 13.9, 28.0, 17.1, 21.6, 18.7, 7.6, 16.0, 20.3, 16.1, 16.1, 9.7, 10.5, 29.9, 9.3, 15.8, 26.7, 19.8, 16.0, 11.8, 8.7, 24.6, 14.0, 9.7, 12.6, 7.5, 8.5.

Raw values, run 2 (ms, in order): 15.3, 14.0, 8.3, 6.6, 13.1, 6.1, 19.3, 8.9, 16.2, 12.0, 27.1, 9.3, 6.4, 11.6, 25.1, 16.2, 7.2, 10.5, 7.2, 10.2, 15.6, 9.8, 16.6, 14.7, 23.5, 12.9, 22.3, 12.1, 19.8, 22.5, 20.4, 6.5, 10.6, 17.4, 8.9, 14.7, 7.8, 11.9, 10.5, 9.0, 16.5, 18.2, 12.7, 22.4, 12.2, 11.5, 7.5, 12.3, 8.2, 21.8.

Raw values, run 3 (ms, in order): 20.2, 9.3, 17.8, 14.0, 15.0, 4.7, 13.5, 8.5, 8.4, 16.8, 15.7, 21.9, 15.6, 10.3, 13.8, 21.7, 12.0, 19.1, 8.5, 14.3, 8.1, 12.4, 14.8, 20.0, 20.1, 11.2, 28.1, 18.6, 12.3, 12.0, 18.2, 15.6, 11.2, 13.2, 15.3, 14.0, 10.7, 7.8, 11.2, 12.3, 10.3, 17.3, 11.6, 13.8, 10.0, 13.5, 18.7, 13.8, 22.6, 18.2.

Not in these numbers:
- The first show after the app starts can be slower: in an earlier smoke run (not part of the 50 trials) the very first show took 225 ms, the next ones about 11 ms. The window is created hidden at start, but its OS window and GPU surface are created on the first show. Budget NFR-01 is for a warm app.
- The real shortcut (owner manual row): start the app normally, press `Ctrl+Shift+V` 50 times (open and close), then `powershell -File scripts/measure-popup.ps1 -LogPath %LOCALAPPDATA%\DarkClipboard\logs\dark-clipboard.log -Source hotkey`.
- Search latency per keystroke with 10,000 clips is covered by the storage bench (NFR-02, Task 013); the popup adds a 60 ms typing debounce on top.

# ADR-0001 revisit check (Task 209)

Date: 2026-11-23. Machine: Windows 11 Home Single Language 10.0.26300, the owner's work machine (in use during all runs). Commit 9322b5e (branch `task/209-adr0001-revisit`), rustc 1.100.0, Slint 1.19.0.
Binary: `cargo build --release -p dark-clipboard` (11,058,176 bytes, the full UI build; the 1.78 MB figure above is the Task 106 headless-only build).
Method: every run used a fresh temp `--data-dir` seeded with `--seed-demo-clips 10000`, `--no-global-shortcut` and `--exit-after-ms`; the dir was deleted afterwards and the deletion confirmed. The popup was shown only through `--toggle` over IPC. No screenshot, no input simulation, the real clipboard was never written, no clip content was read.
Capture state: a fresh dir starts with capture off (D-054). For run A the test setup pre-wrote `settings.json` (`onboarding_done: true`, the `privacy-policy.default.json` example with `capture_enabled: true`). Capture-on is inferred from that file; a 6 s check run on the same settings logged no `capture_off` line, but the UI path has no explicit "capture on" log line, so this is not independently proven. Runs A and B ran at the same time (two processes on the machine).

## Idle CPU and memory (NFR-04, NFR-05)

Script: a throwaway PowerShell sampler (not committed) doing one `Get-Process -Id <pid>` per 60 s. Popup hidden for the whole 600 s, then 20 shows (`--toggle` show, 400 ms, hide, 300 ms), then two samples after.

Run A, capture ON:

| Sample | t (s) | CPU time (s) | Private bytes | Working set | Threads | Handles |
|---|---|---|---|---|---|---|
| idle | 10 | 1.109375 | 86,360,064 | 115,228,672 | 23 | 470 |
| idle | 60 | 1.140625 | 86,224,896 | 115,167,232 | 19 | 465 |
| idle | 120 | 1.203125 | 86,224,896 | 115,167,232 | 19 | 465 |
| idle | 180 | 1.3125 | 86,130,688 | 115,118,080 | 16 | 461 |
| idle | 240 | 1.375 | 86,130,688 | 115,118,080 | 16 | 461 |
| idle | 300 | 1.5 | 86,196,224 | 115,449,856 | 18 | 461 |
| idle | 360 | 1.546875 | 86,196,224 | 115,449,856 | 18 | 461 |
| idle | 420 | 1.75 | 86,200,320 | 115,535,872 | 18 | 463 |
| idle | 480 | 1.84375 | 86,130,688 | 115,503,104 | 16 | 457 |
| idle | 540 | 1.96875 | 86,130,688 | 115,503,104 | 16 | 457 |
| idle | 600 | 2.0625 | 86,159,360 | 115,535,872 | 17 | 457 |
| popup shown, 20th show | 655 | 2.671875 | 103,391,232 | 140,169,216 | 17 | 486 |
| hidden, 5 s after 20 shows | 661 | 2.6875 | 102,866,944 | 139,644,928 | 17 | 486 |
| hidden, 35 s after 20 shows | 691 | 2.75 | 102,838,272 | 139,628,544 | 16 | 486 |

Run B, capture OFF:

| Sample | t (s) | CPU time (s) | Private bytes | Working set | Threads | Handles |
|---|---|---|---|---|---|---|
| idle | 10 | 1.03125 | 86,626,304 | 115,216,384 | 23 | 470 |
| idle | 60 | 1.25 | 86,495,232 | 115,159,040 | 19 | 463 |
| idle | 120 | 1.34375 | 86,495,232 | 115,159,040 | 19 | 463 |
| idle | 180 | 1.453125 | 86,409,216 | 115,109,888 | 16 | 461 |
| idle | 240 | 1.59375 | 86,409,216 | 115,109,888 | 16 | 461 |
| idle | 300 | 1.703125 | 86,474,752 | 115,441,664 | 18 | 461 |
| idle | 360 | 1.828125 | 86,474,752 | 115,441,664 | 18 | 461 |
| idle | 420 | 2.015625 | 86,503,424 | 115,552,256 | 19 | 463 |
| idle | 480 | 2.140625 | 86,409,216 | 115,499,008 | 16 | 457 |
| idle | 540 | 2.390625 | 86,409,216 | 115,499,008 | 16 | 457 |
| idle | 600 | 2.46875 | 86,437,888 | 115,531,776 | 17 | 457 |
| popup shown, 20th show | 655 | 3.171875 | 103,608,320 | 140,460,032 | 17 | 486 |
| hidden, 5 s after 20 shows | 661 | 3.21875 | 103,084,032 | 139,935,744 | 17 | 486 |
| hidden, 35 s after 20 shows | 691 | 3.265625 | 103,051,264 | 139,919,360 | 16 | 486 |

Both runs exited with code 0 on `--quit`.

Reading the numbers:
- Idle CPU (hidden popup, t = 60 to 600 s, 540 s): run A 0.921875 s = 0.17 % of one core; run B 1.21875 s = 0.23 % of one core. The first 10 s cost about 1.1 s of CPU in both (start-up). This is not zero: a few tenths of a percent from the UI process, versus 0.01 % for the headless build above. The owner was using the machine and both instances ran at once, so treat it as an upper bound, not a clean floor. Capture on or off made no systematic difference (B was higher).
- Private bytes hidden: 86.1 to 86.6 MB, that is 82.1 to 82.6 MiB (working set about 110 MiB). Flat over 10 minutes (no growth). After 20 popup shows: 102.8 to 103.6 MB = 98.1 to 98.8 MiB, about +16 MiB once the popup has been shown, then flat 35 s later (not given back when the popup hides). 20 shows do not prove there is no leak over thousands of shows; not measured.
- NFR-05 (<= 40 MiB hidden): not met. The gap is about 42 MiB (82 versus 40) and about 33 MiB above the 49 MiB the Slint spike measured. This build has the full UI and a 10,000-clip history; the spike did not. For comparison the Tauri spike measured 160 MiB private / 372 MiB working set.

## Popup latency (NFR-01)

Command: `powershell -File scripts/measure-popup.ps1 -Trials 50 -SeedClips 10000` (capture OFF, fresh data dir, 10,000 synthetic clips, 400 ms shown, 250 ms hidden).

    raw ms (50 trials, src=ipc): 14.6, 7.4, 21.1, 17.2, 7.7, 9.0, 14.2, 15.6, 14.7, 11.4, 9.6, 7.7, 7.6, 11.0, 12.6, 13.3, 6.1, 11.0, 12.1, 9.0, 7.0, 11.3, 10.8, 10.6, 10.8, 10.4, 11.6, 13.3, 7.5, 11.1, 22.2, 11.8, 15.2, 11.4, 18.0, 15.5, 13.8, 18.3, 19.2, 12.6, 23.0, 10.6, 10.6, 10.8, 20.6, 16.1, 11.3, 8.5, 15.0, 13.4
    min 6.1 ms  p50 11.4 ms  p95 21.1 ms  max 23.0 ms
    temp data dir deleted

Budget 100 ms p95: met with a wide margin (21.1 ms), consistent with Task 204 (21.9 to 26.7 ms). Same scope limits as the Task 204 section: IPC clock start, warm app, first show after start not in the 50 (225 ms seen once earlier), real shortcut is an owner manual row. Not run with capture on.

## Search latency (NFR-02)

The UI path (keystroke to results model updated) is NOT measured: the desktop app has no content-free timing event for search and this task may not edit product code. What exists:
- Storage layer on 10,000 clips, `cargo test --release -p darkclip-storage -- --ignored --nocapture bench_search` (synthetic data, no clipboard involved), 200 queries:

        bench_search_realistic: p50=4.2722ms p95=19.0819ms (avg clip 1029 bytes, 0 of 200 queries empty)
        bench_search: p50=13.2025ms p95=19.8203ms

- Budget is <= 30 ms p95 per keystroke: met at the storage layer (19.1 to 19.8 ms). The popup adds a 60 ms typing debounce (`DEBOUNCE` in `apps/desktop/src/ui/popup_host.rs` and `history_host.rs`) and the model update on top; those two are not measured. A follow-up could add a `perf what=search_done` event (a product-code change, not done here).

## Narrator smoke (ADR trigger 4)

Not automated and not run: Narrator needs real keyboard focus, which the owner's-machine rule forbids us to drive. What the source declares (read-only inventory of `apps/desktop/ui/*.slint`, 91 `accessible-*` properties; whether Narrator actually announces them is unverified):
- Popup: the search box has label, placeholder and key-hint description; filter chips are checkboxes with label and checked state; clip rows are `list-item` with label, description, selected state, index and a default action; the list has role `list`, a label and an item count; the empty/error state is a labelled group; the image preview has a label; file rows are labelled with the path; buttons have role and label.
- History window: same search/list/filter pattern, checkbox filters, buttons, and a delete-confirm group labelled with its text.
- Settings: tabs with role `tab` and checked state, a tab list, labelled shortcut, language and position fields.
- Welcome: has accessible properties (not itemised here).

Owner manual row (Narrator, Windows): press Win+Ctrl+Enter, open the popup, then check and write down verbatim what Narrator says for: (1) the search box on focus; (2) arrow Down/Up over the list (does it say the clip text, its type, "item N of M"?); (3) each filter chip (name and on/off); (4) Enter on a row; (5) the Settings tabs and each field; (6) the welcome Start / Not now buttons. Pass = every one has a spoken name.

## ADR-0001 revisit triggers

| # | Trigger | Verdict | Evidence |
|---|---|---|---|
| 1 | Popup cannot show in <= 100 ms p95 on any target OS | Not hit on Windows: p95 21.1 ms. macOS and Linux not measured here | measurement above |
| 2 | IME (Vietnamese Telex, Japanese) fails in `LineEdit` | Not tested here (needs real typing, forbidden on this machine); owner manual row | none |
| 3 | Non-activating popup that returns focus is impossible on macOS with winit | Not testable on Windows | none |
| 4 | Screen-reader access to the clip list not achievable | Not decided: accessibility properties are declared in source (list, list-item, count); Narrator itself not run; owner manual row above | source inventory only |

No trigger is hit by a measurement. Separately, and not an ADR trigger: NFR-05 is not met (82 MiB hidden, 98 MiB after use, target 40 MiB) and idle CPU is 0.17 to 0.23 % of one core rather than about 0 %. These are for the orchestrator to weigh; the ADR was not changed.
