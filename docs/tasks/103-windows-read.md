# Task 103: Windows `read(plan)` with caps

**Agent:** `specialist` · **Model:** opus · **Branch:** `task/103-windows-read` (from `main`) · **Review:** `reviewer` (opus)
**Global Constraints:** GC-03, GC-06, GC-10, GC-13. **Plan:** Task 103 row; Review Focus 2 and 3; amendment S-5.

## Read first
`ClipboardAccess::read` doc comment in `crates/darkclip-platform/src/traits.rs` (cap rules are normative), `crates/darkclip-core/src/model.rs` (`RawRepresentation`), Task 102's OS seam in `crates/darkclip-windows/src/`.

## Files
Create `crates/darkclip-windows/src/{dib.rs, text.rs}` (pure, compiled on all OSes); modify `src/{access.rs, win.rs}`. Add `png` (same major as storage uses) with `cargo add`.

## Requirements
1. `read(plan)`: one `OpenClipboard` attempt (Busy otherwise). Read **only** the formats named in `plan.formats` (by `platform_format`), never others. For each, check `GlobalSize` **before** locking/copying.
2. **Text** (`CF_UNICODETEXT`): bounded raw read (at most `min(GlobalSize, 4 × truncate_text_at + 2)` bytes of UTF-16), stop at the first NUL, convert to UTF-8 (lossy for lone surrogates), truncate on a char boundary at `truncate_text_at`, set `truncated`. Intermediate UTF-16 and over-cap buffers are wiped (`zeroize` is allowed: `cargo add zeroize`, or a manual volatile wipe) before drop.
3. **HTML Format** stored raw (the whole CF_HTML payload incl. header, as UTF-8 bytes, capped/truncated like text). Plain text never comes from HTML (Review Focus 3). **Rich Text Format**: raw bytes, capped.
4. **Images:** `PNG` registered format → bytes as-is (validate PNG signature, else skip that rep). `CF_DIBV5`/`CF_DIB` → PNG via `dib.rs`: BITMAPINFOHEADER/V5 parse, 24/32-bit, bottom-up and top-down (negative height), BI_RGB and BI_BITFIELDS (32-bit masks), alpha from V5 when present else opaque; reject other bit depths/compressions as `Unsupported`. Size check against `image_max_bytes` on the source size and on `width × height × 4` **before** allocating → `TooLarge`. Width or height 0 or > 32,768, or a header that claims more bytes than `GlobalSize` → error, never panic. Set `width`/`height` on the rep.
5. **`CF_HDROP`** → `text/uri-list` (`DragQueryFileW`; `file:///` URIs, percent-encoded per RFC 8089, CRLF-separated), capped like text.
6. `RawRepresentation.platform_format` = the format name read. Errors carry no content (GC-06).

## Tests
- Pure DIB fixtures (built in-test): `dib_bottom_up_24bit`, `dib_top_down_32bit`, `dibv5_alpha_preserved`, `dib_1x1`, `dib_16k_wide_too_large` (cap), `dib_header_lies_about_size` (error, no panic), `dib_bitfields_32bit`, `dib_8bit_unsupported`. Each decodes the produced PNG and compares pixels.
- Pure text: `utf16_truncates_on_char_boundary`, `utf16_stops_at_nul`, `utf16_lone_surrogate_lossy`, `uri_list_percent_encodes_unicode_and_spaces`.
- With the fake OS seam: `reads_only_planned_formats`, `size_checked_before_copy` (a huge `GlobalSize` on an image → `TooLarge` without a data copy call), `busy_once`.
- `#[ignore = "writes the real clipboard"]`: `real_clipboard_write_text_roundtrip_vietnamese`, `real_clipboard_write_html_non_ascii`. Do not run ignored tests.

## Not doing
RGBA decode for hashing and thumbnails (103b, in app), `write` (104).
