# Task 103b: Image decode, canonical hash and thumbnails in the app

**Agent:** `builder` · **Model:** sonnet · **Branch:** `task/103b-app-image-decode` (from `main`) · **Review:** `reviewer` (opus; dedupe and resource caps)
**Global Constraints:** GC-01 (blocking: `png` is pure Rust and allowed; no `image` crate with OS features), GC-06, GC-10. **Plan:** amendment S-3/F-05.

## Read first
`crates/darkclip-core/src/hash.rs` (`canonical_hash`), `crates/darkclip-app/src/capture.rs` (where reps are classified and hashed, `NewClip`), `crates/darkclip-storage/src/clips.rs` (`thumbnail_png`, `valid_thumbnail`, 256 px bound).

## Files
Create `crates/darkclip-app/src/image.rs`; modify `crates/darkclip-app/src/{capture.rs, lib.rs}`, `crates/darkclip-app/Cargo.toml` (`png`, same version as storage).

## Requirements
1. `decode_png(bytes, max_pixels: u64 /* 64 MP */, max_rgba_bytes: u64 /* 256 MiB */) -> Result<DecodedImage { rgba: Vec<u8>, width, height }, ImageError>`: read the header first and reject over-cap sizes **before** allocating; normalise every PNG colour type and bit depth to 8-bit RGBA. Errors carry no content.
2. `thumbnail(&DecodedImage, max_side: u32 /* 256 */) -> Vec<u8>` PNG, aspect kept, box or bilinear downscale, never upscales.
3. Capture: for an `image/png` rep, decode → pass `Some((&rgba, w, h))` to `canonical_hash`, set `thumbnail_png`, and set `width`/`height` on the rep if missing. A decode failure keeps today's behaviour (fallback hash tag) and the clip is still stored; record a skip only if the policy says so (it does not). Decode happens after the policy decision and after sensitive handling, on the actor thread; the RGBA buffer is dropped right after hashing.
4. Dedupe: the same picture copied as DIB (PNG produced by the adapter) and as a registered `PNG` (different encoder, same pixels) gives one clip.

## Tests
- `decode_rejects_over_pixel_cap_without_allocating` (header claims 100k × 100k).
- `decode_palette_grey_16bit_to_rgba8` (synthetic PNGs encoded in-test).
- `thumbnail_bounded_and_aspect_kept` (1000×10 → 256×3 rounded ≥ 1; 10×10 stays 10×10).
- `same_pixels_different_encoding_dedupes` (two PNG encodings of one image via fake copies → one clip, `ClipBumped` on the second).
- `corrupt_png_still_stored_with_fallback_hash`.
- `image_clip_has_thumbnail_and_dimensions`.

## Not doing
Adapter changes; UI.
