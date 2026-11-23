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

/// Largest PLTE chunk: 256 entries of 3 bytes.
const MAX_PLTE_LEN: u32 = 256 * 3;

/// Whether every `PLTE` chunk of `png` holds 1 to 256 whole entries (a length that is a
/// non-zero multiple of 3, at most 768). Decoders call this before handing the bytes to the
/// `png` crate: version 0.18.1 panics while expanding a palette whose length is not a
/// multiple of 3 (fuzz finding, Task 406), and a panic aborts the release build. It already
/// refuses lengths outside 3..=768 (`ChunkLengthWrong`); they are refused here as well so
/// the check does not depend on that.
///
/// Reads chunk headers only, in one linear pass, without allocating. Bytes that are not a
/// PNG, or a chunk that runs past the end, end the walk with `true`: the decoder refuses
/// those itself before it reads a palette.
pub fn png_palette_ok(png: &[u8]) -> bool {
    // After the 8-byte signature: length (4, big endian), type (4), data, CRC (4).
    let mut at: usize = 8;
    loop {
        let Some(head) = at.checked_add(8).and_then(|end| png.get(at..end)) else {
            return true;
        };
        let len = head.get(..4).and_then(|b| <[u8; 4]>::try_from(b).ok());
        let (Some(len), Some(kind)) = (len, head.get(4..)) else {
            return true;
        };
        let len = u32::from_be_bytes(len);
        match kind {
            b"PLTE" if len == 0 || len % 3 != 0 || len > MAX_PLTE_LEN => return false,
            b"IEND" => return true,
            _ => {}
        }
        let Some(next) = usize::try_from(len)
            .ok()
            .and_then(|l| at.checked_add(12)?.checked_add(l))
        else {
            return true;
        };
        at = next;
    }
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

    #[test]
    fn png_palette_lengths() {
        fn png(chunks: &[(&[u8; 4], u32)]) -> Vec<u8> {
            let mut out = b"\x89PNG\r\n\x1a\n".to_vec();
            for (kind, len) in chunks {
                out.extend_from_slice(&len.to_be_bytes());
                out.extend_from_slice(*kind);
                out.extend(std::iter::repeat_n(0u8, *len as usize + 4));
            }
            out
        }
        for ok in [3, 6, 765, 768] {
            assert!(png_palette_ok(&png(&[
                (b"IHDR", 13),
                (b"PLTE", ok),
                (b"IEND", 0)
            ])));
        }
        for bad in [0, 1, 2, 4, 5, 769, 771, 1024] {
            assert!(!png_palette_ok(&png(&[
                (b"IHDR", 13),
                (b"PLTE", bad),
                (b"IEND", 0)
            ])));
        }
        // No palette, not a PNG, a truncated chunk, a huge length: nothing to refuse here.
        assert!(png_palette_ok(&png(&[(b"IHDR", 13), (b"IEND", 0)])));
        assert!(png_palette_ok(b""));
        assert!(png_palette_ok(b"\x89PNG\r\n\x1a\n\0\0\0\x04PL"));
        let mut huge = png(&[(b"IHDR", 13)]);
        huge.extend_from_slice(&u32::MAX.to_be_bytes());
        huge.extend_from_slice(b"tEXt");
        assert!(png_palette_ok(&huge));
        // A palette after IEND is never read.
        assert!(png_palette_ok(&png(&[(b"IEND", 0), (b"PLTE", 4)])));
    }
}
