# Task 005: Sensitive-data detection (secondary safeguard)

**Agent:** `specialist` · **Model:** opus · **Branch:** `task/005-core-sensitive` · **Review:** `security-reviewer` (opus)
**Global Constraints:** GC-02, GC-06 (no logging in `sensitive/`, including tests), GC-09, GC-10, GC-12 (no regex crate).

## Read first
`docs/security/privacy-design.md` §4. Darkshot's `crates/darkshot-core/src/privacy/detect/*` may be read **for test-corpus ideas only**. Do not copy code: it is OCR-oriented and owned by another product.

## Files
Create `crates/darkclip-core/src/sensitive/{mod.rs, api_key.rs, token.rs, private_key.rs, password.rs, card.rs, email.rs, phone.rs, wipe.rs}`.

## Interfaces
```rust
pub struct ScanResult { pub categories: Vec<SensitiveCategory>, pub mask_hint: Option<MaskHint> }  // no matched text
pub struct MaskHint { pub visible_prefix: String /* ≤ 3 chars */, pub category: SensitiveCategory }
pub fn scan(text: &str, enabled: &[SensitiveCategory]) -> ScanResult;
pub fn masked_preview(hint: &MaskHint, lang: Lang) -> String;   // "sk-…•••• (API key)"
pub(crate) fn wipe(s: &mut String);                              // zero bytes before drop
```
Limits: scan at most the first 256 KiB of text. Linear time (no backtracking). Each detector is in its own file and implements `trait Detector { fn scan(&self, text: &str, out: &mut Vec<Hit>); }` (crate-private).

## Tests (synthetic data; secret-shaped strings built at runtime with `concat!`)
- One positive and one negative test per category; the API-key prefixes `sk-`, `sk-proj-`, `ghp_`, `github_pat_`, `xoxb-`, `AKIA` (+16 uppercase alnum), `AIza` (+35), `glpat-`.
- `jwt_requires_decodable_header` (`eyJhbGciOiJIUzI1NiJ9.<payload>.<sig>` positive; `eyJ` + garbage negative).
- `pem_private_key` (`-----BEGIN OPENSSH PRIVATE KEY-----`), `pem_public_key_is_not_sensitive`.
- `password_after_label` for `password: hunter2`, `pwd=abc123`, `mật khẩu: abc`; `password_label_alone_negative`.
- `luhn_card` (`4111 1111 1111 1111`, `3782 822463 10005`); `non_luhn_16_digits_negative`; `phone_and_email_disabled_by_default` (`PrivacyPolicy::default()` categories).
- False-positive corpus (must be empty): git SHA-40, UUID, semver `1.2.3`, IPv4, ISO date, money `1,234,567.89`, `@mention`, `x@2x.png`, base64 image prefix `iVBORw0KGgo`.
- `scan_result_has_no_text`: `format!("{:?}", scan(...))` does not contain the secret.
- `large_input_bounded`: 10 MiB of `a` returns in < 200 ms (debug build: < 2 s) and scans only 256 KiB.

## Not doing
UI masking, storage of masked clips (010/007).
