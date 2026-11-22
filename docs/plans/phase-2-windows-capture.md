# Phase 2: Windows Adapter and Headless Capture

> **For agentic workers:** detailed task packages (`docs/tasks/1NN-*.md`) are written at the start of the phase from this plan, after Phase 1 interfaces are merged. Interfaces named here come from Phase 1 Tasks 003, 004, 006, 010.

**Goal:** Real clipboard capture on Windows into the Phase 1 storage, verified against Darkshot and password managers, runnable headless (`dark-clipboard --headless`, a dev binary).
**Spec:** ADR-0002, ADR-0004, `docs/spikes/windows-clipboard-monitor.md`, `docs/security/privacy-design.md` §3.
**Global Constraints:** `docs/plans/global-constraints.md`. Especially GC-03, GC-04, GC-07, GC-13.

## Review Focus
1. An app that holds the clipboard open for 1 s: the actor retries and then drops; the message loop never blocks.
2. A `CF_DIBV5` with alpha, a top-down DIB (negative height), and a 1×1 image: correct PNG, no panic.
3. `HTML Format` with a non-ASCII fragment (byte offsets in its header): the plain text representation is taken from `CF_UNICODETEXT`, and HTML is stored raw.
4. Monitor thread exits cleanly on shutdown (listener removed, window destroyed) within the 2 s deadline.
5. Copy-back of a 20 MiB image while other apps write: no lost self-write suppression (seq race).

## Tasks

| Task | Title | Depends | Agent / model | Reviewer | Risk | Acceptance criteria | Tests |
|---|---|---|---|---|---|---|---|
| 101 | `darkclip-windows` crate: monitor thread (message-only window, `AddClipboardFormatListener`), `ChangeSignal` | P1 | specialist / opus | reviewer / opus | High | Signals for same/other-process writes; clean stop | `#[ignore]` integration tests that write via the crate's own writer and via `powershell Set-Clipboard`; unit tests for the window-proc dispatch |
| 102 | `snapshot_meta`: format enumeration, registered names, DWORD markers via `markers::windows_markers`, OLE placeholder detection | 101 | specialist / opus | security-reviewer / opus | **High** | Spike S2/S3 scenarios reproduced as tests; markers detected by comparing **format IDs** (`RegisterClipboardFormatW(known_name)` == enumerated id), then calling `markers::windows_markers` with the canonical name (D-021); no `GetClipboardData` on content formats in `snapshot_meta` (code review + test counting calls through a thin wrapper) | Integration (`#[ignore]`, real clipboard) + pure mapping unit tests |
| 103 | `read(plan)`: `CF_UNICODETEXT`, `HTML Format`, `Rich Text Format`, `CF_DIBV5`/`CF_DIB` → PNG, `PNG` registered format, `CF_HDROP` → `text/uri-list`; size caps via `GlobalSize` before copying | 102 | specialist / opus | reviewer / opus | High | Caps enforced before allocation; text buffers wiped after conversion | DIB fixtures (top-down, bottom-up, 32-bit alpha, 1×1, 16k wide → TooLarge) as pure unit tests |
| 104 | `write(reps)`: atomic multi-format write that returns the post-write sequence number. Copy-backs carry **no** exclusion markers (D-012; see the note below) | 103 | specialist / opus | reviewer / opus | Medium | Spike S8 behaviour as a test | Integration |
| 105 | `SourceResolver`: owner window → PID → image name; elevated/inaccessible → `None`; no window titles | 101 | builder / sonnet | security-reviewer / opus | Medium | `confidence = Owner`; never calls `GetWindowText` (check.ps1 rule added in this task) | Integration + check rule |
| 106 | Headless dev binary `apps/desktop` (`--headless`): wires adapter + app; content-free logging to file | 101–105 | builder / sonnet | reviewer / sonnet | Low | Runs for 10 min idle at ~0% CPU; Ctrl+C shuts down in ≤ 2 s | Manual measurement recorded in `docs/testing/perf-windows.md` |
| 107 | Windows verification: `docs/testing/platform-checklist.md` Windows column: real Darkshot (Copy image, Copy link), KeePassXC, Bitwarden, 1Password (if available), Office Word/Excel, Edge/Chrome HTML, Explorer files, RDP (if available) | 106 | platform-verifier / sonnet; owner runs manual steps | reviewer / opus | **High** (privacy promise) | Every row recorded T with date and app version, or "not available"; any marker missing → new excluded-app default and a decision entry | Manual checklist + `#[ignore]` automated parts |
| 108 | Phase review + memory-bank, CHANGELOG (Unreleased) | 101–107 | reviewer / opus | — | — | APPROVED | `check.ps1` |

Note on 104: Dark Clipboard's own copy-back must **not** add exclusion markers. Otherwise pasting a clip back would hide it from Win+V and other tools in ways the user did not ask for. Self-write suppression uses the sequence number, not markers. This is decided here (decision D-012 in `memory-bank/decisions.md`).
