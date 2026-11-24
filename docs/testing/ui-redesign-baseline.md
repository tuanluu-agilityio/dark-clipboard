# UI redesign baseline (Task 700): P4 memory and P6 binary size

Budgets (`docs/specs/ui-redesign/05-acceptance.md`): P4 <= baseline + 4 MiB, P6 <= baseline + 300 KiB. Measured on Windows 11, release build, Slint software renderer (the default since Task 212). Same machine, same session, same script for both columns.

## Method (GC-19)

1. `cargo build --release -p dark-clipboard`; the exe was copied to a scratch folder (`baseline.exe` from the commit before Task 700, `after.exe` from the Task 700 branch).
2. Per run: a fresh temp `--data-dir` with a `settings.json` written without a BOM (`settings_version: 1`, `onboarding_done: true`, the `docs/architecture/examples/privacy-policy.default.json` policy with `capture_enabled: false`, so the real clipboard is never read), then
   `dark-clipboard.exe --seed-demo-clips 10000 --data-dir <dir>`, then
   `dark-clipboard.exe --data-dir <dir> --no-global-shortcut --exit-after-ms 120000`.
3. After the log said `running`: wait 8 s, read private bytes (`Process.PrivateMemorySize64`) = "hidden t=8 s". Then `dark-clipboard.exe --toggle --data-dir <dir>` (show the popup), wait 2 s, read = "popup shown". Then `--toggle` again (hide), wait 2 s, read = "hidden after popup". `--quit`, delete the dir (deletion confirmed, no `settings.json.corrupt*` file in any run).
4. No screenshot, no input simulation, the clipboard was never written. The script was a throwaway PowerShell file (not in the repo).

**Limit:** the history window and the settings window can only be opened from the tray menu or the popup (mouse or keyboard input), which agents may not simulate. So "after opening popup, history and settings once each" was measured with the popup only. The owner row in `platform-checklist.md` covers the other two windows.

## Raw numbers (bytes)

| Build | Run | hidden t=8 s | popup shown | hidden after popup |
|---|---|---|---|---|
| baseline | 1 | 9,928,704 | 12,754,944 | 12,754,944 |
| baseline | 2 | 9,998,336 | 12,828,672 | 12,980,224 |
| baseline | 3 | 10,018,816 | 12,738,560 | 12,738,560 |
| after (Task 700) | 1 | 10,039,296 | 12,640,256 | 12,640,256 |
| after (Task 700) | 2 | 10,031,104 | 12,787,712 | 12,787,712 |
| after (Task 700) | 3 | 9,748,480 | 13,283,328 | 13,283,328 |

Means after the popup was shown: baseline 12,774,059 (12.18 MiB), after 12,903,765 (12.31 MiB): **+0.12 MiB**, inside the run-to-run spread (baseline runs differ by up to 0.15 MiB, after runs by 0.6 MiB). Budget +4 MiB: met.

## P6: release exe size

| Build | Bytes |
|---|---|
| baseline (`main` at c522eff) | 13,105,664 (12.50 MiB) |
| after (Task 700) | 13,142,528 (12.53 MiB) |
| difference | **+36,864 bytes (36 KiB)**, budget +300 KiB: met |

The 43 Lucide SVGs total 19 KB of source; the kit and tokens add little code. The kit is not on screen yet except two popup buttons, so P4 will be measured again by each redesign task that adds windows.
