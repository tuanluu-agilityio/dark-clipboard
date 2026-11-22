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
