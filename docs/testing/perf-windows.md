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

(Changed by Task 212, 2026-11-23: the timing now ends at the first winit `RedrawRequested` after the show request, taken on the next event-loop turn, because the software renderer that became the default has no rendering notifier. Same log line; see the Task 212 section.)

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
Method: every run used a fresh temp `--data-dir` (10,000 clips from `--seed-demo-clips 10000` unless stated), `--no-global-shortcut` and `--exit-after-ms`; the dir was deleted afterwards and the deletion confirmed. The popup was shown only through `--toggle` over IPC. No screenshot, no input simulation, the real clipboard was never written, no clip content was read. The sampler was a throwaway PowerShell script (one `Get-Process -Id <pid>` per 60 s); the window checks only list class, visibility and size of the app's own windows.

## Correction: the first measurements in this section were invalid

The first version of this section reported 86 MB private (82 MiB) hidden, 0.17 to 0.23 % idle CPU and "capture on/off" runs A and B. That was wrong:
- A fresh data dir without `settings.json` starts the first-run welcome (D-054): a 535x497 window is visible and focused, and capture is off. Confirmed in E1 below (the app's own window list shows it).
- The `settings.json` that the test pre-wrote for the "capture on" run was written with PowerShell 5.1 `Set-Content -Encoding utf8`, which adds a UTF-8 BOM (first bytes 239,187,191). The app treated the file as corrupt, renamed it to `settings.json.corrupt-<ms>` and started fresh. So that run also had the welcome visible and capture OFF, and the "capture on" label was false.
- The 50-trial latency run below the Task 204 section (p95 21.1 ms) also had the welcome window visible (no settings file). It is still a valid show/hide latency of the popup, but not the representative state; it was not repeated (the script cannot pre-write settings) and is reported as such.
Those numbers are not used anywhere below. The valid runs write `settings.json` without a BOM, and each shows 0 `settings.json.corrupt*` files and no welcome window (windows line below).

## Valid runs

Settings written by the test (test setup, not a product bypass): `onboarding_done: true` plus the `privacy-policy.default.json` example; `capture_enabled` true for A2, false for the others. A2 therefore recorded the owner's real clipboard into its temp dir for 11 minutes (never read, deleted). That capture was on is shown only by the file being accepted (no corrupt file, no welcome); the UI path has no explicit "capture on" log line.

### A2: capture ON, 10 minutes idle, popup hidden, then 20 shows

| Sample | t (s) | CPU time (s) | Private bytes | Working set | Threads |
|---|---|---|---|---|---|
| idle | 8 | 0.828125 | 54,558,720 | 74,272,768 | 22 |
| idle | 60 | 0.953125 | 54,415,360 | 74,207,232 | 18 |
| idle | 120 | 1.078125 | 54,415,360 | 74,207,232 | 18 |
| idle | 180 | 1.1875 | 54,325,248 | 74,153,984 | 15 |
| idle | 240 | 1.25 | 54,325,248 | 74,153,984 | 15 |
| idle | 300 | 1.328125 | 54,390,784 | 74,502,144 | 17 |
| idle | 360 | 1.453125 | 54,390,784 | 74,502,144 | 17 |
| idle | 420 | 1.578125 | 54,394,880 | 74,567,680 | 17 |
| idle | 480 | 1.75 | 54,325,248 | 74,530,816 | 15 |
| idle | 540 | 1.859375 | 54,325,248 | 74,530,816 | 15 |
| idle | 600 | 2.015625 | 54,358,016 | 74,571,776 | 16 |
| popup shown (20th show) | 655 | 2.546875 | 76,165,120 | 112,648,192 | 17 |
| hidden, 5 s after 20 shows | 661 | 2.5625 | 75,616,256 | 112,111,616 | 16 |

Own-window list at t = 8 s: only `Winit Thread Event Target` (15x15, visible; this is why `MainWindowHandle` is non-zero even with nothing on screen, so `MainWindowHandle` is not a test for "a window is shown"). Corrupt settings files: 0. Exit code 0.
- Idle CPU, t = 60 to 600 s: 1.0625 s over 540 s = 0.197 % of one core.
- Private bytes hidden: 54.3 to 54.6 MB = 51.8 to 52.0 MiB, flat over 10 minutes. After 20 shows: 75.6 MB = 72.1 MiB hidden (76.2 MB = 72.6 MiB while shown), about +21 MiB, not given back on hide. 20 shows do not prove there is no leak over thousands of shows.
- Working set about 71 MiB hidden, 107 MiB after use.

### E1: the welcome is shown on a fresh dir (seeded, no settings.json)

    start  t=8  cpu_s=1.171875  private=86487040  threads=23  MainWindowHandle=1183910
    windows: class=Window Class visible=True rect=535x497@25,25 | class=Winit Thread Event Target visible=True rect=15x15@0,0
    corrupt settings files: 0

A separate run listed six windows of the app process: the popup (420x560, hidden), the tray window (hidden), the welcome (535x497, visible) and the winit event target (15x15, visible) plus two hidden IME windows. `MainWindowHandle` is non-zero in every UI run (it points at the 15x15 event window when nothing else is visible), so it does not by itself prove a welcome. The window list does. Private bytes with the welcome open: 86.5 MB (82.5 MiB), versus 54.5 MB (52.0 MiB) without it: the welcome window costs about +32 MiB.

### E2: onboarded, capture OFF, never shown, 240 s (also E6 thread data)

| t (s) | CPU time (s) | Private bytes | Threads |
|---|---|---|---|
| 8 | 0.96875 | 54,710,272 | 21 |
| 60 | 1.0625 | 54,628,352 | 18 |
| 120 | 1.1875 | 54,628,352 | 18 |
| 180 | 1.390625 | 54,538,240 | 15 |
| 240 | 1.515625 | 54,538,240 | 15 |

Idle CPU t = 60 to 240: 0.453125 s over 180 s = 0.25 % of one core. Other runs were going at the same time (A2, E3, E4, E5); another process cannot add to this process's CPU time, and the owner's activity could only add through window messages, so this is this process's own cost, not a clean zero either.

### E3: empty dir versus 10,000 seeded clips (onboarded, capture off, never shown, 120 s)

| Run | Private bytes t=8 | t=60 | t=120 | CPU time t=60 / t=120 (s) |
|---|---|---|---|---|
| E3a empty | 51,752,960 | 51,613,696 | 51,613,696 | 0.5625 / 0.6875 |
| E3b 10,000 clips | 54,530,048 | 54,390,784 | 54,390,784 | 1.078125 / 1.171875 |

The 10,000 clips cost about 2.8 MB (2.65 MiB) private. The 49 to 52 MiB floor is the UI, not the history. (E3a idle CPU t = 60 to 120: 0.125 s per 60 s = 0.21 %; E3b: 0.094 s per 60 s = 0.16 %.)

### E4: `--headless` on the 10,000-clip dir (no Slint, no window)

    start t=8   cpu_s=0.75  private=5492736  ws=17653760  threads=10
    idle  t=60  cpu_s=0.75  private=5398528  ws=17600512  threads=7

Private 5.4 MB = 5.1 MiB, zero CPU over 52 s. `--quit` does not stop the headless process (exit code empty, it was killed by the script after 10 s). So the UI layer costs about 47 MiB private on top of the core.

### E5: `SLINT_BACKEND=winit-software`, onboarded, capture off

    start  t=8    cpu_s=0.859375  private=7372800   ws=26869760  threads=19
    idle   t=60   cpu_s=0.859375  private=7229440   ws=26804224  threads=15
    shown (20th show)  t=114  cpu_s=1.125  private=12279808  ws=33710080
    hidden after 20 shows t=121 cpu_s=1.125  private=12255232  ws=33697792

Private 7.2 MB (6.9 MiB) hidden, 12.3 MB (11.7 MiB) after 20 shows, zero CPU over the 52 idle seconds. Compared with the default renderer (52 MiB hidden, 72 MiB after shows, about 0.2 % CPU) this is about 7 times less memory and no idle CPU. Not verified for this renderer: how the popup looks (no screenshots allowed), the popup latency, text/image drawing speed. An attempt to run `measure-popup.ps1` with the variable set found 0 `perf` events: the popup timing is taken from Slint's rendering notifier, which the software renderer does not provide, so NFR-01 cannot be measured with it without a different timing source (product change). The software backend also needs checking on the other platforms.

### E6: which thread uses the idle CPU

Thread CPU time (`Process.Threads[].TotalProcessorTime`), onboarded, hidden popup. The thread that starts with the process (earliest start time) is the main thread.

    E2  t=60 : main 22416@55.409 = 0.1875 | 28408@55.586 = 0.7812 | 13256@55.627 = 0.0781 | 30016 = 0.0156
    E2  t=240: main 22416 = 0.1875 | 28408 = 0.7812 | 13256 = 0.5 | 30016 = 0.0469
    A2  t=60 : main 41400@53.326 = 0.1875 | 45312@53.429 = 0.6094 | 33920@53.461 = 0.1094 | 39624 = 0.0469
    A2  t=240: main 41400 = 0.1875 | 45312 = 0.6094 | 33920 = 0.375 | 39624 = 0.0781

Between t = 60 and 240 s the main thread and the big start-up thread (0.78 s / 0.61 s) add nothing; one worker thread started at process start (13256 in E2, +0.42 s of the +0.45 s; 33920 in A2, +0.27 s of the +0.30 s) accumulates about 90 % of the idle CPU, and a second small one adds about 0.03 s. Thread names are not set (`GetThreadDescription` is empty), so the role is not identified from outside. Because E5 (software renderer) shows exactly zero idle CPU, this worker is tied to the default (GPU) renderer path; identifying it needs a profiler or a product-code change.

## Analysis

- NFR-05 (<= 40 MiB private, hidden): not met. Valid figure 51.8 to 52.0 MiB with 10,000 clips (A2), 49.2 MiB with an empty history (E3a); gap about 12 MiB. The earlier "82 MiB" was the welcome window being open.
- Spike comparison: the Slint spike's 49 MiB was sampled after a show/hide, so the comparable number is the post-show figure: 72.1 MiB (A2, hidden after 20 shows), about +23 MiB over the spike. The hidden-before-first-show figure (52 MiB) is comparable to nothing in the spike. The post-use figure is also what a user has all day, so it is the one to judge NFR-05 against: 72 MiB versus the 40 MiB target. The Tauri spike (160 MiB private) is still about 2 times worse.
- Where the memory goes: core plus 10,000 clips is 5.4 MB (E4); the history adds 2.8 MB (E3); the UI layer is about 47 MiB (E3b - E4); the first popup shows add about 20 MiB; an open welcome adds about 32 MiB (E1). With the software renderer the same app is 7 MiB hidden and 12 MiB after use (E5), so the bulk of the cost is the GPU renderer state (swap chain and glyph/texture caches), not Slint's widgets or the data.
- NFR-04 (idle CPU about 0 %): not met as measured (A2: 0.197 % of one core, E2: 0.25 %, E3: 0.16 to 0.21 %). It is a worker thread, not the main thread, and it vanishes with the software renderer (E5: 0). The headless core is 0 (E4).
- NFR-01: met (p95 21.1 ms), measured with the welcome window visible; see the Correction section.
- A software-renderer fallback or default (`SLINT_BACKEND=winit-software`, or the `renderer-software` feature selected in the app) would meet NFR-05 and NFR-04 on Windows on these numbers, but costs the NFR-01 measurement method and has unverified visuals and speed. This is for the orchestrator to decide; the ADR was not changed.

## Popup latency (NFR-01)

Command: `powershell -File scripts/measure-popup.ps1 -Trials 50 -SeedClips 10000` (no settings file, so the welcome window was visible and capture off; 400 ms shown, 250 ms hidden).

    raw ms (50 trials, src=ipc): 14.6, 7.4, 21.1, 17.2, 7.7, 9.0, 14.2, 15.6, 14.7, 11.4, 9.6, 7.7, 7.6, 11.0, 12.6, 13.3, 6.1, 11.0, 12.1, 9.0, 7.0, 11.3, 10.8, 10.6, 10.8, 10.4, 11.6, 13.3, 7.5, 11.1, 22.2, 11.8, 15.2, 11.4, 18.0, 15.5, 13.8, 18.3, 19.2, 12.6, 23.0, 10.6, 10.6, 10.8, 20.6, 16.1, 11.3, 8.5, 15.0, 13.4
    min 6.1 ms  p50 11.4 ms  p95 21.1 ms  max 23.0 ms
    temp data dir deleted

Budget 100 ms p95: met with a wide margin (21.1 ms), consistent with Task 204 (21.9 to 26.7 ms, empty and seeded). Same scope limits as the Task 204 section: IPC clock start, warm app, first show after start not in the 50 (225 ms seen once earlier). The real shortcut is an owner manual row.

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

## Owner manual rows: IME (ADR trigger 2)

Not run here (it needs real typing). Do this on the owner's machine, once for the popup search box and once for the history window search box (tray menu, open history). Add the input methods under Windows Settings, Time and language, Language and region (Vietnamese keyboard with "Telex", and Japanese with Microsoft IME). Switch with Win+Space.

Vietnamese Telex (popup, then history window):
1. Open the popup (Ctrl+Shift+V), click or tab into the search box (it has focus on open).
2. Type `xin chaof`. Expected: the box shows `xin chào`; while the last word is being composed it is underlined, then commits when you press Space.
3. Clear (Ctrl+A, Delete). Type `Vieetj Nam`. Expected: `Việt Nam`.
4. While a syllable is still underlined (for example after typing `chaof` with no space), press Backspace once: the composition shrinks, nothing else happens.
5. While composing, press Esc once. Record whether it cancels the composition or closes the popup.
6. While composing, press Enter. Record whether it commits the text only, or also pastes the selected clip.
7. Type `vie` and look at the result list: with clips containing `Việt`, record whether the list is filtered as you commit.
Record verbatim: Windows build, keyboard/IME name, what each step showed (the exact text in the box), anything that flickered, duplicated or lost a character.
Pass: steps 2 and 3 give exactly the expected text with no stray or doubled characters; 4 does not close or change the popup; 5 and 6 do not paste a clip or close the popup while a composition is active (a second Esc/Enter, after the composition has ended, may); the list does not jump selection or lose focus mid-composition.

Japanese Microsoft IME (popup, then history window):
1. Switch to Japanese, hiragana mode (the IME bar shows `あ`). Click into the search box.
2. Type `nihongo`. Expected: `にほんご` underlined (composition).
3. Press Space. Expected: converted to `日本語`, with the candidate window appearing next to the box (record where it appears, at the box or at the screen corner).
4. Press Space again to cycle candidates, then Enter to commit. Expected: `日本語` committed in the box; the list filters on it.
5. Type `nihongo`, press F7 (katakana) then Enter: `ニホンゴ`.
6. While composing, press Esc, then press Down. Record: composition cancelled? Does Down move the clip selection while composing?
7. Press Enter on a committed text (no composition active): record whether it pastes the selected clip as usual.
Record verbatim as for Telex, plus the candidate-window position and whether the clip list steals Up/Down/Enter during composition.
Pass: 2 to 5 give the expected text; the candidate window appears at the text box; while a composition is active Up/Down/Enter/Esc act on the IME, not on the clip list or the popup; once committed, the normal keys work again.

## ADR-0001 revisit triggers

| # | Trigger | Verdict | Evidence |
|---|---|---|---|
| 1 | Popup cannot show in <= 100 ms p95 on any target OS | Not hit on Windows: p95 21.1 ms. macOS and Linux not measured here | measurement above |
| 2 | IME (Vietnamese Telex, Japanese) fails in `LineEdit` | Not tested here (needs real typing, forbidden on this machine); owner manual rows above | none |
| 3 | Non-activating popup that returns focus is impossible on macOS with winit | Not testable on Windows | none |
| 4 | Screen-reader access to the clip list not achievable | Not decided: accessibility properties are declared in source (list, list-item, count); Narrator itself not run; owner manual row above | source inventory only |

No trigger is hit by a measurement. Separately, and not an ADR trigger: NFR-05 is not met (52 MiB hidden, 72 MiB after use, target 40 MiB) and NFR-04 is not met as measured (0.2 % of one core in A2). Both are linked to the default GPU renderer (E5). These are for the orchestrator to weigh; the ADR was not changed.

# Software renderer by default (Task 212)

Date: 2026-11-23. Same machine (Windows 11 Home Single Language 10.0.26300, the owner's work machine, in use). Branch `task/212-renderer-memory` (uncommitted working tree at the time of the runs, the code as committed), Slint 1.19.0. Binary: `cargo build --release -p dark-clipboard`, 11,061,248 bytes (Task 209: 11,058,176; no Cargo feature changed: Slint's default features already contain `renderer-software` and `renderer-femtovg`, and the latter stays for a user's `SLINT_BACKEND=winit-femtovg` override and Slint's own internal fallback).

What changed: the app selects Slint's winit backend with the software renderer before any window exists (`slint::BackendSelector`), unless `SLINT_BACKEND` is set; the start line records `renderer=software|default|env`. The NFR-01 timing no longer uses Slint's rendering notifier (the software renderer has none): it is the first winit `RedrawRequested` of the popup window after the show request, taken on the next event-loop turn, so it includes that frame being drawn and presented. Same log line: `perf what=popup_shown src=<ipc|hotkey|tray> us=<microseconds>`.

Method (GC-19): every run used a fresh temp `--data-dir` with a `settings.json` written by the test without a BOM (`[IO.File]::WriteAllText` with `UTF8Encoding($false)`; first bytes `123,13,10`), containing `settings_version: 1`, `onboarding_done: true` and the `privacy-policy.default.json` example with `capture_enabled: false` (so the real clipboard was not even read), then `--seed-demo-clips 10000`, `--no-global-shortcut` and `--exit-after-ms`. The popup was shown only through `--toggle` over IPC (show 400 ms, hide 250 ms). The app's own window list (class, visibility, size; no titles) confirmed no welcome window. Each run quit with `--quit` (exit code 0), had 0 `settings.json.corrupt*` files, and its dir was deleted and the deletion confirmed. No screenshot, no input simulation, the clipboard was never written, no clip content was read. The driver was a throwaway PowerShell script; the latency values were parsed with `scripts/measure-popup.ps1 -LogPath <log> -Source ipc` (the script's own driving mode cannot pre-write `settings.json`, so it would run with the welcome open).

## R1: 5 minutes idle, popup never shown, then 20 shows (software renderer)

Start line: `start version=0.0.1 renderer=software`; no `renderer_fallback`.

| Sample | t (s) | CPU time (s) | Private bytes | Working set | Threads |
|---|---|---|---|---|---|
| idle | 9 | 0.65625 | 7,401,472 | 26,398,720 | 20 |
| idle | 60 | 0.65625 | 7,241,728 | 26,324,992 | 15 |
| idle | 120 | 0.65625 | 7,241,728 | 26,324,992 | 15 |
| idle | 180 | 0.65625 | 7,114,752 | 26,259,456 | 11 |
| idle | 240 | 0.65625 | 7,114,752 | 26,259,456 | 11 |
| idle | 300 | 0.65625 | 7,389,184 | 26,656,768 | 13 |
| popup shown (20th show) | 354 | 0.875 | 12,234,752 | 33,570,816 | 14 |
| hidden, 5 s after 20 shows | 360 | 0.875 | 12,234,752 | 33,574,912 | 14 |

    windows at t=9:   class=Window Class visible=False rect=420x560 | class=tray_icon_app visible=False | class=Winit Thread Event Target visible=True rect=15x15 | class=IME visible=False
    windows at show 20: class=Window Class visible=True rect=420x560 | ... (same, plus a hidden MSCTFIME UI)

- Idle CPU t = 9 to 300 s: 0 s over 291 s (the process CPU time did not change). NFR-04 met.
- Private bytes hidden: 7.11 to 7.40 MB = 6.8 to 7.1 MiB. After 20 shows: 12.23 MB = 11.7 MiB, the same shown and hidden. NFR-05 (<= 40 MiB) met with or without use. 20 shows do not prove there is no leak over thousands of shows.

## R2: popup latency, 50 shows (software renderer)

Start line `renderer=software`; 50 `perf what=popup_shown` events for 50 shows.

    raw ms (50 trials, src=ipc): 36.4, 11.7, 11.9, 13.3, 8.9, 12.1, 8.4, 18.4, 11.4, 8.4, 17.4, 13.6, 10.8, 8.3, 13.0, 8.2, 11.2, 26.2, 13.4, 13.6, 14.0, 15.8, 12.0, 12.7, 6.1, 12.7, 11.0, 15.3, 9.6, 16.0, 12.6, 13.2, 14.0, 15.4, 11.3, 9.8, 14.6, 13.5, 13.4, 8.0, 10.1, 12.6, 11.0, 11.8, 13.0, 11.5, 9.1, 15.7, 8.0, 9.5
    min 6.1 ms  p50 12.1 ms  p95 18.4 ms  max 36.4 ms

The first value (36.4 ms) is the first show after start (the OS window is created then); it is inside the 50. NFR-01 (<= 100 ms p95) met. Process private bytes in this run: 7,294,976 at t=8, 12,353,536 after 50 shows.

## R3: `SLINT_BACKEND=winit-femtovg` set by the user (override check)

Start line: `start version=0.0.1 renderer=env`. Private bytes 54,444,032 (51.9 MiB) at t=8 and 76,726,272 (73.2 MiB) hidden after 20 shows, the Task 209 GPU figures: the variable wins over the app's choice. 20 `popup_shown` events for 20 shows (the new timing also works with the GPU renderer): min 7.0, p50 11.1, p95 28.1, max 72.3 ms (the first show, 72.3 ms, includes creating the GPU surface).

    raw ms (20 trials, src=ipc): 72.3, 18.5, 11.1, 11.5, 9.1, 8.4, 14.7, 23.1, 28.1, 8.8, 8.2, 20.3, 8.4, 7.0, 19.5, 15.0, 10.7, 14.2, 11.0, 10.2

## Search timing in the popup (`perf what=search_done`)

New content-free event `perf what=search_done ms=<n>`: from the moment the 60 ms typing debounce runs the search of a typed query to its results being applied to the list (rows rebuilt, state synced); no query, no counts. It is not measured here: the IPC has only toggle, settings and quit, so nothing can type a query without simulating input (GC-19). 0 events in R1 to R3, as expected. Owner manual row below.

## Not covered (owner manual rows)

1. Look with the software renderer, at 100 %, 150 % and 200 % display scaling (Settings, System, Display, Scale; sign out if Windows asks): open the popup (Ctrl+Shift+V), the history window (tray, History...), Settings (tray, Settings...) and the welcome (tray, Start capturing... while capture is off). Record for each: is text crisp (not blurry or doubled), are icons and chips drawn, are rounded corners and the selection highlight right. Then seed a test profile (`dark-clipboard --seed-demo-clips 10000 --data-dir C:\temp\dc-test`, then `dark-clipboard --data-dir C:\temp\dc-test`), open the popup and the history, and scroll the 10,000 clips with the mouse wheel and with Page Down held: record any tearing, flicker or stutter. Pass: text as sharp as other apps at the same scale, no tearing. Compare once with `set SLINT_BACKEND=winit-femtovg` before starting, if something looks wrong.
2. Search timing: with the test profile above running, open the popup and type 20 different queries (one word each, pausing after each until the list updates), quit with `dark-clipboard --quit --data-dir C:\temp\dc-test`, then run `Select-String -Path C:\temp\dc-test\logs\dark-clipboard.log -Pattern 'perf what=search_done'` and write down the 20 values. Budget: NFR-02 <= 30 ms p95 per keystroke (the 60 ms debounce is outside this number). Delete `C:\temp\dc-test` afterwards.
3. The real-shortcut NFR-01 row (Task 204 section) and the IME and Narrator rows (Task 209 section) still apply, now with the software renderer.

Method notes: `--quit` does not stop a `--headless` process (headless has no instance pipe, so there is nothing to receive the command); stop it with Ctrl+C or end the process. A measurement script that starts `--headless` must kill it itself.

# Search bench re-run, 5 times (Task 407, F-21)

Date: 2026-11-24. Same machine (Windows 11 Home Single Language 10.0.26300, Intel Core Ultra 7 155H, 22 logical CPUs, 32 GB). Commit 9e60487 (`main` at bda38a5 plus a docs commit), rustc 1.100.0. Synthetic data only; the clipboard is not involved.

**The machine was not idle.** Before the runs `Win32_Processor.LoadPercentage` was 26 % (a camera client, a Python process, several editor/agent processes); right after them five samples 1.5 s apart read 0, 1, 1, 71, 67 %. Run 1 took 80 s against 42 to 62 s for the others. Treat these numbers as an upper bound under load, not as the idle figure F-21 asks for.

Command, five times in a row, nothing else started meanwhile (the test binary was built first with `cargo test -p darkclip-storage --release --lib --no-run`):

    cargo test -p darkclip-storage --release --lib -- --ignored --test-threads=1 --nocapture bench_search

This runs both `bench_search` (worst case: 30-word vocabulary, almost every clip matches) and `bench_search_realistic` (2,000-word Zipf vocabulary). 10,000 clips of 50 to 2,000 bytes, 200 queries each. Budget NFR-02: p95 <= 30 ms (the test asserts it).

| Run | `bench_search` p50 | `bench_search` p95 | `bench_search_realistic` p50 | `bench_search_realistic` p95 |
|---|---|---|---|---|
| 1 | 20.76 ms | **31.70 ms** (over) | 4.86 ms | 22.21 ms |
| 2 | 21.70 ms | 29.12 ms | 6.30 ms | 29.58 ms |
| 3 | 23.41 ms | **32.04 ms** (over) | 7.05 ms | 28.78 ms |
| 4 | 22.12 ms | 29.55 ms | 8.21 ms | **30.35 ms** (over) |
| 5 | 23.27 ms | **30.19 ms** (over) | 7.03 ms | 29.22 ms |
| Median | 22.12 ms | 30.19 ms | 7.03 ms | 29.22 ms |

Exact output lines (other test-harness lines omitted):

    run 1: bench_search: p50=20.7567ms p95=31.7028ms                 (FAILED, p95 31.7028ms)
           bench_search_realistic: p50=4.8571ms p95=22.2077ms (avg clip 1029 bytes, 0 of 200 queries empty)
    run 2: bench_search: p50=21.7002ms p95=29.1169ms
           bench_search_realistic: p50=6.2986ms p95=29.5826ms (avg clip 1029 bytes, 0 of 200 queries empty)
    run 3: bench_search: p50=23.4073ms p95=32.0391ms                 (FAILED, p95 32.0391ms)
           bench_search_realistic: p50=7.0451ms p95=28.7776ms (avg clip 1029 bytes, 0 of 200 queries empty)
    run 4: bench_search: p50=22.1166ms p95=29.5549ms
           bench_search_realistic: p50=8.2077ms p95=30.3514ms (avg clip 1029 bytes, 0 of 200 queries empty)   (FAILED, p95 30.3514ms)
    run 5: bench_search: p50=23.2674ms p95=30.1926ms                 (FAILED, p95 30.1926ms)
           bench_search_realistic: p50=7.0268ms p95=29.2234ms (avg clip 1029 bytes, 0 of 200 queries empty)

Reading: under this load the worst-case corpus sits right at the budget (p95 29.1 to 32.0 ms, 3 of 5 over) and the realistic corpus just under it (22.2 to 30.4 ms, 1 of 5 over). The Task 209 run on the same machine gave 19.8 / 19.1 ms and Task 013 gave 24 to 25 ms worst case, so the spread is mostly machine load. F-21 stays open: an idle-machine run (owner, or a CI runner) and the UI number `perf what=search_done` (owner manual row 2 above) decide NFR-02. Not a security finding.
