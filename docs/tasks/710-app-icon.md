# Task 710: App icon (option A): exe, windows, tray

**Agent:** `builder` · **Model:** sonnet · **Branch:** `task/710-app-icon` (from `main`, after 700 is merged) · **Review:** `reviewer` (sonnet) · **Depends on:** 700
**Global Constraints:** GC-08, GC-11, GC-12, GC-14, GC-15, GC-17, GC-18, **GC-19**. **Source:** D-078 (owner chose option A). **Review Focus:** 4 (no key map changes), 6 (strings from i18n EN + VI, glyph scan passes). Every user-visible string comes from i18n (EN + VI).

## Read first
`CLAUDE.md`, `AGENTS.md`, `docs/specs/ui-redesign/02-design-system.md` §5b, `docs/specs/ui-redesign/mockups/app-icon.html` (symbol `A` and the three badges), `apps/desktop/src/tray.rs`, `apps/desktop/build.rs`, `apps/desktop/Cargo.toml`, `deny.toml`. Reference only (read, never modify): `D:\2026 - Tuan\shotme\crates\darkshot-windows\res\make_icon.py` and the `build.rs` in the same crate.

## Requirements
1. **Generator** `apps/desktop/res/make_icon.py` (Python + Pillow, dev-only, not run by the gate). Draws option A exactly as §5b: tile in a 56-unit box, rect 2,2,52,52 rx 13 fill `#202328`, 1.2 border `#3A3E45`; board rect 16,15,24,30 rx 4.5 stroke `#7887FF` w4, no fill; clip rect 21.5,10.5,13,8 rx 2.5 fill `#F3F5F9`; lines `M22 28h12 M22 35h8` stroke `#F3F5F9` w3.5 round caps. Outputs, all committed:
   - `dark-clipboard.ico` with sizes 16, 20, 24, 32, 40, 48, 64, 256 (256 as PNG, the others 32-bit DIB); 16/20/24 hand-tuned on the pixel grid (explicit pixel rects, not downscaled).
   - Tray PNGs per state (recording, paused/off, private, unavailable) at 16/20/24/32 in `apps/desktop/res/tray/`: same icon plus a bottom-right badge (circle r 12 at 43,43 with a 3-unit `#111318` ring): paused amber `#F2C14E` with pause bars, private `#A1AAB8` with struck eye, unavailable `#FF7A85` with "!". Recording has no badge.
   - `preview.png` (contact sheet) and a `self_check` like Darkshot's (ICO header and directory entries, sizes, PNG/DIB encoding, non-empty alpha, badge colour present per state); it runs at the end of the script and exits non-zero on failure.
2. **Exe icon.** `embed-resource` as a build-dependency of `apps/desktop`, Windows only (`cfg(windows)` in `build.rs`; `.rc` file in `res/`), as Darkshot's `build.rs`. Keep the existing Slint compile step. `cargo deny check` passes; record crate, version and licence in `memory-bank/techContext.md`.
3. **Window icon.** Set the `icon:` property on the popup, history, settings and welcome windows (`@image-url` of a PNG from the generator; builder picks 32 or 64 px and says why).
4. **Tray.** Replace the code-drawn `icon_rgba` in `apps/desktop/src/tray.rs` with the pre-rendered state images. Decode with the `png` crate already in the tree, or embed raw RGBA produced by the generator; builder decides and states why (no new crate for this). Pick the size closest to the system tray size (`GetSystemMetrics(SM_CXSMICON)`) among 16/20/24/32 where practical; the selection is a pure function `nearest_tray_size(px) -> u32`. Keep the pure `icon_id` state mapping and all its tests unchanged.
5. **Checklist.** Owner rows in `docs/testing/platform-checklist.md`: exe icon in Explorer, taskbar, Alt+Tab, window title bars; tray icon for each state on a light and a dark taskbar at 100 % and 150 % (the Windows icon cache may need an Explorer restart).
6. **CHANGELOG.** `[Unreleased]` -> Added: new app icon (exe, windows, tray state badges).

## Files this task may edit
`apps/desktop/res/*` (new), `apps/desktop/build.rs`, `apps/desktop/Cargo.toml`, `Cargo.lock`, `apps/desktop/src/tray.rs`, `apps/desktop/ui/{app,history,settings,welcome}.slint` (icon property only), `apps/desktop/tests/*` (new), `deny.toml` only if the new crate needs an entry (state why), `docs/testing/platform-checklist.md`, `memory-bank/techContext.md`, `CHANGELOG.md`. Nothing else.

## Tests (first, seen failing)
- `nearest_tray_size`: exact sizes, in-between values (nearest, ties up), values below 16 and above 32 clamp.
- Every tray state maps to an embedded image of the chosen size with the expected byte length (w x h x 4) and non-transparent pixels; `icon_id` tests unchanged.
- `res/dark-clipboard.ico` parses: 8 entries with the sizes above, the 256 entry is PNG, the others are DIB (header check in Rust, no extra crate).
- Existing tests pass unchanged. Never run `#[ignore]` tests.

## Acceptance
`dark-clipboard.ico` and tray PNGs committed and verified by `self_check` (record command and output in the report); the release exe builds with the icon resource; the app is launched only with `--data-dir <temp> --exit-after-ms`; `cargo deny check` passes; `cargo fmt --all`, then `powershell -File scripts/check.ps1` prints `ALL CHECKS PASSED`.

## Known pitfalls (from `memory-bank/systemPatterns.md`)
- **Windows-only code must lint on macOS/Linux** (CI clippy `-D warnings`): resource code in `build.rs`, the `SM_CXSMICON` lookup and any item used only under `cfg(windows)` need `cfg_attr(not(windows), allow(dead_code))` or their own `cfg`; the pure `nearest_tray_size` and the image tables compile everywhere.
- **No unwrap/expect/panic** in non-test code (`panic = "abort"`): a missing or corrupt tray image falls back to a default image, never panics.
- **Content-free observability** (GC-06): tray images are static; log nothing about clip data when choosing an icon.
- **UI symbols are never font glyphs** (Task 414 scan); the `.ico` and PNGs are images, not text.
- **Build scripts:** `build.rs` must still work where `embed-resource` is not built (non-Windows); offline tools need every locked crate fetched (PR #34); PowerShell scripts end with `exit 0`.
- **Software renderer (D-055):** window icons load once; no timers or animation added.
- **GC-19:** never launch the UI without `--data-dir <temp>` and `--exit-after-ms`, never screenshot; the visual check of the icons is an owner row.

## Not doing
Changing Darkshot files, new tray states or menus, installer or branding changes, a release.
