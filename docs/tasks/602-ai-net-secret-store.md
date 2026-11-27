# Task 602: `ai_net.rs` transport and the `SecretStore` (Windows Credential Manager)

**Agent:** `specialist` · **Model:** opus · **Branch:** `task/602-ai-net-secret-store` (from `main`) · **Review:** `security-reviewer` (opus)
**Global Constraints:** GC-01, GC-06, GC-08 (as amended by D-094), GC-18, GC-19. Plan: `docs/plans/phase-6-translate-in-place.md` (Global Constraints, Interfaces §602). Spec: `docs/specs/translate-in-place/design.md` §5.2, §6.4. Threat model: T-20, T-21 (`docs/security/threat-model.md`).

## Goal
Two building blocks. Task 605 wires them; nothing calls them yet.
1. `apps/desktop/src/ai_net.rs`: the only HTTP client for translate in place. It implements `darkclip_ai::Transport` and can reach exactly one URL.
2. A `SecretStore` platform trait with an in-memory fake, and a Windows implementation on Credential Manager that holds the Anthropic key outside settings, logs and backups.

## Interfaces (fixed by the plan, §602; do not rename)
```rust
// apps/desktop/src/ai_net.rs
pub(crate) const MESSAGES_URL: &str = "https://api.anthropic.com/v1/messages";
pub(crate) struct UreqTransport { /* ureq::Agent */ }
impl UreqTransport { pub(crate) fn new() -> Self; }
impl darkclip_ai::Transport for UreqTransport { fn post_messages(&self, body: &[u8], headers: &[(&'static str, String)]) -> Result<Vec<u8>, TransportError>; }

// crates/darkclip-platform/src/traits.rs (re-export from lib.rs)
pub const ANTHROPIC_KEY_TARGET: &str = "dark-clipboard/anthropic";
pub trait SecretStore: Send + Sync {
    fn get(&self, target: &str) -> Result<Option<Redacted<String>>, PlatformError>;
    fn set(&self, target: &str, secret: &Redacted<String>) -> Result<(), PlatformError>;
    fn delete(&self, target: &str) -> Result<(), PlatformError>;   // Ok when absent
}
// crates/darkclip-platform/src/fake.rs (behind the existing `fake` feature)
pub struct FakeSecretStore;   // in-memory map behind a Mutex; Default
// crates/darkclip-windows
pub struct WinSecretStore;    // Default; unsafe only in win.rs
```

## Facts
- `apps/desktop/src/update.rs` `UreqFetch` (ureq 3.5, rustls) is the model: `Agent::config_builder()` with `timeout_global`, `https_only(true)`, `max_redirects(0)`, a fixed User-Agent, and `body_mut().with_config().limit(..)`. Reuse the same patterns. Do **not** share an agent or helper with `update.rs` (A6: no shared network path).
- ureq 3 returns 4xx/5xx as `Err(ureq::Error::StatusCode(..))` by default. To read the error body (the Messages API puts the reason there), configure `http_status_as_error(false)` and map non-2xx to `TransportError::Status { code, body }`, with the body read under the same cap.
- Credential Manager: `CredWriteW`, `CredReadW`, `CredDeleteW` and `CredFree` (`Win32_Security_Credentials`, a new `windows` feature in `crates/darkclip-windows/Cargo.toml`).
  - Use a generic credential (`CRED_TYPE_GENERIC`) with `CRED_PERSIST_LOCAL_MACHINE`: this user, this machine, not roaming.
  - `UserName` is `"anthropic"`. The blob is the UTF-8 key bytes.
  - `ERROR_NOT_FOUND` on read gives `Ok(None)`, and on delete gives `Ok(())`. Any other failure gives `PlatformError::Backend { code }` (the Win32 code only).
- `crates/darkclip-windows`: all `unsafe` lives in `win.rs`, and the adapters never log (GC-06).
- `darkclip-platform` must stay portable (GC-01), so the trait and the fake only.

## Behaviour and rules
- `post_messages`:
  - POST to `MESSAGES_URL` only. The URL is never a parameter, a setting or an environment variable.
  - Set exactly the given headers. Timeout 30 s for the whole call, HTTPS only, 0 redirects, response cap 256 KiB.
  - User-Agent `DarkClipboard/<CARGO_PKG_VERSION>`.
  - Map errors: a timeout gives `Timeout`; DNS, connect and TLS failures give `Network`; over the cap gives `TooLarge`; non-2xx gives `Status { code, body }`.
  - Proxy behaviour: the same as `update.rs` (ureq's environment-proxy defaults). Write that down in the module doc and in the privacy-design T-21 note if it differs.
- No logging of request or response bytes, headers or the key, anywhere. Errors carry no content.
- `WinSecretStore::set` writes the secret bytes once. `get` copies the blob into a `String` (invalid UTF-8 gives `Backend { code: 0 }`) and calls `CredFree` on every path. Nothing is cached in the struct.
- The key never appears in `Debug` (`Redacted`), errors or logs.
- `ai_net.rs` is declared in `apps/desktop/src/main.rs`. Until 605 uses it, allow dead code on the module with a comment that names Task 605.

## Tests (write first)
- `ai_net.rs` unit tests (no network):
  - `messages_url_is_pinned`: the URL equals the constant, starts with `https://api.anthropic.com/`, and the source of `ai_net.rs` contains no other `http` URL. Read the file with `include_str!` and count `"http`.
  - `agent_config_is_strict`: if ureq 3 exposes the built config, assert `https_only`, `max_redirects == 0` and the timeout. If it does not, factor the config into a `fn config() -> ureq::config::Config` and assert on that.
  - `status_mapping`: a pure helper `fn map_response(status: u16, body: Vec<u8>) -> Result<Vec<u8>, TransportError>` is tested for 200 → Ok, 401 → `Status{401,..}` and 529 → `Status{529,..}`.
- `darkclip-platform` fake: `fake_secret_store_round_trip`, `fake_delete_absent_is_ok`, `fake_get_absent_is_none`.
- `darkclip-windows`:
  - `#[ignore]` test `win_secret_store_round_trip`: uses the target name `dark-clipboard/test-602-<pid>` (never the real `ANTHROPIC_KEY_TARGET`), with set, get, overwrite, get, delete, get (None) and delete-again (Ok). The secret is built at runtime.
  - **Run it yourself once** with `cargo test -p darkclip-windows -- --ignored win_secret_store_round_trip`. It only touches that test credential, which it deletes; that is allowed under GC-19, because it neither writes the clipboard nor sends input. Record the command and output in the report.
  - Afterwards verify the credential is gone with `cmdkey /list:dark-clipboard/test-602*` and record the output.
- `debug_hides_secret`: `format!("{:?}", store.get(..))` on the fake contains no part of the secret.

## Files this task may edit
`apps/desktop/src/ai_net.rs` (new), `apps/desktop/src/main.rs` (module line), `apps/desktop/Cargo.toml` (add `darkclip-ai` path dependency), `crates/darkclip-platform/src/{traits.rs,fake.rs,lib.rs}`, `crates/darkclip-windows/{Cargo.toml,src/lib.rs,src/win.rs,src/secret.rs (new)}`, `Cargo.lock`, `docs/security/threat-model.md` (T-21 residual risks only, if something differs), `docs/security/privacy-design.md` (proxy sentence in the D-094 note, if needed), `memory-bank/techContext.md` (the new `windows` feature), `memory-bank/progress.md` (the 602 row).

## Acceptance
- All tests above pass; the ignored round trip was run once, with output recorded.
- `cargo clippy --workspace --all-targets -- -D warnings` is clean on Windows.
- Non-Windows builds compile: `darkclip-windows` stays `cfg`-gated as today, and `ai_net.rs` is platform-neutral.
- `powershell -File scripts/check.ps1` prints `ALL CHECKS PASSED`. The GC-08 `.rs` check passes with `ai_net.rs` and still fails for any other file.

## Known pitfalls (from `memory-bank/systemPatterns.md`)
- **Windows-only code must lint on macOS/Linux too** (F-19): items used only under `#[cfg(windows)]` need `cfg_attr(not(windows), allow(dead_code))` or their own `cfg`. CI runs clippy `-D warnings` on macOS and Ubuntu.
- **No panics in release paths:** no `unwrap`/`expect` outside tests.
- **Tests first; synthetic secrets assembled at runtime** (never a key-shaped literal).
- **Manual `Debug`** for anything holding a secret or content (GC-06). Adapters never log.
- **All `unsafe` in `win.rs`** with a `// SAFETY:` comment per block (existing convention in `darkclip-windows`).
- **GC-19:** never write the real clipboard, never send input, never run the app against the default data dir. The Credential Manager test uses only its own test target and deletes it.
