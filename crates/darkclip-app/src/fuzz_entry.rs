//! Fuzz entry points (Task 406). Built only for tests and with the `fuzzing` feature (the
//! `fuzz/` cargo-fuzz targets enable it); never part of a normal build.
//!
//! The function takes arbitrary bytes, drives the capture-side PNG decoder with the shared
//! core limits and checks its invariants with `assert!`; the messages never include the
//! input (GC-06). The `#[ignore]` stable harness in `harness` calls the same function.

use crate::image::{
    decode_png, downscale, thumbnail, ImageError, MAX_PIXELS, MAX_RGBA_BYTES, THUMB_MAX_SIDE,
};

/// PNG → RGBA with the core caps, then the thumbnail.
pub fn png_decode(data: &[u8]) {
    match decode_png(data, MAX_PIXELS, MAX_RGBA_BYTES) {
        Ok(img) => {
            let px = u64::from(img.width) * u64::from(img.height);
            assert!(img.width > 0 && img.height > 0, "zero dimension");
            assert!(px <= MAX_PIXELS, "pixel cap");
            assert_eq!(img.rgba.len() as u64, px * 4, "RGBA length");
            let small = downscale(&img, THUMB_MAX_SIDE);
            assert!(small.is_some(), "a decoded image downscales");
            if let Some(s) = small {
                assert!(s.width.max(s.height) <= THUMB_MAX_SIDE, "thumbnail side");
                assert!(
                    s.width <= img.width && s.height <= img.height,
                    "never upscaled"
                );
            }
            let thumb = thumbnail(&img, THUMB_MAX_SIDE);
            assert!(!thumb.is_empty(), "thumbnail encodes");
        }
        Err(ImageError::TooLarge | ImageError::Corrupt) => {}
    }
}

#[cfg(test)]
mod harness;
