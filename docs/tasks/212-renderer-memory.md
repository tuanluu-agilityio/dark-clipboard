# Task 212: Software renderer by default, settings BOM, UI timing events

**Agent:** `specialist` · **Model:** opus · **Branch:** `task/212-renderer-memory` (from `main`) · **Review:** `reviewer` (opus)
**Global Constraints:** GC-06, GC-10, GC-14, **GC-19**. **Source:** Task 209 measurements (`docs/testing/perf-windows.md`, "ADR-0001 revisit check (Task 209)" and its Analysis), decision D-055.

## Findings this task acts on
- With the default (GPU, femtovg/OpenGL) renderer the UI process uses 52 MiB private bytes hidden and 72 MiB after 20 shows (NFR-05 target ≤ 40 MiB), and one renderer-related thread burns ~0.2 % of a core while idle (NFR-04 ≈ 0 %).
- With `SLINT_BACKEND=winit-software` the same build uses 6.9 MiB hidden, 11.7 MiB after 20 shows and 0 idle CPU.
- A `settings.json` written with a UTF-8 BOM is rejected as corrupt, renamed and replaced by a first run (all settings lost).
- The NFR-01 timing uses Slint's rendering notifier, which the software renderer does not provide; UI-path search latency has no timing event.

## Requirements
1. **Renderer.** On Windows the app selects the Slint software renderer (winit backend, software renderer) by default, at start, before any window is created. `SLINT_BACKEND` set by the user still wins (documented override). If the software renderer cannot start, fall back to the default backend and log a static `renderer_fallback` event. Record the chosen backend in the start log (`renderer=software|default`). Check the Cargo features: keep only what is needed, so that the binary does not grow; state any feature change and run `cargo deny check`.
2. **Timing without the rendering notifier.** NFR-01 `perf` timing must work with the software renderer: measure from the show request to the first completed frame using a mechanism that exists for the software renderer (for example a one-shot after the window becomes visible and the event loop has processed the next frame, or Slint's window-shown/redraw hook; state what is used and what it measures). Keep the log line format so `scripts/measure-popup.ps1` keeps working.
3. **Search timing.** Add a content-free `perf what=search_done ms=<n>` event in the popup: from the keystroke that changed the query (after debounce) to the results model being applied. No query text, no counts that reveal content.
4. **Settings BOM.** `Settings::load` accepts a UTF-8 BOM at the start of `settings.json` (strip it before parsing). Saving never writes a BOM. Test: a file with a BOM loads as `Loaded` with its values.
5. **Headless quit (doc).** `--quit` does not stop a `--headless` process (no IPC there): document this in `README.md` and `docs/testing/perf-windows.md` method notes; no code change.
6. **Re-measure** (platform-verifier rules, GC-19): release build, fresh temp `--data-dir` with a pre-written `settings.json` (no BOM, `onboarding_done: true`), `--seed-demo-clips 10000`, `--no-global-shortcut`, `--exit-after-ms`; idle CPU and private bytes hidden for 5 min; private bytes after 20 `--toggle` shows; `measure-popup.ps1` p50/p95 with the new timing; 20 synthetic search timings if the IPC can drive a query, otherwise state that search timing needs an owner row. Record raw numbers in `docs/testing/perf-windows.md`.
7. Owner manual rows (checklist): the popup, history, settings and welcome look right with the software renderer (text crisp at 100/150/200 % DPI, no tearing when scrolling 10k clips), and IME/Narrator rows still apply.

## Tests
- Unit: backend selection logic (env override, Windows default, fallback) as a pure function; BOM load test.
- All existing tests pass unchanged. Do not run `#[ignore]` tests.

## Not doing
Changing ADR-0001 (the framework stays Slint; this is a renderer configuration). macOS/Linux renderer choice (Phase 4).
