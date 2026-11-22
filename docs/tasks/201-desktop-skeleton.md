# Task 201: Desktop app skeleton (Slint, single instance, App wiring)

**Agent:** `builder` · **Model:** sonnet · **Branch:** `task/201-desktop-skeleton` · **Review:** `reviewer` (opus; threading and IPC)
**Global Constraints:** GC-05 (no async runtime), GC-06, GC-10, GC-14, GC-18. **Plan:** Phase 3 Task 201; ADR-0001 (Slint), ADR-0006 (process model), D-034, D-037 (dark only), D-040.

## Read first
`apps/desktop/src/{main.rs, log.rs}` (Task 106 headless binary), `crates/darkclip-app/src/api.rs` (`App`, `ClipApi`), `docs/spikes/ui-shell-comparison.md` (Slint 1.19, measured), `docs/architecture/system-overview.md` §1a and §2. The Slint spike may be read for API names only; never copied (CLAUDE.md).

## Files
`apps/desktop/{Cargo.toml, build.rs, ui/*.slint, src/main.rs, src/ui/mod.rs, src/single_instance.rs, src/theme.rs}`; i18n strings in `crates/darkclip-core/src/i18n.rs`.

## Requirements
1. `slint` (latest 1.x, `cargo add`), `.slint` files compiled in `build.rs`. Release profile unchanged.
2. Modes: `dark-clipboard` (default) starts the UI app **hidden** (tray comes in 202); `--headless` keeps Task 106 behaviour; `--toggle` asks a running instance to toggle the popup; `--data-dir` and `--exit-after-ms` keep working in both modes.
3. **Single instance:** a per-user named mutex (`Local\DarkClipboard-<user SID hash>` or similar; no user name in the name) plus a local IPC channel (Windows named pipe restricted to the current user, ACL = current user SID only) carrying only a small fixed command enum (`Toggle`, `ShowSettings`, `Quit`). A second instance forwards its command and exits 0. Unknown or oversized messages are dropped. No network. (This is the IPC base ADR-0006 needs later; keep the protocol versioned: 1 byte version + 1 byte command.)
4. **Threading:** the Slint event loop owns the main thread. All `App` calls that touch storage run on a worker thread; results return with `slint::invoke_from_event_loop`. The UI thread never blocks on storage, capture or IPC.
5. **Theme:** `theme.rs`/a Slint global with Darkshot's dark palette **values** (read colours from the Darkshot repo `D:\2026 - Tuan\shotme` read-only; never modify it; never copy code). Dark only (D-037). Font: system UI font.
6. Every user-visible string from `darkclip_core::i18n` (en + vi); UI language follows the setting (default: OS UI language if Vietnamese, else English).
7. Logging stays content-free (S-10 rule covers `apps/*/src`).

## Tests
- Arg parsing for the new flags; IPC message encode/decode (version, unknown command, oversize → dropped); single-instance logic behind a small trait so it is testable without a second process, plus one integration test that starts the binary twice with `--data-dir <tmp> --exit-after-ms` and asserts the second exits 0 after forwarding (no clipboard writes).
- Do NOT run `#[ignore]` tests; never write the real clipboard; never simulate input.

## Not doing
Tray (202), shortcut (203), popup content (204).
