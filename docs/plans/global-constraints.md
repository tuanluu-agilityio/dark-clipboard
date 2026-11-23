# Global Constraints (apply to every task in every phase)

Every task package links here. A reviewer treats a violation as **blocking**.

## Architecture

- GC-01 `darkclip-core`, `darkclip-platform`, `darkclip-storage`, `darkclip-app` must not depend on `windows`, `windows-sys`, `objc2*`, `core-foundation*`, `x11rb`, `wayland-*`, `gtk*`, `slint`, `tauri*`, `tokio`. `check.ps1` enforces this with `cargo tree`.
- GC-02 `darkclip-core` performs no I/O: no filesystem, no network, no clock (time comes through the `Clock` trait), no environment variables. OS entropy for the random bits of UUID v7 ids (`uuid` → `getrandom`) is allowed; ids are therefore not deterministic (D-017).
- GC-03 Only the capture actor, `darkclip-app/src/capture/` (any file in that directory), may call `ClipboardAccess::read` or `ClipboardAccess::write`, and only after a `Decision::Read` from `darkclip_core::policy::decide_pre_read` (for reads). `check.ps1` enforces this. (Changed by D-044, 2026-11-23: `capture.rs` was split into the `capture/` directory by Task 200.)
- GC-04 Adapters contain no policy decisions (they report markers; they never skip).
- GC-05 No async runtime in P0/P1. Threads and `std::sync::mpsc` only.

## Privacy and security

- GC-06 Never log, print, `Debug`-format, or include in error messages: clip content, previews, file paths from `files` clips, search queries, window titles. Model types implement `Debug` manually. No `println!`, `eprintln!`, `dbg!` or `log`/`tracing` macros in any file under `darkclip-app/src/capture/`, `darkclip-core/src/sensitive/`, `darkclip-core/src/policy.rs` (check enforced). (Changed by D-044, 2026-11-23: was `darkclip-app/src/capture.rs` before Task 200.)
- GC-07 The mandatory marker list (`docs/security/privacy-design.md` §3) is never made configurable. No flag, setting, env var or debug mode bypasses it.
- GC-08 No network access in any crate in P0/P1. No telemetry. No crash upload. (Changed by D-057, 2026-11-23: the only exception before 0.1.0 is the opt-in update check of Task 412, in `apps/desktop` only, off by default; crates under `crates/` stay network-free.)
- GC-09 Test data is synthetic. Secret-shaped test strings are assembled at runtime (`concat!`/`format!`) so secret scanners do not fire (Darkshot practice).
- GC-19 Agents work on the owner's live desktop: never take screenshots or screen captures, never read other windows, never simulate keyboard or mouse input, never write the real clipboard, and never run the app against the default data dir. UI checks are owner manual rows. (Added by D-048, 2026-11-23, after a screen capture in Task 204 caught part of another window.)

## Code quality

- GC-10 No `unwrap`/`expect`/`panic!`/indexing that can panic in non-test code of core, platform, storage, app and adapter crates (`#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic)]` at crate roots, `cfg_attr(test, allow(...))`). The release profile uses `panic = "abort"`.
- GC-11 TDD: tests in the task package are written first and seen failing. Tests are never weakened to pass.
- GC-12 Add crates with `cargo add`. Do not add crates outside the task package's list without stating why in the report.
- GC-13 `unsafe` blocks are minimal, each with a `// SAFETY:` line; every OS handle is released on every path.
- GC-14 All user-visible strings come from `darkclip_core::i18n` (English and Vietnamese, compile-time complete).
- GC-15 `cargo fmt --all`, then `powershell -File scripts/check.ps1` (Windows) or `bash scripts/check.sh` (macOS/Linux) must print `ALL CHECKS PASSED` before a task is reported done.

## Process

- GC-16 One task = one branch `task/NNN-short-name`, one implementing agent, one different reviewing agent. The implementer edits only the files listed in the package; anything else is reported with a reason.
- GC-17 No work on a later phase's tasks, and no P2 features before 0.1.0.
- GC-18 Docs: a change in user-visible behaviour updates `README.md` and `CHANGELOG.md` (Unreleased) in the same task.
