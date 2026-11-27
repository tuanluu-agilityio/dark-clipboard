# Task 606: Translate UI: overlay window, Settings → Translate, consent dialog, key entry

**Agent:** `builder` · **Model:** opus (raised from the plan's sonnet: the overlay is a Win32 no-activate window driven from Slint, and if it steals focus the paste breaks) · **Branch:** `task/606-translate-ui` (from `main`) · **Review:** `reviewer` (opus)
**Global Constraints:** GC-06, GC-07, GC-14, GC-18, GC-19, D-093 (consent), D-094, D-095. Plan: `docs/plans/phase-6-translate-in-place.md` (Interfaces §606). The prototype the owner approved is `docs/specs/translate-in-place/prototype.html`: its layout, its states and its **EN/VI text are the spec**. Also read the design `docs/specs/translate-in-place/design.md` §5.2 and §6.3, and the UI system `docs/specs/ui-redesign/02-design-system.md` plus the kit in `apps/desktop/ui/kit/`.

## Goal
Make the 605 flow usable without editing files, with three surfaces:
1. **Overlay:** a small window near the mouse cursor that **never takes focus**.
2. **Settings → Translate:** a new section with the consent dialog, key entry and a Test button.
3. Every string in i18n, in EN and VI.

## 1. Overlay window
- `ui/translate_overlay.slint` exports `TranslateOverlay`:
  - frameless, always on top, the kit tokens (`raised` fill, `border-strong` / `danger` / `warning` border by state);
  - properties `state: int` (0 sending, 1 done, 2 cancelled, 3 left-on-clipboard, 4 blocked, 5 failed), `message: string`, `detail: string`;
  - callback `cancel()`;
  - a 14 px spinner only in the sending state, with no animation when reduced motion is set (as in the prototype);
  - vector icons from `ui/icons` (no glyphs).
- Host `ui/translate_host.rs` implements `translate::host::OverlaySink`:
  - maps `OverlayState` to the i18n strings (table below) and places the window with `popup/place.rs` near `winsys::cursor_pos()` on that monitor;
  - auto-hides after 1.5 s (done or cancelled) or 4 s (blocked or failed);
  - while sending, shows until the next state;
  - every call to `show` from the flow thread goes through `slint::invoke_from_event_loop`.
  - The existing content-free log line stays.
- **No focus, ever (critical).** After the Slint window is created and before it is first shown, get its HWND through Slint's winit/raw-window-handle access (it is already in the dependency tree through `unstable-winit-030`). Then:
  - set `WS_EX_NOACTIVATE | WS_EX_TOOLWINDOW | WS_EX_TOPMOST`;
  - show it with `ShowWindow(SW_SHOWNOACTIVATE)`, or `SetWindowPos(.., SWP_NOACTIVATE | SWP_SHOWWINDOW)`, instead of a call that activates.
  If Slint's `show()` activates the window, do not call it for re-shows after the first: use the Win32 path above.
  All unsafe code goes in `winsys.rs`, with `// SAFETY:` comments.
  **Verify** with a Windows test that creates the overlay with the real backend **only if** that needs no input and no screenshot. Otherwise, verify by reading the window style bits back (`GetWindowLongPtrW`) in a test on our own hidden window, and add an owner row. Write down exactly what was verified and what was not.
- Esc stays the 605 temporary hotkey. A click on the overlay's ✕ icon button calls `cancel()`, which sets the same cancel flag. A click must not activate the window: the style bits above do that.
- Strings (EN | VI, from the prototype):

  | State | Message | Detail |
  |---|---|---|
  | sending | "Sending N characters to Anthropic…" \| "Đang gửi N ký tự tới Anthropic…" | "Esc to cancel" \| "Esc để huỷ" |
  | done | "Translated" \| "Đã dịch" | "Ctrl+Z brings the original back" \| "Ctrl+Z để trả lại câu gốc" |
  | cancelled | "Cancelled, nothing replaced" \| "Đã huỷ, không thay gì" | — |
  | left on clipboard | "You switched windows: the translation is on the clipboard" \| "Bạn đã chuyển cửa sổ: bản dịch nằm trong clipboard" | "Paste it with Ctrl+V" \| "Dán bằng Ctrl+V" |

  The blocked and failed states each get one EN/VI pair per `Block` / `FlowError` / `AiError` group, with prototype wording where it exists. The rest follow the same style:
  - Off, NoKey, Private, Terminal ("Not supported in terminals" / "Không hỗ trợ trong terminal"), UnknownApp, ExcludedApp, KeysHeld;
  - NothingCopied, Sensitive, Empty, TooLong, ClipboardChanged;
  - Auth, RateLimited, Overloaded, Network/Timeout, Refused, other.
  No string contains the app name or any clip text.
  A "done" message names the direction only when it is auto ("Translated").

## 2. Settings → Translate (section 5; About moves to 6)
- `prefs/sections.rs`: add `(5, "translate-enable")`, `(5, "translate-shortcut")`, `(5, "translate-direction")`, `(5, "translate-model")`, `(5, "translate-key")`, `(5, "translate-test")`, `(5, "translate-never-sent")`. Renumber About to 6. Put matching `// section:5 …` markers in `ui/settings.slint`.
- `ui/settings_keys.rs`: Ctrl+1..6. The existing tests are updated only for the new count.
- Rows, as in the prototype:
  - **Enable switch.** Turning it on with `consent_version < 1` opens the consent dialog (the kit `ConfirmDialog`, title and five bullet lines from the prototype). **Turn on** sets `enabled = true` and `consent_version = 1`; **Cancel** leaves it off. Turning it off needs no dialog.
  - Shortcut field (the same control as the popup shortcut). A conflict with the popup shortcut shows a new EN/VI warning, replacing the reuse of `ShortcutRegisterFailed` (605 deviation 7).
  - Direction select (Auto / Always to English / Always to Vietnamese).
  - Model select (Claude Opus 5.5 (quality) / Claude Haiku 5.5 (fast, cheap)).
  - Key row:
    - When no key is stored: a password field plus **Save**.
    - When a key is stored: the status "Stored in Windows Credential Manager (sk-ant-…XXXX)", showing the last 4 characters only, read once at render; plus **Replace key…** and **Remove key**.
    - The key is never shown in full and never put in a Slint property after Save. Clear the field.
    - Length cap: 2560 bytes, the Credential Manager blob maximum. A longer key gets an EN/VI message and is not saved. A store error gets a message with the code only.
    - After a set or a remove, call `translate::host::key_changed()`.
  - **Test** button: off the UI thread, translates the fixed sample "Xin chào" with the current model and shows "OK: \"Xin chào\" → \"…\" in X.X s" or the mapped error. This is the only non-shortcut request, documented in privacy-design §9 (D-093). The answer is shown once and not stored. Show at most 60 chars.
  - **"Never sent" card:** four locked rows with no control (GC-07 style), with the prototype text.
  - When the feature is off, the rows below Enable are disabled (shown as in the prototype, not hidden).
- The settings `ai` section saves through the existing draft and save bar. The key is not part of the draft: it is stored at once on Save, with its own feedback.
- Off Windows, the section shows one line, "Not available on this platform" (EN/VI), with the controls disabled.

## Tests
- Headless (GC-19; the `tests/menu_click.rs` pattern with `MinimalSoftwareWindow` and `dispatch_event`; one `#[test]` per file because of `set_platform`):
  - `tests/translate_overlay.rs`: a pointer click on ✕ runs `cancel`; each state renders the right icon and visibility (spinner only in sending).
  - `tests/translate_settings.rs`:
    - clicking the Enable switch with no consent opens the dialog;
    - Cancel keeps it off;
    - Turn on sets enabled and consent 1;
    - Save with an empty or too-long key shows the error and stores nothing (fake store);
    - Remove clears the stored key;
    - the Ctrl+6 key picks About.
- Pure tests:
  - the `OverlayState` → (state, message, detail) mapping for every variant in EN and VI, with no `{}` left;
  - the key mask shows only the last 4 characters;
  - `settings_sections` drift;
  - the Ctrl+1..6 key map.
- Existing scans updated, not weakened:
  - `a11y_scan` (labels and roles for every new control);
  - `no_font_glyphs`;
  - `layout_scan`;
  - `settings_budget`: EN/VI width budget for the new rows and the overlay at 340 px max width.
- Windows: the overlay window's style bits read back (see §1).

## Files this task may edit
- `apps/desktop/ui/{translate_overlay.slint (new),settings.slint,settings_parts.slint,app.slint (exports)}`
- `apps/desktop/src/{ui/translate_host.rs (new),ui/mod.rs,ui/settings_host.rs,ui/settings_keys.rs,prefs/**,translate/host.rs (swap the sink, key_changed),winsys.rs}`
- `apps/desktop/tests/**`, `apps/desktop/Cargo.toml` (`windows` features only if needed)
- `crates/darkclip-core/src/i18n.rs`
- `docs/security/privacy-design.md` (§9: the Test button sentence, if not already exact)
- `memory-bank/progress.md` (the 606 row)

## Acceptance
- Matches the prototype's layout, states and text.
- Every test above passes, and the existing UI tests are unchanged in meaning.
- `cargo clippy --workspace --all-targets -- -D warnings` is clean.
- `cargo fmt --all` has been run.
- `powershell -File scripts/check.ps1` prints `ALL CHECKS PASSED`.
- The headless smoke run (`--data-dir <temp> --exit-after-ms --no-global-shortcut`) exits 0.
- A list of owner manual rows: overlay focus behaviour in Notepad, Zalo and Slack; consent dialog; key Save, Replace and Remove; Test button; DPI 100 and 150 %; Vietnamese text.

## Known pitfalls (from `memory-bank/systemPatterns.md`)
- **UI interaction changes need a headless test** (`CLAUDE.md`, Task 713): every click path above has a `dispatch_event` test; key-map tests alone missed a dead mouse path before.
- **UI icons are vector `Path`s, never glyphs** (Task 414). The overlay and settings must fit their widths in both languages (budget tests).
- **UI kit:** compose the `ui/kit/` components (Switch, Select, Field, Button, IconButton, ConfirmDialog); do not draw new controls. Key handling is a Rust key map driven from Rust (`settings_keys.rs`).
- **`#[path]` modules in integration tests only at the top level** (F-37).
- **Windows-only code must lint on macOS/Linux** (F-19).
- **GC-06:** no key, translation or clip text in logs, `Debug` or Slint properties beyond what is shown. The Test result is shown, not logged.
- **GC-19:** no screenshots, no real input, no real clipboard, never the default data dir. Visual checks are owner rows.
- **F-42:** known timing flakes. Rerun before blaming the change.
