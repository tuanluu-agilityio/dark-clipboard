# Phase 0 spikes (throwaway)

Small, isolated proof-of-concept projects that answer feasibility questions. Each spike is its own Cargo project (with an empty `[workspace]` table, so it is **not** part of the product workspace) and has its own `target/` (git-ignored).

Rules: never import, depend on or copy this code into `crates/` or `apps/`. Re-implement from task packages. Results are written up in `docs/spikes/`.

| Spike | Question | Write-up | Run |
|---|---|---|---|
| `win-clipboard-monitor/` | Windows listener, exclusion markers, bursts, OLE, source app, self-write suppression | `docs/spikes/windows-clipboard-monitor.md` | `cargo run` (overwrites the clipboard briefly, restores text) |
| `tauri-shell/` | Tauri 2: tray, global shortcut, always-on-top popup; memory and startup | `docs/spikes/ui-shell-comparison.md` | `cargo build --release`, then run `target/release/tauri-shell-spike.exe 15` |
| `slint-shell/` | Slint: same checks as above | `docs/spikes/ui-shell-comparison.md` | `cargo build --release`, then run `target/release/slint-shell-spike.exe 15` |

Spikes log to `%TEMP%\dark-clipboard-*-spike.log`.
