# Changelog

All notable changes to Dark Clipboard, newest first. Versions follow [SemVer](https://semver.org/). Sections per release: Added, Fixed, Known issues.

## [Unreleased]

### Added
- Phase 0 blueprint: product vision, feature matrix, MVP scope, user journeys, system architecture, platform capability matrix, data model with JSON Schemas and examples, threat model, privacy design, AI/MCP interface draft, ADR-0001 to ADR-0006, roadmap and Phase 1–5 plans, Phase 1 task packages.
- Feasibility spikes on Windows (clipboard monitoring; Tauri 2 vs Slint shell). These are throwaway code under `spikes/`.
- Agent workflow (`CLAUDE.md`, `AGENTS.md`, `.claude/`), memory-bank, quality gate scripts, CI workflow.

### Added (Phase 1 Foundation)
- Portable Rust workspace: `darkclip-core` (models, capture policy and privacy markers, classification and hashing, sensitive-data detection, i18n en/vi), `darkclip-platform` (adapter traits, capabilities, fake platform), `darkclip-storage` (SQLite + FTS5 with accent-insensitive search, content-addressed blobs, retention, migrations), `darkclip-app` (single capture actor, Application API, settings). Headless only: no OS adapter or UI yet.
- Quality gates: banned-crate, privacy-boundary and logging checks; cargo-deny.

### Known issues
- No application build exists yet (no OS adapter, no UI).
- macOS and Linux behaviour is untested.
