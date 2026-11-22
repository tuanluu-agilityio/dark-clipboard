# Task 200: Split `capture.rs` into modules (no behaviour change)

**Agent:** `specialist` · **Model:** opus · **Branch:** `task/200-split-capture` (from `main`) · **Review:** `security-reviewer` (opus)
**Global Constraints:** GC-03 (blocking), GC-06 (blocking), GC-10, GC-11, GC-16. **Plan:** `docs/plans/phase-3-desktop-shell.md` Task 200; D-039.

## Read first
`crates/darkclip-app/src/capture.rs` (whole file), `scripts/check.ps1` + `scripts/check.sh` (GC-03 allowed-file pattern, GC-06 quiet-file pattern), D-027, D-031, D-038, D-042, D-043.

## Requirements
1. Move `crates/darkclip-app/src/capture.rs` into a module directory `crates/darkclip-app/src/capture/` with focused files, for example: `mod.rs` (public API: `CaptureHandle`, `CaptureConfig`, `ActorCommand`, re-exports), `actor.rs` (thread, run loop, commands), `pending.rs` (debounce, burst, supersede chain, heir, carried, excluded_recent), `pipeline.rs` (snapshot → `decide_pre_read` → read → classify → sensitive → decode/hash → store), `copy_back.rs`, `counters.rs`, and `tests/` split by topic. Names are your call; each file has one responsibility and none is over ~800 lines of non-test code.
2. **No behaviour change.** Public paths used by other modules and crates (`crate::capture::…`, `darkclip_app::…`) keep compiling unchanged. No test assertion changes; test code may move.
3. Update both check scripts: GC-03 allows `crates/darkclip-app/src/capture/.*` (instead of `capture.rs`); GC-06 quiet set covers every file under `crates/darkclip-app/src/capture/`. Keep the scripts equivalent. Prove each rule still fires with a planted line in a temp file under `capture/`, then remove it.
4. Update file references in docs that point to `capture.rs` only where they are normative (global constraints GC-03/GC-06 text, `docs/architecture/system-overview.md` crate notes) with a dated "Changed by D-044" note; do not rewrite history in task packages or decisions.

## Tests
Full workspace suite passes with the same counts as before the move (report before/after counts). `cargo test -p darkclip-app` three times in a row.

## Not doing
Any logic change, renames of public types, new features.
