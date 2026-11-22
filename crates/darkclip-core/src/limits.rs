//! Decoded-image caps shared by capture (`darkclip-app` PNG decoding) and the adapters (DIB
//! input, copy-back PNG decoding), so every decoder bounds memory the same way (D-043).
//!
//! These bound the **decoded** pixels and are checked from the header before any pixel
//! buffer exists (decompression-bomb guard). They are not the stored-image cap: that is
//! `SizeCaps::image_max_bytes`, which applies to the encoded PNG.

/// Largest decoded image: 64 megapixels.
pub const MAX_PIXELS: u64 = 64 * 1024 * 1024;
/// Largest decoded RGBA buffer: 256 MiB.
pub const MAX_RGBA_BYTES: u64 = 256 * 1024 * 1024;
/// Longest accepted side in pixels.
pub const MAX_SIDE: u32 = 32_768;

/// `width x height` is within [`MAX_PIXELS`] and its RGBA size within [`MAX_RGBA_BYTES`].
/// The side limit is checked separately (callers report it differently).
pub fn within_decode_caps(width: u32, height: u32) -> bool {
    let pixels = u64::from(width) * u64::from(height);
    pixels <= MAX_PIXELS && pixels.saturating_mul(4) <= MAX_RGBA_BYTES
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decode_caps_boundaries() {
        assert!(within_decode_caps(8192, 8192));
        assert!(!within_decode_caps(8193, 8192));
        assert!(within_decode_caps(MAX_SIDE, 2048));
        assert!(!within_decode_caps(MAX_SIDE, 2049));
        assert!(!within_decode_caps(u32::MAX, u32::MAX));
    }
}
