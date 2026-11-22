# Threat Model

Status: Phase 0, 2026-11-22. Method: assets → trust boundaries → STRIDE per boundary → mitigations → residual risk. Re-review at the end of Phase 3 (P0 complete) and before P2 ships MCP.

## 1. Assets

| ID | Asset | Sensitivity |
|---|---|---|
| A1 | Clipboard content in transit (OS clipboard) | May contain passwords, tokens, private keys, personal data |
| A2 | Stored history (`history.db`, blobs) | Same as A1, plus persistence over time |
| A3 | Settings (excluded apps, policies) | Low, but tampering disables protections |
| A4 | AI grants and audit (P2) | Integrity-critical |
| A5 | Logs and crash reports | Must be content-free |
| A6 | Darkshot outputs marked excluded | Owner's explicit intent not to be captured |

## 2. Trust boundaries

```
 [Other apps] --(OS clipboard)--> [B1 platform adapter] --> [B2 capture actor + policy] --> [B3 storage on disk]
                                                                                              ^      ^
 [User] --(UI)--> [desktop]                                                  read-only open  |      | IPC (mutations)
 [MCP client / agent] --(stdio, B4)--> [darkclip mcp] ------------------------------------------+------+
 [CLI user/scripts] --(argv/stdout, B5)--> [darkclip]
 [Optional AI provider] <--(B6, P2, explicit per request)-- [darkclip-ai]
```

Assumption: the attacker does **not** run code as the same OS user. A same-user process can read the clipboard and the user's files directly. We do not claim protection against it (see T-08). We do aim to avoid making things worse than the OS baseline.

## 3. Threats and mitigations

| ID | Boundary | STRIDE | Threat | Mitigation | Residual |
|---|---|---|---|---|---|
| T-01 | B1→B2 | I | Password-manager copy stored in history | Mandatory OS markers checked before read (Win `ExcludeClipboardContentFromMonitorProcessing` / `CanIncludeInClipboardHistory=0`; macOS `org.nspasteboard.ConcealedType`/`TransientType`; Linux `x-kde-passwordManagerHint=secret`); excluded-apps defaults; sensitive detection as a second net | Apps that set no marker and are not excluded; detection misses. Tested per password manager (platform matrix) |
| T-02 | B1→B2 | I | Darkshot-excluded content captured | Darkshot's three Windows formats are mandatory markers (tested in spike S2); no setting disables them; `check.ps1` asserts the marker list contains them | macOS/Linux Darkshot builds do not exist yet; when they do, they must use nspasteboard / KDE hint conventions (coordinate in Darkshot repo, not here) |
| T-03 | B2 | I | Clip content leaks into logs, panics, crash dumps | No `Debug` with content (manual impls); `Redacted<T>`; no logging macros in `capture.rs`, `sensitive/`; `panic = "abort"` and no unwrap in core paths; Windows WER local dumps disabled for our process (Darkshot `wer.rs` pattern) | A third-party crate logging internally (checked when adding deps) |
| T-04 | B3 | I | Disk access by another OS user or offline attacker | Data in per-user local dir with default ACLs (Windows `%LOCALAPPDATA%`, Unix 0700 data dir, 0600 files); documentation recommends full-disk encryption; P2 encryption at rest | Offline attacker on an unencrypted disk can read history |
| T-05 | B3 | I | Deleted clips recoverable | `secure_delete=ON`, blob unlink, WAL checkpoint after bulk delete | SSD remapping; we never claim forensic erasure |
| T-06 | B2 | D | Huge clipboard payload (e.g. 2 GB image) exhausts memory/disk | Per-format byte caps checked before allocation where the API allows (Windows `GlobalSize`, macOS data length, Wayland pipe read with cap); storage cap with retention | A platform API that forces a full read before size is known (Wayland pipes can be read incrementally; fine) |
| T-07 | B2 | T | Malicious content (crafted image, HTML) exploits a decoder | Decode only PNG/DIB/TIFF with memory-safe Rust decoders; never render HTML (Slint has no HTML engine); never execute file paths | Bugs in decoders; fuzz in Phase 5 |
| T-08 | B3/B4/B5 | S, I | Same-user malware reads `history.db` or spawns `darkclip mcp` | Out of scope by assumption; minimize what is stored (no sensitive, retention defaults); P2 encryption at rest with OS keystore narrows the window | Accepted and stated in README privacy section |
| T-09 | B4 | E | Agent/MCP client escalates: copies to clipboard, exports, or reads sensitive data | Default no grant; scopes; sensitive never grantable; privileged tools need explicit scope; desktop toast for clipboard writes; audit | A user over-granting; UI copy explains each scope |
| T-10 | B4 | I | Prompt injection through clip content makes an agent exfiltrate other clips | Scopes limit what can be read; collection filter and max age; results paginated (max 50); content returned only with `clips:read-content`; tool descriptions warn that clip text is untrusted data | Inherent to giving an agent data; documented |
| T-11 | B4 | T | MCP used to read arbitrary files (path traversal via export or "files" clips) | No tool takes a filesystem path as input; export target fixed to `<data>/exports/<uuid>.dcexport`; `files` clips return paths as strings, never contents | — |
| T-12 | IPC | S, E | Another local user connects to the named pipe/socket | Pipe DACL current-user only; socket in user runtime dir 0600; peer credential check (`SO_PEERCRED`/`getpeereid`/`GetNamedPipeClientProcessId` + token user) | Same-user processes (T-08) |
| T-13 | B6 | I | Implicit upload of clips to a cloud AI | No provider by default; cloud providers require per-request confirmation listing the data; no background AI jobs | — |
| T-14 | Update/supply chain | T | Malicious dependency or tampered release | `cargo deny` (advisories, licences, sources) in CI; minimal deps; release artifacts with SHA-256 sums; signing plan per OS (Phase 5); no auto-update in P0 (manual download, like Darkshot's update check that only notifies) | Unsigned builds trigger SmartScreen/Gatekeeper until signing is set up |
| T-15 | B1 | R | User cannot tell whether something was captured | Tray state (normal/paused/private/unavailable); "last skipped reason" counter in Settings without content | — |
| T-16 | B2 | I | Linux PRIMARY selection captures every text highlight (passwords selected in a terminal) | PRIMARY capture off by default; when on, the same markers and detection apply | — |
| T-17 | B1 | I | Source-app attribution stores window titles (document names) | Model has no window-title field (schema rejects it) | — |
| T-18 | B2 | D | Clipboard locked indefinitely by a buggy app | Bounded retries; drop the change; never block the UI thread | Missed capture (logged by code) |

## 4. Security requirements derived (traceable to tasks)

SR-01 mandatory markers (T-01, T-02) · SR-02 content-free logging, enforced by check (T-03) · SR-03 size caps before allocation (T-06) · SR-04 no HTML rendering, no execution of clip content (T-07) · SR-05 secure delete (T-05) · SR-06 file permissions (T-04) · SR-07 MCP scopes and audit (T-09, T-10, T-11) · SR-08 IPC peer checks (T-12) · SR-09 no implicit AI upload (T-13) · SR-10 `cargo deny` in CI (T-14) · SR-11 no window titles (T-17) · SR-12 PRIMARY off by default (T-16).
