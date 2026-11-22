# Task 006: Platform traits, capability model, FakePlatform

**Agent:** `builder` · **Model:** sonnet · **Branch:** `task/006-platform-traits-fake` · **Review:** `specialist` (opus). This is the contract for three adapters.
**Global Constraints:** GC-01, GC-04, GC-05, GC-06, GC-10.

## Read first
`docs/adr/0004-platform-abstraction.md` (the Interfaces there are normative), `docs/architecture/platform-compatibility.md` §1 rows, Task 003 `PrivacyMarker`, Task 004 `RawRepresentation`.

## Files
Create `crates/darkclip-platform/src/{lib.rs, caps.rs, traits.rs, error.rs, fake.rs}`.

## Interfaces
Exactly ADR-0004's code block, with these additions:
```rust
pub struct ChangeSignal { pub seq: u64, pub at: Timestamp }
pub struct PlatformFormat { pub name: String, pub mime: Option<Mime>, pub size_hint: Option<u64> }
pub struct PlatformBundle { pub monitor: Box<dyn ClipboardMonitor>, pub access: Box<dyn ClipboardAccess>,
                            pub source: Box<dyn SourceResolver>, pub paste: Option<Box<dyn PasteInjector>>, pub caps: Capabilities }
// fake.rs (also compiled for downstream tests via feature `fake`)
pub struct FakePlatform { /* shared state behind Arc<Mutex<..>> */ }
impl FakePlatform {
    pub fn new() -> (Self, FakeHandle);
    pub fn bundle(&self) -> PlatformBundle;
}
pub struct FakeHandle;  // test driver
impl FakeHandle {
    /// Simulates another app copying. Bumps seq, sets formats/markers/source, emits one signal.
    pub fn copy(&self, c: FakeCopy) -> u64;
    /// Emits n extra signals without changing seq (Windows burst/OLE behaviour).
    pub fn signal_again(&self, n: usize);
    /// Changes formats without a new seq (OLE delayed rendering phase 2).
    pub fn render(&self, formats: Vec<(PlatformFormat, Vec<u8>)>);
    pub fn set_busy(&self, times: u32);      // next `times` calls return PlatformError::Busy
    pub fn clear_source(&self);              // OLE flush: owner gone
    pub fn current(&self) -> Vec<RawRepresentation>;   // what's on the fake clipboard (for copy-back asserts)
    pub fn read_calls(&self) -> u32;          // how many times read() was invoked (privacy tests)
}
pub struct FakeCopy { pub formats: Vec<(PlatformFormat, Vec<u8>)>, pub markers: Vec<PrivacyMarker>, pub source: Option<ClipboardSource> }
```
`FakeHandle::read_calls` is how Task 010 proves that excluded content is never read.

### Added after Task 003 security review (D-021)
1. **Read caps are part of the contract.** Add `pub image_max_bytes: u64` to `darkclip_core::policy::ReadPlan` (set from `caps.image_max_bytes` in `decide_pre_read`; update the 003 tests that build a `ReadPlan`). Document on `ClipboardAccess::read`: every non-image format (plain, HTML, RTF, uri-list) is capped at `plan.truncate_text_at` bytes, and every image at `plan.image_max_bytes`. Over-cap images return `PlatformError::TooLarge`; over-cap text is truncated on a UTF-8 boundary and flagged. `FakePlatform::read` enforces exactly this. Tests: `fake_read_truncates_html_and_uri_list`, `fake_read_rejects_oversize_image_even_without_size_hint`.
2. **KDE hint compare is case-insensitive** (fail-closed): in `markers.rs`, `secret` matches with `eq_ignore_ascii_case`. Test `linux_kde_secret_any_case` (`b"Secret"`, `b"SECRET"`).
Files added for these: `crates/darkclip-core/src/policy.rs`, `crates/darkclip-core/src/markers.rs` (only these changes).

## Tests
- `fake_copy_emits_signal_with_new_seq`.
- `signal_again_keeps_seq`.
- `snapshot_meta_does_not_count_as_read`.
- `write_returns_new_seq_and_updates_current`.
- `busy_then_ok`.
- `caps_debug_and_serde` (Capabilities serializes for the UI's About/diagnostics).

## Not doing
Real adapters.
