# Implementation Roadmap

Status: Phase 0 complete (this document), 2026-11-22. The phases are engineering phases. Product levels P0–P3 are defined in `docs/product/feature-matrix.md`; the mapping is below. Following Darkshot decision 105, the product roadmap is kept separate from task and version numbers.

| Phase | Name | Product level | Platforms | Exit criteria (all must hold) | Plan |
|---|---|---|---|---|---|
| 0 | Discovery and architecture | — | Windows spikes | Deliverables in `docs/`, ADR-0001…0006, spikes measured | this repo |
| 1 | Foundation: workspace, core, storage, app services (headless) | P0 base | CI on all three OSes (build + unit tests) | `check.ps1` / `check.sh` green on Windows, macOS, Linux CI; capture pipeline runs end to end against `FakePlatform`; search bench ≤ 30 ms p95 on 10k clips | `phase-1-foundation.md` |
| 2 | Windows adapter and headless capture | P0 | Windows | Real capture to DB on Windows; mandatory markers verified with Darkshot, KeePassXC, Bitwarden; self-write suppression; platform checklist Windows column at **T** | `phase-2-windows-capture.md` |
| 3 | Desktop shell (Slint) and P0 UX | P0 | Windows | Tray, shortcut, popup ≤ 100 ms p95, search UI, copy/paste back, pin/delete, pause/private, settings, retention, autostart, i18n en/vi; ADR-0001 revisit triggers checked → 0.1.0-alpha (internal) | `phase-3-desktop-shell.md` |
| 4 | macOS and Linux adapters | P0 | macOS, Linux X11, KDE/wlroots Wayland, GNOME (test only) | macOS and Linux matrix columns at **T** or explicitly marked unsupported; popup, tray, shortcut on each; packaging prototypes | `phase-4-macos-linux.md` |
| 5 | P1 organization, hardening, first public release | P1 + release | All | Collections, favorites, tags, notes, transforms, backup/restore; fuzzing of decoders; signing plan; README, CHANGELOG, website section → **0.1.0** public (Windows supported; macOS/Linux preview) | `phase-5-p1-release.md` |
| 7 | UI redesign: shared component kit, SVG icons, app icon, popup/history/settings rebuilt, keyboard model (*added 2026-11-24, Changed by D-078*) | P1 polish | Windows first (UI code platform-neutral) | Design `docs/specs/ui-redesign/` met; acceptance criteria `05-acceptance.md` (contrast, keys, Vietnamese fit, popup p95 ≤ 30 ms, hidden memory ≤ 9 MiB); phase review APPROVED | `phase-7-ui-redesign.md` |
| 6 | Translate in place: a hotkey that translates the selected or whole-field text with the user's Claude API key and pastes it back (*added 2026-11-27, D-093, D-094*) | P1 feature | Windows only | Design `docs/specs/translate-in-place/design.md` met; owner end-to-end rows done; runs **before Paste Stack (0.2)** | `phase-6-translate-in-place.md` |
| 6+ | P2: CLI, MCP, IPC, optional AI, rules, encryption at rest | P2 | All | Separate design + plan required (security review of MCP surface) | not planned yet |
| later | P3: E2E sync, mobile, Darkshot integration | P3 | — | Separate product decision | — |

## Sequencing rationale

1. Phase 1 has no OS dependence, so it can be built and reviewed in parallel task packages, and CI proves the "core is portable" claim on three OSes early.
2. Windows goes first because it is the only platform with evidence (spike) and the owner's daily machine (Darkshot is Windows-only).
3. The UI (Phase 3) comes after real capture (Phase 2), so the UI is built against real data and timing.
4. macOS and Linux (Phase 4) start with spikes on real machines, because they hold the most uncertainty: the GNOME data-control conflict, the macOS pasteboard privacy prompt, and the Linux tray loop.
5. P1 features land before the first public release, so 0.1.0 is useful for daily work. P2 (AI) is deliberately after it: "useful without an AI provider".

## Cross-phase risks (tracked in `memory-bank/activeContext.md`)

| ID | Risk | Likelihood | Impact | Mitigation / owner phase |
|---|---|---|---|---|
| R-01 | GNOME Wayland has no data-control → no capture | Medium | High (large Linux share) | Phase 4 test; clear "unavailable" UX; consider Shell extension later |
| R-02 | Password managers without markers on some OS | Medium | High | Built-in excluded apps; per-manager verification task |
| R-03 | macOS pasteboard privacy prompt on every read | Medium | High | Phase 4 spike on macOS 26; onboarding; type-detect APIs before reading |
| R-04 | Slint popup latency or IME issues | Low–Med | Medium | ADR-0001 revisit triggers; Tauri fallback proven on Windows |
| R-05 | Clipboard contention with Office/RDP | Medium | Low | Retries; drop with counter |
| R-06 | Linux tray needs GTK loop alongside winit | Medium | Medium | `ksni` (D-Bus) spike in Phase 4 |
| R-07 | macOS focus return after popup | Medium | Medium | Non-activating panel spike in Phase 4 |
| R-08 | Slint licence choice blocks release | Low | High | Owner decision Q-01 before Phase 5 |
| R-09 | Code signing cost/time (SmartScreen, Gatekeeper notarization) | High | Medium | Phase 5 plan; ship unsigned preview with checksums if needed (Darkshot precedent) |
| R-10 | Scope creep into AI before P0 is solid | Medium | Medium | Phase gates; CLAUDE.md rule "no P2 work before 0.1.0" |
