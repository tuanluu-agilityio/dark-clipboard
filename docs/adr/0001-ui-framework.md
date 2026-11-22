# ADR-0001: Desktop UI framework

- Status: **Accepted (conditional)**, 2026-11-22. Conditions are listed under "Revisit triggers".
- Owner approval: 2026-11-22 (D-034). Licence decided: GPLv3.
- Deciders: Phase 0 architecture (owner approval pending)
- Evidence: `docs/spikes/ui-shell-comparison.md`

## Context

The desktop shell must provide a tray/menu-bar icon, a global shortcut, a floating always-on-top popup that appears in ≤ 100 ms, keyboard-only navigation, start at login, and a settings window, on Windows, macOS and Linux. It is resident all day, so idle memory and CPU matter. The brief preferred Tauri 2 "only if technical spikes prove it satisfies the requirements". Darkshot uses raw Win32 and is Windows-only, so it offers no reusable UI layer.

## Decision

Use **Slint** (1.x, Rust API, `.slint` markup) for the desktop UI in `apps/desktop`. Use `global-hotkey` for shortcuts, `tray-icon` for the tray on Windows and macOS, and `ksni` (StatusNotifierItem over D-Bus) on Linux, subject to the Phase 4 spike.

The UI is a thin client of the Application API (`darkclip-app`). No domain logic lives in `.slint` files or UI callbacks. Replacing the UI framework later means rewriting only `apps/desktop`.

## Alternatives

| Option | Verdict | Reason |
|---|---|---|
| Tauri 2 + WebView | **Rejected for P0** (measured) | Hidden-popup steady state: 160 MiB private / 372 MiB working set across 7 processes, versus 49 MiB / 85 MiB in 1 process for Slint. Setup 1.7 s versus 0.2 s. Destroying the webview on hide would break the popup latency budget. Tray, shortcut, always-on-top and show/hide all worked, so it is a viable fallback |
| GPUI (Zed) | Rejected (not prototyped) | Pre-1.0, API churn tied to Zed releases. Windows and Linux support are recent. No tray story. Small community outside Zed |
| egui / eframe | Rejected (not prototyped) | Immediate mode suits tools more than a polished popup; IME and accessibility weaker; no tray |
| iced | Rejected (not prototyped) | Multi-window and tray need extra crates; still pre-1.0 |
| Native per OS (Win32, AppKit, GTK) | Rejected | Three UI codebases; contradicts "one product, three platforms" for a small team |
| Qt (via cxx-qt) | Rejected | C++ toolchain and Qt licensing and distribution weight; CopyQ already occupies this niche |

## Consequences

- Positive: one native process, a small resident footprint, Rust end to end, no JS toolchain for the app (Darkshot's website keeps its own toolchain).
- Negative: no HTML rendering, so rich-text clips preview as plain text (acceptable: the preview is plain in P0 anyway). Fewer ready-made plugins (autostart, updater), so these are implemented in the platform adapters, which Darkshot already does on Windows (`autostart.rs` pattern).
- **Licensing:** Slint is available under GPLv3, a Royalty-free License (requires attribution, for desktop apps) or commercial. Dark Clipboard has no `LICENSE` yet. The owner must pick before the first public release: GPLv3 for the whole app, or a permissive app licence plus Slint's royalty-free licence with attribution in About and README. Tracked as open question Q-01 in `memory-bank/activeContext.md`.
- Risk R-06 (Linux tray, GTK loop) and R-07 (macOS panel focus) are handled by Phase 4 spikes.

## Revisit triggers

Re-open this ADR if any of these is measured in Phase 3/4:
1. Slint cannot show the popup in ≤ 100 ms p95 on any target OS.
2. IME input (Vietnamese Telex, Japanese) fails in `LineEdit` on any target OS.
3. A non-activating popup that returns focus to the previous app is impossible on macOS with Slint's winit backend.
4. Screen-reader access to the clip list is not achievable.
In that case Tauri 2 (already proven on Windows for tray, shortcut and popup) is the fallback, accepting the memory cost.
