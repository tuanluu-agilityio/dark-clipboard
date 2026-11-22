# Technical context

Updated: 2026-11-22

## Development environment (Phase 0 machine)
- Windows 11 Home 10.0.26300 x64. Project folder `D:\2026 - Tuan\dark-clipboard` (the path contains spaces: always quote it).
- Rust 1.100.0 stable, `x86_64-pc-windows-msvc`, clippy and rustfmt installed. `cargo` lives in `%USERPROFILE%\.cargo\bin`.
- Node 24 (schema validation tooling only).
- No WSL, no macOS, no Linux machine available in Phase 0.
- Darkshot repo: `D:\2026 - Tuan\shotme` (read-only for this project).

## Commands
| Purpose | Command |
|---|---|
| Quality gate (Windows) | `powershell -File scripts/check.ps1`, which must print `ALL CHECKS PASSED` |
| Quality gate (macOS/Linux) | `bash scripts/check.sh` |
| Format | `cargo fmt --all` |
| Schema validation only | `node tools/schema-check/validate.mjs` |
| Regenerate schemas | `python scripts/gen_schemas.py` (then re-run validation) |
| Add a dependency | `cargo add -p <crate> <dep>` |
| Supply-chain check | `cargo deny check` (cargo-deny 0.22.0 installed locally; also runs in CI) |

## Planned dependencies (decided in ADRs; versions are pinned by Cargo.lock when added)
| Crate | Where | Why |
|---|---|---|
| `serde`, `serde_json`, `thiserror` | core and up | models, JSON, errors |
| `uuid` (v7) | core | sortable ids |
| `sha2` | core | content hashing |
| `rusqlite` 0.43 (bundled SQLite 3.54.1) | storage | SQLite + FTS5 |
| `png` | storage/app | thumbnails, DIB→PNG |
| `windows` 0.62 | darkclip-windows | Win32 clipboard (spike-proven) |
| `objc2-app-kit` | darkclip-macos | NSPasteboard (Phase 4) |
| `x11rb`, `wayland-client`, `wayland-protocols(-wlr)` | darkclip-linux | X11/Wayland (Phase 4) |
| `slint` 1.x, `global-hotkey`, `tray-icon` (Win/macOS), `ksni` (Linux) | apps/desktop | UI shell (ADR-0001) |

## Constraints
- Release profile: `opt-level="z"`, `lto=true`, `codegen-units=1`, `panic="abort"`, `strip=true` (Darkshot).
- No network crates in P0/P1. No async runtime.
- `cargo deny` policy in `deny.toml` (Task 001).
