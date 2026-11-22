# Task 001: Workspace bootstrap and quality gates

**Agent:** `scaffolder` · **Model:** sonnet · **Branch:** `task/001-workspace-bootstrap` (from `main`) · **Review:** `reviewer` (sonnet)
**Global Constraints:** `docs/plans/global-constraints.md`, especially GC-01, GC-10, GC-15.

## Read first
`docs/architecture/system-overview.md` §1, `docs/plans/bootstrap-and-quality.md`, current `scripts/check.ps1`, `scripts/check.sh`, `.github/workflows/ci.yml`.

## Files
Create: `Cargo.toml` (workspace), `rust-toolchain.toml`, `rustfmt.toml`, `clippy.toml`, `deny.toml`, `crates/darkclip-core/{Cargo.toml,src/lib.rs}`, `crates/darkclip-platform/{Cargo.toml,src/lib.rs}`, `crates/darkclip-storage/{Cargo.toml,src/lib.rs}`, `crates/darkclip-app/{Cargo.toml,src/lib.rs}`.
Modify: `scripts/check.ps1`, `scripts/check.sh` (only where marked `# TASK-001`), `.github/workflows/ci.yml` (remove the `if: hashFiles('Cargo.toml')` guard), `.gitignore`.
Do not create adapter crates or `apps/desktop` (later phases).

## Requirements
1. Workspace: `members = ["crates/*"]`, `resolver = "2"`, `[workspace.package] edition = "2021", version = "0.0.1", license = "UNLICENSED"` (licence decision Q-01 pending), `publish = false`. `[workspace.lints.clippy]`: `unwrap_used = "deny"`, `expect_used = "deny"`, `panic = "deny"`; each crate has `[lints] workspace = true` and `#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used, clippy::panic))]` in `lib.rs`.
2. Release profile exactly like Darkshot: `opt-level = "z"`, `lto = true`, `codegen-units = 1`, `panic = "abort"`, `strip = true`.
3. `rust-toolchain.toml`: `channel = "stable"`, components `rustfmt`, `clippy`.
4. Crate dependency edges only: platform → core; storage → core; app → core, platform, storage. No external crates yet.
5. Each `lib.rs` contains a module doc comment stating the crate's responsibility and its "must not depend on" list (copy from system-overview §1), and one smoke test `fn crate_builds() {}`.
6. `deny.toml`: advisories deny, licences allow-list `MIT`, `Apache-2.0`, `Apache-2.0 WITH LLVM-exception`, `BSD-2-Clause`, `BSD-3-Clause`, `ISC`, `Unicode-3.0`, `Zlib`, `MPL-2.0`; sources: crates.io only.
7. `check.ps1` and `check.sh` already contain the gates; switch them from "skip when no workspace" to active by removing the `# TASK-001` skip blocks. Both must pass on the empty crates.

## Tests
- `cargo test --workspace` runs 4 smoke tests.
- `scripts/check.ps1` → `ALL CHECKS PASSED`.
- Negative check (do it, do not commit it): temporarily add `windows = "0.62"` to `darkclip-core`, run `check.ps1`, see `CHECK FAILED: darkclip-core depends on a banned crate`, then revert. Report the output.

## Not doing
Any domain code; adapters; UI.
