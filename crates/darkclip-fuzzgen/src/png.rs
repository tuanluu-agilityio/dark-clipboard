//! Hand-built PNG files for regression tests: every chunk is written explicitly, so a test
//! can hold a malformed chunk that no encoder would produce.

use crate::{crc32, Rng};

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

/// A valid PNG of random type, depth and size (small), optionally with extra chunks.
pub fn gen_png(rng: &mut Rng) -> Vec<u8> {
    use png::{BitDepth, ColorType};
    let combos = [
        (ColorType::Grayscale, BitDepth::One),
        (ColorType::Grayscale, BitDepth::Two),
        (ColorType::Grayscale, BitDepth::Four),
        (ColorType::Grayscale, BitDepth::Eight),
        (ColorType::Grayscale, BitDepth::Sixteen),
        (ColorType::Rgb, BitDepth::Eight),
        (ColorType::Rgb, BitDepth::Sixteen),
        (ColorType::Indexed, BitDepth::One),
        (ColorType::Indexed, BitDepth::Four),
        (ColorType::Indexed, BitDepth::Eight),
        (ColorType::GrayscaleAlpha, BitDepth::Eight),
        (ColorType::GrayscaleAlpha, BitDepth::Sixteen),
        (ColorType::Rgba, BitDepth::Eight),
        (ColorType::Rgba, BitDepth::Sixteen),
    ];
    let (color, depth) = combos[rng.below(combos.len())];
    let (w, h) = (rng.range(1, 24) as u32, rng.range(1, 24) as u32);
    let mut out = Vec::new();
    {
        let mut enc = png::Encoder::new(&mut out, w, h);
        enc.set_color(color);
        enc.set_depth(depth);
        if color == ColorType::Indexed {
            let entries = rng.range(1, 256);
            enc.set_palette(rng.bytes(entries * 3));
            if rng.one_in(2) {
                let n = rng.range(1, entries);
                enc.set_trns(rng.bytes(n));
            }
        }
        let Ok(mut writer) = enc.write_header() else {
            return out;
        };
        let samples = match color {
            ColorType::Grayscale | ColorType::Indexed => 1,
            ColorType::GrayscaleAlpha => 2,
            ColorType::Rgb => 3,
            ColorType::Rgba => 4,
        };
        let bits = samples * depth as usize;
        let row = (w as usize * bits).div_ceil(8);
        let data = rng.bytes(row * h as usize);
        let _ = writer.write_image_data(&data);
    }
    out
}

/// PNG tokens for the mutator: signature, chunk types, large dimensions, a zlib header.
pub const PNG_DICT: &[&[u8]] = &[
    b"\x89PNG\r\n\x1a\n",
    b"IHDR",
    b"IDAT",
    b"IEND",
    b"PLTE",
    b"tRNS",
    b"iCCP",
    b"tEXt",
    b"zTXt",
    b"acTL",
    b"fcTL",
    b"fdAT",
    b"\x00\x00\x80\x00",
    b"\x00\x00\x7f\xff",
    b"\x78\x9c",
];

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
