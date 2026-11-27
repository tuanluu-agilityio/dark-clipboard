# Task 605: Translate flow controller, settings and second shortcut

**Agent:** `specialist` · **Model:** opus · **Branch:** `task/605-translate-flow` (from `main`) · **Review:** `security-reviewer` (opus)
**Global Constraints:** all of the plan's; especially GC-03, GC-06, GC-08 (D-094), GC-14, GC-19, D-093, D-095. Plan: `docs/plans/phase-6-translate-in-place.md` (Review Focus 1–5, Interfaces §605 as changed by 603b). Spec: `docs/specs/translate-in-place/design.md` §5.1, §6. Integration review I1 and the 601–604/603b review notes are folded in below.

## Goal
Wire the merged pieces into one working flow. The pieces are `darkclip-ai`, `ai_net::UreqTransport`, `WinSecretStore`, the capture claim (`ExpectForeignCopy`, `CopyTextIf`, `RestoreIf`, `RestoreCandidate`), `PasteInjector::copy/paste` and `process_app_id`.

The flow is triggered by a second global shortcut. It is pure and tested through a `FlowSys` seam; a thin host does the real calls on a worker thread.

**Not in this task (606):** the overlay window, Settings → Translate, the consent dialog and the key entry UI. Here the overlay is a trait with a content-free logging stub. 606 replaces it.

## Settings (`crates/darkclip-app/src/settings.rs`)
```rust
#[serde(default)] pub ai: AiSettings,
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct AiSettings {
    pub enabled: bool,          // false
    pub shortcut: String,       // "Ctrl+Alt+T"
    pub direction: Direction,   // darkclip_ai::Direction::Auto
    pub model: String,          // darkclip_ai::DEFAULT_MODEL; any value not in darkclip_ai::MODELS falls back to the default at use
    pub consent_version: u32,   // 0; 606's dialog sets 1
}
```
The feature is **active** only when `enabled && consent_version >= 1` **and** a key is stored. An old settings file loads with defaults (`SETTINGS_VERSION` unchanged; add a test like the `updates` one). `darkclip-app` gains a `darkclip-ai` dependency (pure crate).

## App API (`crates/darkclip-app/src/api/translate.rs`, new; UI caller only, as `transform_ui.rs`)
Thin wrappers over the actor commands, with the existing `Caller`/permission pattern:
- `ui_translate_claim(c, app_id: String) -> Result<Receiver<Result<ClaimedText, ClaimError>>, ApiError>`. Arming does not block. The caller waits on the receiver.
- `ui_restore_candidate(c) -> Result<Option<ClipId>, ApiError>`
- `ui_copy_text_if(c, expect_seq, text: Redacted<String>) -> Result<Option<u64>, ApiError>`
- `ui_restore_if(c, expect_seq, id) -> Result<bool, ApiError>`
- `ui_clipboard_seq(c) -> Result<u64, ApiError>`. Add an actor command if needed; the seq is not content.
- `is_excluded_app(app_id: &str) -> bool`: the same matching as the policy's `app_matches` against the current excluded list (built-ins included). Expose a pure `darkclip_core::policy::app_is_excluded(app_id, platform, excluded) -> bool` and reuse it, so the flow and capture can never disagree.

## Flow (`apps/desktop/src/translate/flow.rs`, pure)
Use the `FlowSys` sketch from the plan §605 (603b version), with these additions:
- `Block` adds `Terminal` (D-095), `UnknownApp` (`process_app_id` is `None`, I1 M2) and `KeysHeld`.
- `FlowError` adds `ClipboardChanged` (`write_own_if` returned `None`) and `NotPasted` (paste refused: the translation stays on the clipboard; this maps to `Outcome::LeftOnClipboard`).
- A `Cleared` newtype (private constructor in `flow.rs`) holds the text that passed the empty, cap and sensitive checks. `FlowSys::translate` takes `Cleared`, not a plain `Redacted<String>` (I1 item 1). `Cleared` has a manual `Debug` that prints `<redacted>`.

Constants (`flow.rs`): `KEYS_UP_WAIT_MS = 1000`, `COPY_WAIT_MS = 400`, `CLAIM_WAIT_MS = CLAIM_WINDOW_MS + CLAIM_REPLY_SLACK_MS`, `RESTORE_DELAY_MS = 400`, `POLL_MS = 10`.

`run(sys, cfg)` order:
1. `target()`, the foreground window of another process. `None` gives `Blocked(NotNormalWindow)`.
2. `blocked(t)` checks, in this order, and a hit returns `Blocked(..)`:
   - `Off`: not active, as defined in Settings above;
   - `NoKey`;
   - `Private`;
   - `Terminal`: window class in the console list (below);
   - `UnknownApp`;
   - `ExcludedApp`.
   No key is sent and nothing is armed.
3. `keys_up(KEYS_UP_WAIT_MS)`: all modifiers **and** the shortcut's main key are up (I1 M1, 604 note). Otherwise `Blocked(KeysHeld)`.
4. `candidate = restore_candidate()`, taken **before** the copy (I1 R5).
5. `seq0 = clipboard_seq()`, `rx = claim(app_id)` (one receiver for both attempts; never re-arm: a second arm replies `Busy`), then `send_copy(t, false)`.
   - A refusal ends with `Blocked(NotNormalWindow)`, or `KeysHeld` if that is the reason. Let the slot expire.
   - Poll `clipboard_seq()` every `POLL_MS` for up to `COPY_WAIT_MS`.
   - If it moved, wait on `rx`.
   - If not, re-check `clipboard_seq()` right before `send_copy(t, true)`. If it moved meanwhile, wait on `rx` without select-all (I1 M4).
   - Wait on `rx` for at most `CLAIM_WAIT_MS` from arming.
   - `Esc` (`cancelled()`) at any wait gives `Cancelled` (drop the receiver; the actor wipes the text if it arrives later).
   - A timeout or `Expired` gives `Failed(NothingCopied)`.
   - `Err(e)` gives `Failed(Claim(e))`.
6. Text checks on the claimed text:
   - trimmed empty gives `Failed(Empty)`;
   - more than `darkclip_ai::MAX_TRANSLATE_CHARS` chars gives `Failed(TooLong)`;
   - `is_sensitive` (all categories, regardless of the user's sensitive setting, which is stricter) gives `Failed(Sensitive)`.
   Then build `Cleared`.
7. `overlay(Sending { chars })`, `rx2 = translate(cleared)`. Wait with `cancelled()` polling, the timeout coming from the transport (30 s). Cancel gives `Cancelled`; a late result is dropped and wiped.
8. **Replace:**
   - `own = write_own_if(claimed.seq, translation)`. `None` gives `Failed(ClipboardChanged)`, nothing written or pasted. `Err` gives a failure.
   - If the foreground is no longer the target (same window and pid), `LeftOnClipboard`, no paste.
   - Otherwise `paste(t)`. A refusal gives `LeftOnClipboard`. Success gives `overlay(Done)` immediately, **at paste time** (G1: hide the restore latency).
9. **Restore:**
   - On `Replaced`: `sleep(RESTORE_DELAY_MS)`, then `restore_if(own, id)` if `candidate` is `Some`.
   - On **every failure after the claim succeeded** (steps 6–8 without a write): `restore_if(claimed.seq, id)`. The clipboard then still holds the target's copy of the user's text, so the previous clipboard comes back.
   - On `LeftOnClipboard`: no restore.
   - The flow never calls `restore_candidate` again.
10. Wipe `ClaimedText.text`, the translation and every intermediate `String` you own, on every path (wipe helper as in `capture/wipe.rs`; expose a small `darkclip_core` wipe if needed).
11. `overlay` gets the final state. The host logs one content-free line, for example `translate outcome=replaced src_chars=N out_chars=M api_ms=T total_ms=T`, or `outcome=blocked reason=terminal`. No text, no app id.

## Host (`apps/desktop/src/translate/host.rs`, plus `mod.rs`)
- Real `FlowSys`:
  - target from `winsys::foreground_window_of_other_process`;
  - `app_id` from `darkclip_windows::process_app_id(pid)`;
  - `keys_up` via `GetAsyncKeyState` over Ctrl, Shift, Alt, Win and the shortcut's key (`winsys.rs`);
  - the paste injector from `popup_host::Injector` (the same `Arc<Mutex<..>>`: copy and paste are serialized with the popup's paste);
  - the API calls above;
  - the provider: `AnthropicProvider::new(UreqTransport::new(), key, model)`, with the key from `WinSecretStore.get(ANTHROPIC_KEY_TARGET)` read **per press**, never cached;
  - `is_sensitive` via `darkclip_core::sensitive::scan` with all categories.
- Terminal (D-095): `winsys::is_console_window(hwnd) -> bool`. It compares `GetClassNameW` against a fixed list (`ConsoleWindowClass`, `CASCADIA_HOSTING_WINDOW_CLASS`, `mintty`, `PuTTY`, `VirtualConsoleClass`, `org.wezfurlong.wezterm`, `Alacritty`) and returns only the bool. The class string is never stored or logged. Add a dated "Changed by D-095" note to SR-11 / the `winsys.rs` doc ("no class name" now has this one exception, returning only a bool).
- **Single flight:** an `AtomicBool`. A press while running is ignored (Review Focus 3). The flow runs on a named worker thread `darkclip-translate`.
- **Esc:** while a flow runs, register `Escape` as a temporary global hotkey that sets the cancel flag. Unregister it when the flow ends. Never leave it registered.
- **Overlay sink:** `trait OverlaySink { fn show(&self, s: &OverlayState); }`. In this task the implementation is `LogOverlay` (content-free log line); 606 swaps in the window.
- **Non-Windows:** the shortcut is not registered and the feature reports unavailable. Everything compiles on macOS and Linux (F-19).

## Second shortcut (`apps/desktop/src/ui/shortcut_host.rs`)
- Generalize the registrar to several shortcuts, keyed by an id: `Popup`, `Translate`.
- The single `GlobalHotKeyEvent` handler dispatches on `e.id()` (`HotKey::id()`).
- Each shortcut has its own `Status`, including the "taken by another app" warning.
- `Translate` is registered only while the feature is active, and re-applied on settings changes.
- If `ai.shortcut` equals the popup shortcut, `Translate` is not registered and its status says so.
- The popup shortcut's behaviour and existing tests are unchanged.

## Tests (write first; flow tests with a scripted fake `FlowSys` that records calls in order)
Review Focus:
- `restore_skipped_when_clipboard_changed_after_own_write`: the fake actor's `restore_if` returns `false`, and the flow does nothing more.
- `newer_user_copy_during_api_wait_is_not_overwritten`: `write_own_if` returns `None`; no paste, `Failed(ClipboardChanged)`.
- `second_press_while_running_is_ignored`: a host-level test with a fake flow runner.
- `failure_never_pastes_and_restores_previous_clipboard`: for each `AiError` and for `Sensitive`, `Empty` and `TooLong`, there is no `paste`, and `restore_if(claimed.seq, candidate)` is called.
- `empty_or_over_cap_text_is_not_sent`: no `translate` call. 8000 chars is sent; 8001 is not.

Ordering:
- `keys_up_before_claim`: the call order is `keys_up`, `restore_candidate`, `clipboard_seq`, `claim`, `send_copy`.
- `held_keys_block_before_arming`

Blocks (no `claim` or `send_copy` calls):
- `blocked_checks_in_order_and_send_nothing`: one case per `Block`.

Select-all (I1 M4):
- `select_all_only_when_seq_unchanged`
- `no_select_all_when_seq_moves_just_before_retry`
- `one_claim_for_both_attempts`

Waits and replace:
- `claim_timeout_is_nothing_copied`
- `cancel_at_each_wait_writes_nothing`
- `focus_moved_leaves_on_clipboard_without_paste`
- `paste_refused_leaves_on_clipboard`
- `done_shown_at_paste_time_before_restore`

Settings and safety:
- `sensitive_uses_all_categories`
- `debug_and_log_lines_are_content_free`: format the fakes' log lines and the outcome `Debug`; neither contains the text.
- `settings_old_file_loads_ai_defaults`
- `inactive_without_consent_or_key`

Shortcut and Windows:
- shortcut registrar: `two_shortcuts_dispatch_by_id`, `translate_not_registered_when_inactive_or_same_as_popup`, and the existing popup tests unchanged.
- `is_console_window` (Windows): a unit test on the pure class-list matcher (`class_is_console(&str) -> bool`). No real window reads are needed beyond our own message window, which returns false.

## Files this task may edit
- `crates/darkclip-app/{Cargo.toml,src/settings.rs,src/api.rs,src/api/translate.rs (new),src/api/tests/*}`, and `src/capture/**` only if a small actor command for the clipboard seq is needed
- `crates/darkclip-core/src/policy.rs` (`app_is_excluded`), a small wipe helper in core if needed
- `apps/desktop/src/{main.rs,winsys.rs,translate/** (new),ui/mod.rs,ui/shortcut_host.rs,shortcut.rs,popup_host.rs (injector sharing only)}`, `apps/desktop/Cargo.toml`
- `apps/desktop/src/prefs/model.rs` (load and save `ai` round trip only; no UI)
- `docs/plans/global-constraints.md` (SR-11 / D-095 note, if SR-11 is stated there; otherwise threat-model SR-11)
- `docs/security/privacy-design.md` (proxy sentence for translate: env proxies honoured including NO_PROXY, Windows system proxy not used; 602 review)
- `docs/security/threat-model.md` (T-21: R2, a claim that cannot match stores the source in Normal mode; R6, plaintext copies not wiped in darkclip-ai)
- `memory-bank/progress.md` (the 605 row)

## Acceptance
- Every test above passes; the existing popup shortcut tests are unchanged in meaning.
- `cargo clippy --workspace --all-targets -- -D warnings` is clean.
- `cargo fmt --all` has been run.
- The Linux-target clippy for the crates you touched compiles where it can (F-19).
- `powershell -File scripts/check.ps1` prints `ALL CHECKS PASSED`, including GC-08: no network outside `ai_net.rs` and `update.rs`, and the flow reaches the network only through the `Transport`.
- No test sends real input, writes the real clipboard or makes a network call (GC-19).
- The app starts and exits headless with `--data-dir <temp> --exit-after-ms` (existing smoke path), with the feature off.

## Known pitfalls (from `memory-bank/systemPatterns.md`)
- **GC-03:** the desktop never reads the clipboard. Everything goes through the actor API.
- **GC-06:** no content in logs, `Debug`, errors or overlay states. Lengths, timings and result codes only.
- **Windows-only code must lint on macOS/Linux** (F-19): `cfg_attr(not(windows), allow(dead_code))`.
- **No panics in release paths;** every lock uses `unwrap_or_else(PoisonError::into_inner)`.
- **UI interaction changes need a headless test** (`CLAUDE.md`): the new shortcut dispatch is tested in the registrar's pure tests. Esc is a hotkey, not a window key, so 606 adds the overlay pointer and keyboard tests.
- **GC-19:** no real `SendInput`, clipboard writes or network in tests. Never run the app against the default data dir.
- **F-42:** known timing flakes. Rerun before blaming the change.
