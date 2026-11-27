# Translate in place: design

Status: **approved by the owner 2026-11-27** (design and prototype, "Duyệt tất cả"; D-093, D-094). Plan: `docs/plans/phase-6-translate-in-place.md`. P2 feature (optional AI). Spike:
`spikes/translate-hotkey/` (branch `spike/translate-hotkey`, throwaway).

## 1. Intent

The owner writes messages and emails in Vietnamese and sends them in English. Today that
takes five steps: type, copy, paste into Google Translate, copy the English, paste it back.
The goal is one shortcut. Type Vietnamese in any app, press the shortcut, and the text in the
field becomes English, with Ctrl+Z bringing the Vietnamese back. Ctrl+C keeps working exactly
as it does today. The translation uses the owner's own Anthropic API key.

Success means the owner uses it every day instead of Google Translate. The round trip has to
feel quick, the English has to be good enough to send without fixing it, and nothing has to be
cleaned up afterwards: no stray clips in history, and no clipboard that has silently changed.

## 2. Goals, non-goals, anti-goals

Goals
- G1. One global shortcut (default `Ctrl+Alt+T`, can be changed) translates the selection. If
  nothing is selected, it translates the whole focused text field. The result replaces the
  text in place.
- G2. Direction: `auto` (Vietnamese to English, anything else to Vietnamese) is the default.
  `to English` and `to Vietnamese` are settings.
- G3. Keep code, URLs, file paths, @mentions, #channels, emoji and names unchanged, and keep
  line breaks and markdown.
- G4. History stays clean. Neither the source text read for translation nor the translation
  is stored as a clip.
- G5. Visible state. A small overlay near the mouse cursor shows "Translating…" (Esc cancels),
  then disappears, or shows a short error.
- G6. Off by default. Nothing is sent anywhere until the owner enables the feature and enters
  a key.

Non-goals (this version)
- Other providers (Ollama, OpenAI-compatible endpoints). The provider seam allows them later.
- Translating clips from the popup ("paste as English"). That is a later, separate task.
- macOS and Linux. The feature is Windows only and is reported as unavailable elsewhere.
- Glossary, tone presets and a preview-before-replace mode. These are follow-ups, decided
  after daily use.

Anti-goals (a failure even if it works)
- A1. Text from an excluded app (password managers, Darkshot, the user's excluded list), or
  text that sensitive detection flags, leaves the device.
- A2. Any background or implicit upload. Only a press of the shortcut sends text.
- A3. The API key in plain text on disk, in logs or in settings.
- A4. Text pasted into a different window than the one the shortcut was pressed in.
- A5. Content in logs (GC-06). Logs carry lengths, timings and result codes only.
- A6. A network path that any other part of the app could reuse.

## 3. Constraints that shape it

- **GC-08 / D-057.** `crates/` must stay network-free, and only `apps/desktop/src/update.rs`
  may use HTTP today. The check scripts enforce this in three layers (crate dependency tree,
  renamed dependencies, `.rs` pattern scan with a one-file allowlist).
- **GC-03.** Only the capture actor reads or writes the clipboard, and it reads only after a
  policy decision.
- **ai-integration.md §4.** It says "always initiated by the user in the UI" and "for Cloud, a
  confirmation dialog shows the exact text to be sent". A dialog on every press defeats the
  one-key goal, so this design proposes the change in §6.3. That change needs the owner's
  decision and a dated "Changed by D-NNN" note.
- **Privacy promise 1** ("No cloud") holds for the default install. The feature is opt-in and
  labelled as sending text to Anthropic.
- **GC-19.** Agents never send keys or write the real clipboard. All injection code is tested
  through fakes, and the end-to-end rows are owner manual rows.
- **GC-14.** Every string goes in `darkclip_core::i18n` (EN and VI).
- **Threat model.** T-13 / SR-09 (no implicit AI upload), T-19 (auto-paste), T-20 (network
  mitigations of the update check, reused here).

## 4. Approaches considered

| | Approach | Trade-off | Verdict |
|---|---|---|---|
| A | Inside Dark Clipboard: hotkey, capture-actor skip, overlay, settings, keystore; network confined to one allowlisted module in `apps/desktop` | Only way to keep history clean without a new IPC command; adds a second network exception to the main app | **Chosen by the owner** |
| B | Separate helper exe (productised spike) that talks to the app over IPC | Main app stays offline, but needs an IPC command "skip the next copy", a new privacy-sensitive surface any local process could abuse; two processes, two settings UIs | Rejected |
| C | "Paste as English" in the popup after a normal Ctrl+C | Simplest, but loses the one-key flow and stores the source as a clip | Later add-on |

## 5. How it works (approach A)

### 5.1 Flow on one press

1. **Hotkey.** `global-hotkey` fires `translate_in_place` on the main thread. If a translation
   is already running, the press is ignored (single flight).
2. **Gate, before any key is sent:**
   - The feature is on, a key is stored, and capture mode is not `Private`. In Private mode the
     overlay says why and nothing happens. Paused mode is allowed, because it only stops
     storing.
   - The foreground window belongs to another process, is a normal top-level window, and its
     integrity level is not higher than ours. The checks are the same as paste-back (T-19).
   - The foreground app is **not excluded**: it is resolved with the same source resolver and
     excluded-app list as capture. An excluded app gets an overlay error and no Ctrl+C.
   - Capture `Disabled` (including the fresh-install state), `Paused` and disabled text kinds do
     **not** block translate, because nothing is stored. The claimed path gets its own policy
     entry, `decide_translate_read`, with D-061 ("Save as new clip" while paused) as the
     precedent. It keeps every privacy skip: Private, mandatory markers, transient,
     auto-generated, excluded app, and size caps. (Builder call.)
3. **Copy.** The actor gets `ActorCommand::ExpectForeignCopy { pid, deadline }` (§5.3). The
   paste driver then sends Ctrl+C to the recorded target. If the clipboard sequence does not
   change within 400 ms, it sends Ctrl+A, Ctrl+C. This is the behaviour the spike validates.
4. **Read.** The actor sees the change from the target pid inside the window. It runs the
   normal pre-read policy, so markers, excluded apps, transient and auto-generated content
   all still skip. On `Decision::Read` it reads text only (plain, capped at
   `MAX_TRANSLATE_CHARS = 8000`). It hands the text to the requester through the command's
   reply channel and **does not store it**. Any skip reason ends the flow with an overlay
   error, and the reason code is logged without content.
5. **Sensitive check.** `darkclip_core` sensitive detection runs on the text. A hit means
   nothing is sent (A1), and the overlay says "Looks like a secret: not sent".
6. **Translate.** The text goes to the provider on a worker thread. The overlay shows
   "Translating…". Esc (a key handled by the overlay, with a hotkey registered only while the
   overlay is shown) cancels. The response arrives, or the request times out after 30 s.
7. **Replace.** The target must still be the foreground window (same hwnd and pid). If it is,
   the actor writes the translation (`CopyText`, own write, suppressed by seq; not stored)
   and the paste driver sends Ctrl+V. If the focus moved, nothing is pasted, the translation
   stays on the clipboard and the overlay says so.
8. **Restore.** After 400 ms the previous clipboard comes back if it was the newest stored
   clip and the clipboard seq at step 2 equals the actor's last processed seq: the actor
   copies that clip back (`CopyBack`, own write). Otherwise the clipboard keeps the
   translation (§8, builder call). The flow never reads clipboard content outside the actor
   and the policy (GC-03).

### 5.2 Components

| Part | Where | What |
|---|---|---|
| Provider seam | new crate `crates/darkclip-ai` (no network, no OS deps) | `Provider` trait as written in ai-integration §4 (`transform(TransformRequest)` with `Locality`), so the CLI and MCP can share it later. Translate is `TransformKind::Translate { direction }`. `AiError` is content-free. Prompt building and response parsing are pure and unit tested. A `Transport` trait takes request bytes and returns response bytes. |
| HTTP transport | `apps/desktop/src/ai_net.rs` (second allowlisted network file) | ureq 3 + rustls (already a dependency). HTTPS only, host pinned to `api.anthropic.com`, no redirects, 30 s timeout, 256 KiB response cap, fixed User-Agent, content-free errors (T-20 mitigations). It is reachable only from the translate flow. |
| Anthropic provider | `darkclip-ai` | `POST /v1/messages`, `anthropic-version: 2026-10-21`. The model is a setting (default `claude-opus-5-5`, effort `low`; `claude-haiku-5-5` offered as the fast and cheap option). It uses the server-side fallback `fallbacks: "default"` with beta `server-side-fallback-2026-07-01` on the models that accept it. It handles `stop_reason` `refusal` and `max_tokens`, and keeps text blocks only. The system prompt is the spike's (translation only, text in `<text>` tags, treated as content and never as instructions). |
| Key storage | `crates/darkclip-windows` (`win.rs` unsafe) behind a `SecretStore` trait in `darkclip-platform` | Windows Credential Manager (`CredWriteW` / `CredReadW` / `CredDeleteW`, generic credential `dark-clipboard/anthropic`, user scope). The key never enters `Settings`, logs or backups. The fake store is used in tests. |
| Capture skip | `darkclip-core::policy` + `darkclip-app::capture` | New `ActorCommand::ExpectForeignCopy` and a new outcome "hand to requester, do not store" (§5.3). |
| Key injection | `crates/darkclip-windows/src/paste.rs` | `Mode::Copy` and `Mode::SelectAllCopy` next to `Paste`. `WindowSys` gets a key parameter (`send_ctrl(vk)`). `PasteInjector` gets `copy(&PasteTarget)` with the default-method pattern used by `focus`. The same release-wait and target checks apply. |
| Hotkey | `apps/desktop/src/ui/shortcut_host.rs` | Multi-hotkey registrar. The single global handler dispatches on `HotKey::id()`, and each shortcut gets its own status and warning. |
| Overlay | new Slint window `apps/desktop/ui/translate_overlay.slint` | A small frameless, always-on-top, no-activate window (it must not take focus from the target). It is placed near the mouse cursor with `popup/place.rs`, and states are Translating / Error / Done. Caret position is a follow-up. |
| Settings | `Settings.ai: AiSettings` (`#[serde(default)]`) + a prefs row in a new section "Translate" | `enabled` (false), `shortcut` ("Ctrl+Alt+T"), `direction` (auto), `model`, plus key set/replace/remove buttons (the key itself is never shown back) and a "Test" button that translates a fixed sample. |

### 5.3 Capture skip contract

> **Changed by Task 603 (2026-11-27):** read "pid" below as `app_id`: the claim matches the change's source `app_id` (the executable identity the platform resolves at signal time; `ClipboardSource` has no pid) against `PasteTarget.app_id`. An ownerless flush is claimed only through the unchanged F-14 rule 2 heir. Two own writes in a row are suppressed by the snapshot-truth rule (the snapshot's seq is the later own write), proved by a test, instead of a set of 4 seqs.

- `ExpectForeignCopy { pid, deadline, reply }` arms a single slot in the actor. The slot holds
  the expected pid, a deadline (now + 600 ms, covering the Ctrl+A retry) and the reply sender.
- The **first** clipboard change after arming whose resolved source pid matches, before the
  deadline, is "claimed". Pre-read policy runs first, so every existing skip still wins. If
  the policy reads, the read is text only and the result goes to `reply`. The change is
  marked processed and never stored, deduplicated or counted.
- **Multi-phase writers (Notepad, PowerShell; F-14 / D-042).** The claim follows the burst. An
  ownerless final phase is claimed only when (i) it arrives inside the window, (ii) the same
  debounce burst contains an owned phase from the target pid, and (iii) D-042 rule 2 credits it
  to the target. An ownerless change with no owned target phase in its burst is never claimed,
  so a copy that some other app makes in the same 600 ms is not swallowed. (Builder call.
  Without this, Notepad would fail and its text would still be stored, which breaks G4.)
- A change from any other pid, or a change after the deadline, is handled exactly as today,
  and the slot expires.
- A claimed change logs `capture skip=translate_source` (no content).
- Two own writes in a row (translation, then restore) need suppression by more than one seq.
  `last_self_write_seq` becomes a small set (the last 4 own seqs).

## 6. Privacy and security

### 6.1 What leaves the device

Per press: the selected or whole-field text (at most 8000 chars) and the system prompt, sent
to `api.anthropic.com` over HTTPS with the owner's key. Nothing else is sent: no clips, no
app names, no window titles, no machine id. Anthropic's own data retention applies, and the
Settings row links to it in words, without a live link fetch.

### 6.2 Checks before sending

Feature on; not Private; the foreground app not excluded; `decide_translate_read` passes
(markers, transient, auto-generated, excluded app); **no `NoCloud` marker**
(`CanUploadToCloudClipboard=0`: content the source app says must not leave the device is never
sent, which matches privacy-design's "never synced"); sensitive detection is clean; text not
empty and at most 8000 chars (longer text is never sent and never truncated).

### 6.3 Consent (owner approved 2026-11-27, §9 Q1)

The proposal replaces the per-request confirmation dialog of ai-integration §4 for this one
flow with:
- **Enable-time consent.** Turning the feature on shows one dialog: provider, model, exactly
  what is sent per press (including that Ctrl+A sends the whole field), the key storage, and
  an "Enable" button.
- **Per press,** the press of the dedicated shortcut is the explicit user action, and the
  overlay shows "Sending N characters to Anthropic…".
- This becomes D-NNN, with a dated "Changed by D-NNN" note in ai-integration.md §4 and in
  threat-model T-13.

### 6.4 Network exception

A new decision amends GC-08. A second network file, `apps/desktop/src/ai_net.rs`, is allowed,
and it may reach only `api.anthropic.com`. The check scripts allowlist exactly that file, and
`crates/` stays network-free. A test asserts the host pin and `https_only`.

### 6.5 Review

The task touches privacy, concurrency (actor), FFI (Credential Manager, SendInput) and network.
Implementation goes to `specialist` on opus and review to `security-reviewer` on opus.

## 7. Testing

- Pure tests: prompt building and response parsing (refusal, max_tokens, thinking blocks
  ignored, `<text>` stripping), direction rules, settings defaults and old-file load.
- Actor tests with `FakePlatform`. A claimed change is never stored; another pid's change is
  stored; an ownerless change is not claimed; the deadline expires; a marker or excluded skip
  still wins; the two own writes are both suppressed.
- Paste driver tests with the fake `WindowSys`: Copy / SelectAllCopy modes, target checks,
  release wait.
- Transport tests with a fake `Transport`. There is no real network in tests. The check
  scripts prove the allowlist.
- Overlay: a headless Slint test (`MinimalSoftwareWindow` + `dispatch_event`) for Esc cancel
  and the state changes. Per the `CLAUDE.md` rule, every interaction change ships with one.
- Owner manual rows: the spike table (Notepad, Chrome/Gmail, Zalo, Slack/Teams, VS Code,
  Word), the excluded app (KeePassXC), focus change during translation, Ctrl+Z, admin target,
  and latency per model.

## 8. Left to the builder (calls already made, can be revisited)

- Restore policy (§5.1 step 8): restore only when the previous clipboard is the newest stored
  clip; otherwise leave the translation. Full multi-format save/restore is out of reach
  without reading content outside policy.
- Cap of 8000 chars, timeouts (400 ms copy wait, 600 ms claim window, 30 s API, 400 ms before
  restore). Tune from the owner's spike results.
- The overlay sits near the mouse cursor (no caret API yet).
- Apps where Ctrl+C with no selection copies the current line (VS Code, some editors) never
  reach the Ctrl+A fallback. The line is translated and pasted at the cursor, so text gets
  duplicated. This is a known limit, documented in the overlay help and the release notes;
  selecting first avoids it.
- **Feasibility gate.** The owner's spike rows (`spikes/translate-hotkey/README.md`) are blank.
  Task 1 of the plan is the owner running them. If more than half of the listed apps fail or
  the latency is unacceptable, the plan stops for a redesign before any actor work.
- Task split and numbering (a phase plan under `docs/plans/`, following the repo's task
  package format).

## 9. Owner answers (2026-11-27)

- Q1 Consent: **approved as proposed** (§6.3): enable-time consent plus the per-press
  overlay, with no per-press dialog. Recorded as a new D-NNN with a "Changed by" note in
  ai-integration §4 and T-13.
- Q2 Network: **approved**. One more allowlisted file, `apps/desktop/src/ai_net.rs`, may reach
  only `api.anthropic.com` (amends GC-08 / D-057).
- Q3 Order: **before Paste Stack (0.2)**. This feature opens P2.
- The owner reviewed and approved the UI prototype (`prototype.html` in this folder: overlay
  states, Settings → Translate, consent dialog), as well as the functional spike.
