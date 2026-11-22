# Spike: UI shell, Tauri 2 vs Slint (Windows)

Date: 2026-11-22 · Machine: Windows 11 Home 10.0.26300 x64 · Rust 1.100.0
Code: `spikes/tauri-shell/` (tauri 2.12.5, tauri-plugin-global-shortcut 2.x, WebView2) and `spikes/slint-shell/` (slint 1.19.0 default backend, tray-icon 0.26.1, global-hotkey 0.10.0). Both are throwaway and not workspace members.

Both spikes do the same thing: create a tray icon with a Quit menu, register `Ctrl+Shift+F12` globally, create an undecorated always-on-top 420×520 popup containing a search box and a list, show it, hide it, then idle until a timer exits at 15 s. Memory was sampled about 9 s after launch with the popup hidden (the steady state of a tray app). Same release profile in both: `opt-level="z"`, LTO, `codegen-units=1`, `panic="abort"`, `strip`.

## Results (single run each, measured)

| Metric | Tauri 2 | Slint 1.19 |
|---|---|---|
| Tray icon created | yes | yes |
| Global shortcut registered | yes | yes |
| Always-on-top flag | `true` (read back) | set in markup (not read back) |
| Show → visible, hide → hidden | yes / yes | yes / yes |
| Setup time (process start → end of setup) | **1,744 ms** | **196 ms** |
| Release exe size | **2.6 MiB** (+ system WebView2) | **8.0 MiB** |
| Processes | 7 (host + 6 `msedgewebview2.exe`) | 1 |
| Private bytes, popup hidden | **160 MiB** total (host 5.6 MiB, WebView2 154.6 MiB) | **49 MiB** |
| Working set, popup hidden | **372 MiB** total | **85 MiB** |
| Clean release build time (first, with downloads) | 4 min 3 s | about 4 min (2 min 52 s + 1 min 7 s after a markup fix) |

Not measured in Phase 0: popup show latency from shortcut to first frame, idle CPU over 10 minutes, behaviour on macOS and Linux, IME, screen readers, high-DPI multi-monitor. These are Phase 3 and Phase 4 tasks.

Caveats: one sample per framework. WebView2 memory is partly shared with Edge and other WebView2 apps on a machine that runs them; private bytes are still attributed to our process tree. Slint used its default renderer; the software renderer or the Skia renderer will change memory, untested.

## Interpretation

- A tray app is resident all day. Keeping a hidden webview alive costs about 110 MiB more private memory than Slint, plus 6 processes. Destroying the webview on hide would save memory but would break the 100 ms popup budget (NFR-01), since WebView2 setup alone was about 1.7 s cold here.
- Tauri's strengths (HTML/CSS iteration speed, rich HTML preview, mature tray/autostart/updater plugins, TypeScript skills from Darkshot's website) are real but do not outweigh a resident footprint more than 3× larger, for a product whose vision says "lightweight".
- Slint's risks move to integration: Linux tray (`tray-icon` needs a GTK loop; the alternative is `ksni` over D-Bus), macOS non-activating panel behaviour, focus return to the previous app, IME in `LineEdit`, and licensing (see ADR-0001).

Decision: ADR-0001.
