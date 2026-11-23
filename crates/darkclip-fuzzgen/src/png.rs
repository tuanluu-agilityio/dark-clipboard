//! Hand-built PNG files for regression tests: every chunk is written explicitly, so a test
//! can hold a malformed chunk that no encoder would produce.

use crate::crc32;

/// Appends one chunk (length, type, data, CRC).
pub fn push_chunk(out: &mut Vec<u8>, kind: &[u8; 4], data: &[u8]) {
    out.extend_from_slice(&u32::try_from(data.len()).unwrap_or(u32::MAX).to_be_bytes());
    out.extend_from_slice(kind);
    out.extend_from_slice(data);
    let mut crc_input = kind.to_vec();
    crc_input.extend_from_slice(data);
    out.extend_from_slice(&crc32(&crc_input).to_be_bytes());
}

/// A 1x1, 8-bit indexed PNG whose single pixel is palette index 0, with `plte` as the PLTE
/// chunk data, whatever its length. The image data is a stored (uncompressed) zlib stream.
pub fn indexed_1x1(plte: &[u8]) -> Vec<u8> {
    let mut out = b"\x89PNG\r\n\x1a\n".to_vec();
    // Width 1, height 1, depth 8, colour type 3 (indexed), deflate, filter 0, no interlace.
    push_chunk(&mut out, b"IHDR", &[0, 0, 0, 1, 0, 0, 0, 1, 8, 3, 0, 0, 0]);
    push_chunk(&mut out, b"PLTE", plte);
    // zlib header, one final stored block of 2 bytes (filter 0, index 0), Adler-32.
    let idat = [
        0x78, 0x01, 0x01, 0x02, 0x00, 0xFD, 0xFF, 0x00, 0x00, 0x00, 0x02, 0x00, 0x01,
    ];
    push_chunk(&mut out, b"IDAT", &idat);
    push_chunk(&mut out, b"IEND", &[]);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn well_formed_indexed_png_decodes() {
        let png = indexed_1x1(&[10, 20, 30]);
        // The IEND chunk carries the well-known CRC.
        assert_eq!(&png[png.len() - 4..], &[0xAE, 0x42, 0x60, 0x82]);
    }
}
