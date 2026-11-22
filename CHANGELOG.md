# Changelog

All notable changes to Dark Clipboard, newest first. Versions follow [SemVer](https://semver.org/). Sections per release: Added, Fixed, Known issues.

## [Unreleased]

### Added
- Phase 0 blueprint: product vision, feature matrix, MVP scope, user journeys, system architecture, platform capability matrix, data model with JSON Schemas and examples, threat model, privacy design, AI/MCP interface draft, ADR-0001 to ADR-0006, roadmap and Phase 1–5 plans, Phase 1 task packages.
- Feasibility spikes on Windows (clipboard monitoring; Tauri 2 vs Slint shell). These are throwaway code under `spikes/`.
- Agent workflow (`CLAUDE.md`, `AGENTS.md`, `.claude/`), memory-bank, quality gate scripts, CI workflow.

### Added (Phase 2 Windows capture)
- Developer binary `dark-clipboard --headless` (Windows): runs the capture actor on the real Windows clipboard until Ctrl+C, with a content-free size-capped log. No UI yet.
- `App::capabilities()`; the Windows adapter reports read, write and privacy-marker support as native (real-clipboard tests ran in Task 107, see `docs/testing/platform-checklist.md`).
- The headless log records, on stop, one line of skip counts per reason (static reason names and numbers only).
- When storage is full, the capture actor runs retention once and retries the insert once before reporting the failure.
- Check rule S-10: log and print lines in the headless binary may not mention content-like words.

### Added (Phase 1 Foundation)
- Portable Rust workspace: `darkclip-core` (models, capture policy and privacy markers, classification and hashing, sensitive-data detection, i18n en/vi), `darkclip-platform` (adapter traits, capabilities, fake platform), `darkclip-storage` (SQLite + FTS5 with accent-insensitive search, content-addressed blobs, retention, migrations), `darkclip-app` (single capture actor, Application API, settings). Headless only: no OS adapter or UI yet.
- Quality gates: banned-crate, privacy-boundary and logging checks; cargo-deny.

### Fixed
- Windows: clipboard writes through OLE with a flush (for example Notepad) keep their source app, and an excluded app without privacy markers that writes this way is now skipped (F-14). Fixed in the capture actor; real-clipboard recheck pending.
- Windows: a screenshot-sized bitmap copy (for example 4K, offered only as a DIB) is stored when its PNG fits the image size limit; the limit applies to the stored PNG, and bitmap input is bounded separately (64 megapixels, 256 MiB decoded).
- Windows: copying back a stored image too large to decode safely now puts the PNG alone on the clipboard instead of decoding it; a copy-back retried while the clipboard is busy converts the image once.
- A clipboard change that replaced another before capture looked at it is no longer stored with the earlier app as its source.

### Known issues
- Windows verification covered KeePassXC 2.8.0 and Darkshot; Bitwarden, 1Password, Office and RDP are untested.
- No UI yet: only the Windows developer binary (`dark-clipboard --headless`).
- macOS and Linux behaviour is untested.
