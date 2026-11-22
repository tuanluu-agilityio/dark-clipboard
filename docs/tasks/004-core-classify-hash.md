# Task 004: Classification, preview, canonical hashing

**Agent:** `builder` · **Model:** sonnet · **Branch:** `task/004-core-classify-hash` · **Review:** `reviewer` (sonnet)
**Global Constraints:** GC-02, GC-06, GC-10.

## Read first
`docs/architecture/data-model.md` §3 (ClipType), §5 (hashing), Interfaces of Task 002.

## Files
Create `crates/darkclip-core/src/{classify.rs, hash.rs, preview.rs}`; add `sha2` to `Cargo.toml`.

## Interfaces
```rust
pub struct RawRepresentation { pub mime: Mime, pub platform_format: String, pub bytes: Vec<u8>, pub width: Option<u32>, pub height: Option<u32> }
// Debug: manual, prints mime, format, len only.
pub struct Classified { pub kind: ClipType, pub preview: String, pub metadata: ClipMetadata, pub plain_text: Option<String> }
pub fn classify(reps: &[RawRepresentation]) -> Classified;
pub fn canonical_hash(kind: ClipType, reps: &[RawRepresentation], decoded_rgba: Option<(&[u8], u32, u32)>) -> [u8; 32];
pub fn make_preview(text: &str) -> String;     // ≤ 512 chars, whitespace runs collapsed, no newlines
pub fn image_preview(w: u32, h: u32) -> String; // "Image 1280×720" (i18n-free; the UI localizes the "Image" word)
```
Rules: `Url` = trimmed text with no whitespace, scheme `http`/`https`/`ftp`/`mailto`/`file`, length ≤ 2048. `Color` = `#rgb`, `#rrggbb`, `#rrggbbaa`, `rgb(...)`, `rgba(...)`, `hsl(...)`. `Code` is P1 (always false now, keep the variant). `RichText` when HTML or RTF accompanies plain text. `Files` from `UriList` (count lines not starting with `#`). Hash prefix `b"dc1\0"`, CRLF → LF for text, files sorted.

## Tests
- `url_detection` (positive: `https://example.com/a?b=1`; negative: `see https://x.y`, `example.com`).
- `color_detection` (`#1e222a`, `rgb(1, 2, 3)`; negative `#12345`).
- `rich_text_when_html_present`.
- `crlf_and_lf_hash_equal`; `trailing_newline_differs`.
- `same_pixels_same_hash_regardless_of_format` (two reps with different `platform_format`, same RGBA).
- `files_order_independent_hash`.
- `preview_truncates_to_512_chars_on_char_boundary` (Vietnamese text with combining marks, no panic, valid UTF-8).
- `preview_collapses_whitespace`.
- `raw_representation_debug_hides_bytes`.

## Not doing
Image decoding (the adapter or app supplies decoded RGBA); storage.
