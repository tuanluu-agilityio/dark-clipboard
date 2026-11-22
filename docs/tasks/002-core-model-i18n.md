# Task 002: Core domain model, errors, i18n table

**Agent:** `builder` · **Model:** sonnet · **Branch:** `task/002-core-model-i18n` · **Review:** `reviewer` (sonnet)
**Global Constraints:** GC-02, GC-06, GC-10, GC-14.

## Read first
`docs/architecture/data-model.md` §2–3, `docs/architecture/schemas/clip.schema.json`, `docs/architecture/examples/clip.*.json`.

## Files
Create `crates/darkclip-core/src/{model.rs, ids.rs, time.rs, error.rs, i18n.rs, redact.rs}`; modify `lib.rs`, `Cargo.toml` (add `serde` with `derive`, `serde_json`, `thiserror`, `uuid` with `v7`, `serde`).

## Interfaces (contract for 003–011)
```rust
// ids.rs
pub struct ClipId(uuid::Uuid);  pub struct CollectionId(uuid::Uuid);  pub struct TagId(uuid::Uuid);
impl ClipId { pub fn new_v7(now: Timestamp) -> Self; pub fn parse(s: &str) -> Result<Self, CoreError>; }
// time.rs
pub struct Timestamp(pub i64);              // Unix ms, UTC
pub trait Clock: Send + Sync { fn now(&self) -> Timestamp; }
impl Timestamp { pub fn to_rfc3339(self) -> String; pub fn parse_rfc3339(s: &str) -> Result<Self, CoreError>; }
// model.rs
pub enum ClipType { Text, RichText, Url, Color, Code, Image, Files }
pub enum Mime { TextPlain, TextHtml, TextRtf, ImagePng, UriList }
pub enum Storage { Inline, Blob }
pub struct Representation { pub mime: Mime, pub platform_format: Option<String>, pub size_bytes: u64,
                            pub storage: Storage, pub blob_sha256: Option<[u8; 32]>, pub width: Option<u32>, pub height: Option<u32> }
pub enum SensitiveCategory { ApiKey, Token, PrivateKey, Password, PaymentCard, Email, Phone }
pub enum SensitivityLevel { None, Masked }
pub struct Sensitivity { pub level: SensitivityLevel, pub categories: Vec<SensitiveCategory> }
pub enum Platform { Windows, Macos, LinuxX11, LinuxWayland }
pub enum SourceConfidence { Owner, Frontmost, Declared }
pub struct ClipboardSource { pub app_id: String, pub app_name: Option<String>, pub platform: Platform, pub confidence: SourceConfidence }
pub struct ClipMetadata { pub char_count: Option<u64>, pub line_count: Option<u64>, pub file_count: Option<u64>,
                          pub language_hint: Option<String>, pub image_thumb_sha256: Option<[u8; 32]>, pub truncated: bool }
pub struct Clip { /* every field of data-model §3 Clip, P1 fields included as Option/Vec */ }
pub struct RetentionPolicy { .. }  impl Default for RetentionPolicy  // values of examples/retention-policy.default.json
pub enum CaptureMode { Normal, Paused { until: Timestamp }, Private }
pub struct ExcludedApp { pub app_id: String, pub platform: Platform, pub name: Option<String>, pub builtin: bool }
pub enum SensitiveHandling { Drop, StoreMasked, Store }
pub struct PrivacyPolicy { .. }    impl Default for PrivacyPolicy    // values of examples/privacy-policy.default.json
// error.rs
#[derive(thiserror::Error)] pub enum CoreError { InvalidId, InvalidTimestamp, InvalidTag, TooLong { field: &'static str, max: usize } }
// redact.rs
pub struct Redacted<T>(pub T);     // Debug prints "<redacted>"
// i18n.rs
pub enum Lang { En, Vi }
messages! { /* enum Msg + fn t(lang, msg) -> &'static str, both languages mandatory */ }
```
Serde: JSON field names and enum values exactly as the schemas (`snake_case`; `Mime` serializes to the MIME strings; hashes as lowercase hex; timestamps as RFC 3339 `Z`).

## Tests (write first)
- `clip_example_roundtrip`: deserialize each `docs/architecture/examples/clip.*.json` (`include_str!`), serialize, deserialize again; equal.
- `defaults_match_examples`: `RetentionPolicy::default()` and `PrivacyPolicy::default()` serialize to JSON equal (as `serde_json::Value`) to the example files.
- `debug_never_prints_content`: a `Clip` with preview `"TOPSECRET-PREVIEW"` and note `"TOPSECRET-NOTE"` → `format!("{:?}", clip)` contains neither.
- `uuid_v7_sorts_by_time`: ids made at t and t+1 ms sort ascending as strings.
- `i18n_complete`: for every `Msg`, `t(En)` and `t(Vi)` are non-empty and differ unless listed in `SAME_IN_BOTH`.
- `tag_name_rules`: `Tag::validate_name("SQL")` → normalized `"sql"`; `"a b"` → `InvalidTag`; 41 chars → `TooLong`.
- `timestamp_rfc3339_roundtrip` for `2026-11-22T04:01:44.903Z`.

## Not doing
Policy, hashing, storage, sensitive detection.
