# Windows headless performance (Task 106)

Date: 2026-11-22. Machine: Windows 11 Home Single Language 10.0.26300, the owner's work machine (in use during the run).
Binary: `cargo build --release -p dark-clipboard` (release profile: opt-level z, lto, panic abort), 1,783,296 bytes.
The clipboard was never written by these runs; the monitor only listened.

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
