# Phase 6: Translate in place (opens P2)

> **Status: APPROVED by the owner 2026-11-27** ("Duyệt tất cả", design + prototype). Runs
> **before** 0.2 Paste Stack. Design: `docs/specs/translate-in-place/design.md` (the spec; read it
> with this plan). Prototype: `docs/specs/translate-in-place/prototype.html`. Spike:
> `spikes/translate-hotkey/` (throwaway). Phase 6 was reserved for CLI/MCP/AI (D-036). This is
> its first feature, and the provider seam is shared with the later CLI and MCP work.
> Task packages `docs/tasks/6NN-*.md` are written just before each task starts (the Phase 7
> precedent), from the Interfaces below. Execution follows `CLAUDE.md`: one task, one branch,
> one agent, independent review, `/next-task`.

**Goal:** One global shortcut (default `Ctrl+Alt+T`) replaces the selected text, or the whole
focused field, with its translation from the owner's Anthropic key. History stays clean, and
nothing is sent unless the owner enabled the feature and pressed the shortcut.

**Architecture:** A pure `crates/darkclip-ai` crate holds the provider seam, the prompt and the
response parsing. HTTP lives only in `apps/desktop/src/ai_net.rs`, which is pinned to
`api.anthropic.com`. The key lives in Windows Credential Manager behind a `SecretStore`
platform trait. The capture actor gains a one-shot "claim" that hands the target app's copy
to the translate flow instead of storing it. A pure flow controller in
`apps/desktop/src/translate/` drives copy → claim → translate → paste → restore through seams.
A no-activate Slint overlay shows the state.

**Tech stack:** Rust; `ureq` 3 + rustls (already in `apps/desktop`); `windows` 0.62 (new
features `Win32_Security_Credentials`); Slint; Claude Messages API (raw HTTP, `anthropic-version:
2026-10-21`).

## Global Constraints

All of `docs/plans/global-constraints.md`, plus these from the spec:
- GC-08 amended by D-094 (Task 600): network only in `apps/desktop/src/update.rs` and
  `apps/desktop/src/ai_net.rs`; `ai_net.rs` reaches only `https://api.anthropic.com/v1/messages`,
  with no redirects. `crates/` stay network-free, including `darkclip-ai`.
- Consent per D-093 (Task 600): enable-time dialog plus the per-press overlay "Sending N
  characters to Anthropic…". There is no per-press dialog.
- Off by default: `AiSettings.enabled = false`. With no key stored, nothing is sent.
- Never sent: text from an excluded app, mandatory markers, `NoCloud`
  (`CanUploadToCloudClipboard=0`), transient or auto-generated content, a sensitive-detection
  hit, Private mode, text over `MAX_TRANSLATE_CHARS = 8000`.
- Never stored as a clip: the claimed source change and the translation write.
- The key is never in `Settings`, logs, backups, error messages or `Debug` output (wrap it in
  `Redacted`).
- Values: `COPY_WAIT_MS = 400`, `CLAIM_WINDOW_MS = 600`, `API_TIMEOUT = 30 s`,
  `RESTORE_DELAY_MS = 400`, response cap 256 KiB. Default model `claude-opus-5-5` (effort
  `low`). Offered model `claude-haiku-5-5`. `fallbacks: "default"` with beta
  `server-side-fallback-2026-07-01` only on `claude-opus-5-5`.
- GC-06: logs carry lengths, timings and result codes, never text. GC-14: every string in
  `darkclip_core::i18n` (EN + VI). GC-19: agents never send keys or write the real clipboard,
  and the end-to-end rows are owner rows.
- Windows only. Other platforms report the feature unavailable, and nothing is claimed in
  `platform-compatibility.md` until it is tested.

## Review Focus

Input classes the spec implies but does not spell out. Each one has a test in the task named.

1. **The user copies something else while the translation is pending** (another app writes the
   clipboard between the claim and the restore). The restore must not overwrite the user's new
   copy, and the paste must not paste it. The restore runs only if the clipboard seq is still
   the translate flow's own write. Test in 605:
   `restore_skipped_when_clipboard_changed_after_own_write`.
2. **The source contains framing text** (`</text>`, "ignore previous instructions", markdown
   fences). The request carries it verbatim inside the JSON string. Output parsing strips only
   one outer `<text>…</text>` pair, and the result is only ever inserted as text. Test in 601:
   `source_with_closing_tag_is_sent_verbatim`, `parse_strips_only_outer_text_tags`.
3. **The shortcut is pressed again while a translation is running, or held down.** Single
   flight: the second press is ignored and no second Ctrl+C is sent. Test in 605:
   `second_press_while_running_is_ignored`.
4. **A failed request** (offline, 401, 429, 529, timeout, refusal, `max_tokens`, empty text).
   The field is untouched (no Ctrl+V), the clipboard is restored, and the overlay shows the
   mapped error. Tests in 601 (`maps_status_codes_to_ai_errors`) and 605
   (`failure_never_pastes_and_restores`).
5. **Empty, whitespace-only or over-cap text.** Nothing is sent, and the overlay says why. Text
   over the cap is never truncated. Test in 605: `empty_or_over_cap_text_is_not_sent`.

## Tasks

| Task | Title | Depends | Agent / model | Reviewer | Risk | Acceptance |
|---|---|---|---|---|---|---|
| 600 | Decisions and gates: D-093 (consent), D-094 (GC-08 amendment), dated notes in ai-integration §4, threat-model T-13/T-20 (new T-21 "AI request"), privacy-design §9, roadmap; `check.ps1`/`check.sh` allowlist `apps/desktop/src/ai_net.rs` next to `update.rs` (and self-test that a third file still fails); F-41 regex fix in `check.ps1` (the GC-08 check is silently empty on Windows) | — | scaffolder / sonnet | security-reviewer / opus | Medium (loosens a privacy gate) | Both scripts print `ALL CHECKS PASSED`; a scratch `.rs` with `ureq::` outside the two files fails both scripts (command and output recorded) |
| G1 | **Owner gate:** spike rows in `spikes/translate-hotkey/README.md` filled (Notepad, Chrome/Gmail, Zalo, Slack/Teams, VS Code, Word; latency per model) | — | owner | — | — | If more than half the apps fail or the latency is unacceptable, stop and redesign before 603 |
| 601 | `crates/darkclip-ai`: types, `Provider`, `Transport`, `AnthropicProvider`, prompt, request build, response parse, error mapping | 600 | builder / sonnet | reviewer / opus | Medium | Review Focus 2, 4 (provider side); crate has no network dependencies (gate passes) |
| 602 | Network and secrets: `ai_net.rs` (`UreqTransport`), `SecretStore` trait + `FakeSecretStore` (platform), `WinSecretStore` (Credential Manager, unsafe in `win.rs`) | 600, 601 | specialist / opus | security-reviewer / opus | High (FFI, network, secret) | Host pin and `https_only` tests; key never in `Debug` or errors; Credential Manager round trip runs only in an `#[ignore]` test with a test-only target name |
| 603 | Capture claim: `decide_translate_read`, `ActorCommand::ExpectForeignCopy`, burst-follow claim for ownerless flushes, `NoCloud` block, own-write seq set (4), `ActorCommand::RestoreCandidate` | 601, **G1** | specialist / opus | security-reviewer / opus | High (privacy, concurrency) | Claimed change never stored, counted or deduplicated; other-pid and unrelated ownerless changes stored as today; every privacy skip still wins (tests named in the 603 package) |
| 604 | Key injection: `WindowSys::send_ctrl(vk)`, `Mode::Copy` / `Mode::SelectAllCopy`, `PasteInjector::copy` | 600 | specialist / opus | security-reviewer / opus | High (SendInput, T-19) | Same target checks and release waits as paste; fake-driver tests for both modes; partial `SendInput` releases keys |
| I1 | Integration review of 602–604 contracts: what leaves the device, what is never stored, what is restored | 602–604 | reviewer / sonnet | — | — | Short report; findings become fixes in 605 or a follow-up task |
| 605 | Flow controller and settings: `AiSettings`, `translate/flow.rs` (pure, seams), `translate/host.rs` (wiring, worker thread, cancel), multi-hotkey registrar with id dispatch | 601–604 | specialist / opus | security-reviewer / opus | High (privacy, concurrency) | Review Focus 1, 3, 4, 5; settings old-file load; second shortcut has its own status |
| 606 | UI: overlay window (no-activate, near cursor), Settings → Translate section, consent dialog, key set/replace/remove, Test button, i18n EN/VI, headless pointer and keyboard tests | 605 | builder / sonnet | reviewer / opus | Medium (consent presentation) | Matches the prototype; `tests/settings_sections.rs` updated; headless tests for Esc cancel, consent Cancel/Turn on, key buttons; width budget EN/VI |
| 607 | Verification and docs: owner manual rows in `docs/testing/platform-checklist.md`, platform matrix row (Windows `untested` until the owner rows pass), README, CHANGELOG `[Unreleased]`, privacy notice text, release-notes known limits (VS Code line copy, admin targets, non-text clipboard) | 606 | platform-verifier / sonnet | reviewer / sonnet | Low | No untested platform claim (`platform-capability-check` skill); every known limit listed |
| 608 | Phase review | all | specialist / opus | — | — | Review Focus 1–5 proven, anti-goals A1–A6 of the spec checked one by one |

Order: 600 → 601 → (G1 in parallel) → 602 → 604 → 603 → I1 → 605 → 606 → 607 → 608.
604 comes before 603 because it does not depend on G1.

## Interfaces

Exact names and types that later tasks rely on. A task package may add private items but must
not rename these.

### 601: `crates/darkclip-ai` (depends on `darkclip-core`, `serde`, `serde_json` only)

```rust
pub const MAX_TRANSLATE_CHARS: usize = 8000;
pub const DEFAULT_MODEL: &str = "claude-opus-5-5";
pub const MODELS: &[&str] = &["claude-opus-5-5", "claude-haiku-5-5"];
pub const API_VERSION: &str = "2026-10-21";
pub const FALLBACK_BETA: &str = "server-side-fallback-2026-07-01";

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum Direction { #[default] Auto, ToEnglish, ToVietnamese }

pub enum Locality { Local, Cloud }
pub enum TransformKind { Translate { direction: Direction } }
pub struct TransformRequest { pub kind: TransformKind, pub text: Redacted<String> }
pub struct TransformResult { pub text: Redacted<String> }

/// Content-free: no variant carries request or response text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AiError { NoKey, TooLong, Empty, Network, Timeout, Auth, RateLimited, Overloaded,
                   Http(u16), Refused, Truncated, BadResponse }

pub enum TransportError { Network, Timeout, TooLarge, Status { code: u16, body: Vec<u8> } }
pub trait Transport: Send + Sync {
    /// POSTs to the one fixed Messages endpoint; the URL is not a parameter (A6).
    fn post_messages(&self, body: &[u8], headers: &[(&'static str, String)])
        -> Result<Vec<u8>, TransportError>;
}

pub trait Provider: Send + Sync {
    fn id(&self) -> &str;
    fn locality(&self) -> Locality;
    fn transform(&self, req: &TransformRequest) -> Result<TransformResult, AiError>;
}

pub struct AnthropicProvider<T: Transport> { /* transport, key: Redacted<String>, model, effort */ }
impl<T: Transport> AnthropicProvider<T> {
    pub fn new(transport: T, key: Redacted<String>, model: &str) -> Self;
}

pub fn system_prompt(direction: Direction) -> String;          // spike prompt, per direction
pub fn build_request(model: &str, direction: Direction, text: &str) -> serde_json::Value;
pub fn parse_response(body: &[u8]) -> Result<String, AiError>;
pub fn map_status(code: u16) -> AiError;  // 401/403 Auth, 429 RateLimited, 529 Overloaded, else Http
```

`build_request` emits `model`, `max_tokens: 16000`, `output_config: {effort: "low"}`, `system`,
and one user message `"<text>\n{text}\n</text>"`, plus `fallbacks: "default"` when
`model == "claude-opus-5-5"`. Headers: `x-api-key`, `anthropic-version`, `content-type`, and
`anthropic-beta: FALLBACK_BETA` with the fallback. `parse_response` returns `Refused` on
`stop_reason == "refusal"` and `Truncated` on `"max_tokens"`, joins the `type == "text"` blocks,
trims, strips one outer `<text>` pair, and returns `Empty` if nothing is left.

### 602: transport and secrets

```rust
// apps/desktop/src/ai_net.rs
pub(crate) const MESSAGES_URL: &str = "https://api.anthropic.com/v1/messages";
pub(crate) struct UreqTransport { /* agent: https_only, max_redirects 0, timeout 30 s,
                                     body cap 256 KiB, User-Agent "DarkClipboard/<ver>" */ }
impl darkclip_ai::Transport for UreqTransport { /* … */ }

// crates/darkclip-platform/src/traits.rs
pub const ANTHROPIC_KEY_TARGET: &str = "dark-clipboard/anthropic";
pub trait SecretStore: Send + Sync {
    fn get(&self, target: &str) -> Result<Option<Redacted<String>>, PlatformError>;
    fn set(&self, target: &str, secret: &Redacted<String>) -> Result<(), PlatformError>;
    fn delete(&self, target: &str) -> Result<(), PlatformError>;   // Ok when absent
}
pub struct FakeSecretStore;  // in-memory, in the fake platform
// crates/darkclip-windows: pub struct WinSecretStore;  CRED_TYPE_GENERIC, CRED_PERSIST_LOCAL_MACHINE
```

### 603: capture claim

```rust
// darkclip-core::policy
pub fn decide_translate_read(input: &PreReadInput) -> Decision;
// Same as decide_pre_read minus Disabled / Paused / KindDisabled; Private still skips.

// darkclip-app::capture
pub const CLAIM_WINDOW_MS: u64 = 600;
pub enum ClaimError { Skipped(SkipReason), NoCloud, NotText, TooLong, Expired, Busy }
ActorCommand::ExpectForeignCopy {
    pid: u32,
    reply: Sender<Result<Redacted<String>, ClaimError>>,
}
ActorCommand::RestoreCandidate { reply: Sender<Option<ClipId>> }
// Some(id) only if the OS seq now equals last_processed_seq and that change was stored as `id`.
// ActorCommand::CopyText: its reply gains the own write seq (Ok(u64)), so the flow can tell
// whether the clipboard is still its own write before restoring.
```

Claim rules are as in the spec §5.3. One slot; a second `ExpectForeignCopy` while one is armed
replies `Busy`. An expired slot replies `Expired`. The own-write suppression set holds the last
4 seqs.

### 604: key injection

```rust
// crates/darkclip-platform/src/traits.rs, PasteInjector (default: Err(Unsupported))
fn copy(&mut self, target: &PasteTarget, select_all: bool) -> Result<(), PlatformError>;
// crates/darkclip-windows/src/paste.rs
pub(crate) enum Mode { Focus, Paste, Copy, SelectAllCopy }
// WindowSys: fn send_ctrl(&mut self, vk: u16) -> bool   (replaces send_ctrl_v)
```

### 605: flow and settings

```rust
// crates/darkclip-app/src/settings.rs
#[serde(default)] pub ai: AiSettings,
pub struct AiSettings { pub enabled: bool /*false*/, pub shortcut: String /*"Ctrl+Alt+T"*/,
                        pub direction: Direction /*Auto*/, pub model: String /*DEFAULT_MODEL*/,
                        pub consent_version: u32 /*0; 1 after the dialog*/ }

// apps/desktop/src/translate/flow.rs (pure)
pub(crate) trait FlowSys {
    fn target(&mut self) -> Option<PasteTarget>;          // foreground, checked like paste
    fn blocked(&mut self, t: &PasteTarget) -> Option<Block>; // excluded app, Private, no key, off
    fn clipboard_seq(&mut self) -> u64;
    fn claim(&mut self, pid: u32) -> Receiver<Result<Redacted<String>, ClaimError>>;
    fn send_copy(&mut self, t: &PasteTarget, select_all: bool) -> Result<(), PlatformError>;
    fn is_sensitive(&mut self, text: &str) -> bool;
    fn translate(&mut self, text: Redacted<String>) -> Receiver<Result<TransformResult, AiError>>;
    fn write_own(&mut self, text: Redacted<String>) -> Result<u64, ()>; // returns own seq
    fn paste(&mut self, t: &PasteTarget) -> Result<(), PlatformError>;
    fn restore_candidate(&mut self) -> Option<ClipId>;
    fn copy_back(&mut self, id: ClipId);
    fn overlay(&mut self, s: OverlayState);
    fn sleep(&mut self, ms: u64);
    fn cancelled(&mut self) -> bool;                        // Esc
}
pub(crate) enum Block { Off, NoKey, Private, ExcludedApp, NotNormalWindow }
pub(crate) enum OverlayState { Sending { chars: usize }, Done, Cancelled,
                               LeftOnClipboard, Blocked(Block), Failed(FlowError) }
pub(crate) enum FlowError { NothingCopied, Claim(ClaimError), Sensitive, Empty, TooLong, Ai(AiError) }
pub(crate) enum Outcome { Replaced, LeftOnClipboard, Cancelled, Blocked(Block), Failed(FlowError) }
pub(crate) fn run(sys: &mut impl FlowSys, cfg: &FlowConfig) -> Outcome;
```

The order of `run` is the spec §5.1. The restore candidate is taken **before** the copy and
used only if `clipboard_seq()` still equals the seq `write_own` returned (Review Focus 1).

### 606: UI

`apps/desktop/ui/translate_overlay.slint` exports `TranslateOverlay` with properties
`state: int`, `message: string`, `detail: string` and callback `cancel()`. The window is
created with no-activate and topmost; the host `ui/translate_host.rs` maps `OverlayState` to
the i18n strings from the prototype (EN/VI text is fixed by `prototype.html`). The settings
section is `5 Translate` (About moves to 6), with the consent dialog through the existing
`ConfirmDialog`.

## Why these models

- **specialist / opus** builds 602–605: FFI (Credential Manager, SendInput), the capture actor
  and a concurrent flow, all privacy-critical (CLAUDE.md: never haiku there).
- **builder / sonnet** for 601 (pure, well specified) and 606 (Slint layout following the
  prototype).
- **security-reviewer / opus** reviews every task that touches what leaves the device or what
  is stored (600, 602–605). **reviewer / opus** for 601 (prompt and error content) and 606
  (consent presentation). **reviewer / sonnet** for 607 docs and the I1 integration review.
- **specialist / opus** for the phase review. Never `fable`.

## Stopping points

- After 600: decisions recorded, gates ready, nothing user-visible.
- After G1: go / redesign.
- After 605: the feature works headless with a settings file and a key (no UI). The owner can
  try it with a hand-edited `settings.json`.
- After 606: complete. A release (0.3.0 or 0.2.x) only when the owner asks, after
  `/phase-review` APPROVED (D-077).

## Estimate

Comparable high-risk tasks cost 0.4–0.8 M tokens each (Phase 2/5 ledger in
`memory-bank/progress.md`), and UI tasks 0.2–0.5 M. The total is roughly 4–5.5 M tokens.
