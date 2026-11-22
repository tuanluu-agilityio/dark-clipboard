//! PNG decoding for dedupe and thumbnails (Task 103b).
//!
//! Pure Rust (`png`), no OS features (GC-01). The header is read first and any size over the
//! caps is rejected before a pixel buffer exists, because the adapter's width and height come
//! from untrusted IHDR bytes (decompression-bomb guard). Errors carry no content (GC-06).

use png::{BitDepth, ColorType, Decoder, Limits, Transformations};
use std::io::Cursor;

/// Decoded-size caps used by capture: 64 megapixels, 256 MiB of RGBA.
pub const MAX_PIXELS: u64 = 64 * 1024 * 1024;
pub const MAX_RGBA_BYTES: u64 = 256 * 1024 * 1024;
/// Thumbnail bound (ADR-0003).
pub const THUMB_MAX_SIDE: u32 = 256;

/// Why a PNG was not decoded. No variant holds image data.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImageError {
    /// The header claims more pixels or bytes than the caps allow.
    TooLarge,
    /// Not a decodable PNG (corrupt, truncated, zero-sized).
    Corrupt,
}

/// 8-bit RGBA pixels, row-major, `width * height * 4` bytes.
pub struct DecodedImage {
    pub rgba: Vec<u8>,
    pub width: u32,
    pub height: u32,
}

impl std::fmt::Debug for DecodedImage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DecodedImage")
            .field("width", &self.width)
            .field("height", &self.height)
            .finish()
    }
}

/// Decodes any PNG to 8-bit RGBA. Over-cap sizes fail before any allocation.
pub fn decode_png(
    bytes: &[u8],
    max_pixels: u64,
    max_rgba_bytes: u64,
) -> Result<DecodedImage, ImageError> {
    let limit_bytes = usize::try_from(max_rgba_bytes).unwrap_or(usize::MAX);
    let mut decoder = Decoder::new_with_limits(
        Cursor::new(bytes),
        Limits {
            bytes: limit_bytes.saturating_add(1024 * 1024),
        },
    );
    decoder.set_transformations(Transformations::normalize_to_color8());
    let info = decoder
        .read_header_info()
        .map_err(|_| ImageError::Corrupt)?;
    let (w, h) = (info.width, info.height);
    if w == 0 || h == 0 {
        return Err(ImageError::Corrupt);
    }
    let pixels = u64::from(w) * u64::from(h);
    if pixels > max_pixels || pixels.saturating_mul(4) > max_rgba_bytes {
        return Err(ImageError::TooLarge);
    }
    let mut reader = decoder.read_info().map_err(|_| ImageError::Corrupt)?;
    let (color, depth) = reader.output_color_type();
    if depth != BitDepth::Eight {
        return Err(ImageError::Corrupt);
    }
    // At most 4 bytes per pixel, so already bounded by the cap checked above.
    let mut buf = vec![0u8; reader.output_buffer_size().ok_or(ImageError::TooLarge)?];
    let frame = reader
        .next_frame(&mut buf)
        .map_err(|_| ImageError::Corrupt)?;
    buf.truncate(frame.buffer_size());
    let px = usize::try_from(pixels).map_err(|_| ImageError::TooLarge)?;
    let rgba = match color {
        ColorType::Rgba => buf,
        ColorType::Rgb => {
            let mut out = Vec::with_capacity(px * 4);
            for c in buf.as_chunks::<3>().0 {
                out.extend_from_slice(c);
                out.push(255);
            }
            out
        }
        ColorType::GrayscaleAlpha => {
            let mut out = Vec::with_capacity(px * 4);
            for c in buf.as_chunks::<2>().0 {
                out.extend_from_slice(&[c[0], c[0], c[0], c[1]]);
            }
            out
        }
        ColorType::Grayscale => {
            let mut out = Vec::with_capacity(px * 4);
            for &g in &buf {
                out.extend_from_slice(&[g, g, g, 255]);
            }
            out
        }
        ColorType::Indexed => return Err(ImageError::Corrupt),
    };
    if rgba.len() != px * 4 {
        return Err(ImageError::Corrupt);
    }
    Ok(DecodedImage {
        rgba,
        width: w,
        height: h,
    })
}

/// PNG thumbnail with the longest side at most `max_side`, aspect kept (box filter), never
/// upscaled. Empty on an encoding failure or an inconsistent image.
pub fn thumbnail(img: &DecodedImage, max_side: u32) -> Vec<u8> {
    let (w, h) = (u64::from(img.width), u64::from(img.height));
    let long = w.max(h);
    let max_side = u64::from(max_side.max(1));
    if w == 0 || h == 0 || img.rgba.len() as u64 != w * h * 4 {
        return Vec::new();
    }
    let (tw, th) = if long <= max_side {
        (w, h)
    } else {
        (
            ((w * max_side + long / 2) / long).max(1),
            ((h * max_side + long / 2) / long).max(1),
        )
    };
    let mut out = Vec::with_capacity((tw * th * 4) as usize);
    for ty in 0..th {
        let y0 = ty * h / th;
        let y1 = ((ty + 1) * h).div_ceil(th).clamp(y0 + 1, h);
        for tx in 0..tw {
            let x0 = tx * w / tw;
            let x1 = ((tx + 1) * w).div_ceil(tw).clamp(x0 + 1, w);
            let mut acc = [0u64; 4];
            for y in y0..y1 {
                for x in x0..x1 {
                    let i = ((y * w + x) * 4) as usize;
                    for (a, v) in acc.iter_mut().zip(&img.rgba[i..i + 4]) {
                        *a += u64::from(*v);
                    }
                }
            }
            let n = (y1 - y0) * (x1 - x0);
            for a in acc {
                out.push(((a + n / 2) / n) as u8);
            }
        }
    }
    let mut png_bytes = Vec::new();
    let mut enc = png::Encoder::new(&mut png_bytes, tw as u32, th as u32);
    enc.set_color(ColorType::Rgba);
    enc.set_depth(BitDepth::Eight);
    let ok = enc
        .write_header()
        .and_then(|mut w| w.write_image_data(&out))
        .is_ok();
    if ok {
        png_bytes
    } else {
        Vec::new()
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    fn crc32(data: &[u8]) -> u32 {
        let mut c = 0xFFFF_FFFFu32;
        for &b in data {
            c ^= u32::from(b);
            for _ in 0..8 {
                c = if c & 1 != 0 {
                    (c >> 1) ^ 0xEDB8_8320
                } else {
                    c >> 1
                };
            }
        }
        !c
    }

    pub(crate) fn encode(
        w: u32,
        h: u32,
        color: ColorType,
        depth: BitDepth,
        data: &[u8],
        plte: Option<&[u8]>,
    ) -> Vec<u8> {
        let mut out = Vec::new();
        let mut e = png::Encoder::new(&mut out, w, h);
        e.set_color(color);
        e.set_depth(depth);
        if let Some(p) = plte {
            e.set_palette(p.to_vec());
        }
        let mut wr = e.write_header().unwrap();
        wr.write_image_data(data).unwrap();
        wr.finish().unwrap();
        out
    }

    #[test]
    fn decode_rejects_over_pixel_cap_without_allocating() {
        // Signature + IHDR only (100k x 100k, RGBA8): no pixel data exists at all.
        let mut ihdr = Vec::new();
        ihdr.extend_from_slice(b"IHDR");
        ihdr.extend_from_slice(&100_000u32.to_be_bytes());
        ihdr.extend_from_slice(&100_000u32.to_be_bytes());
        ihdr.extend_from_slice(&[8, 6, 0, 0, 0]);
        let mut file = vec![0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A];
        file.extend_from_slice(&13u32.to_be_bytes());
        file.extend_from_slice(&ihdr);
        file.extend_from_slice(&crc32(&ihdr).to_be_bytes());
        assert_eq!(
            decode_png(&file, MAX_PIXELS, MAX_RGBA_BYTES).err(),
            Some(ImageError::TooLarge)
        );
        // A small image over a tiny cap is rejected the same way.
        let small = encode(4, 4, ColorType::Rgba, BitDepth::Eight, &[0; 64], None);
        assert_eq!(
            decode_png(&small, 8, MAX_RGBA_BYTES).err(),
            Some(ImageError::TooLarge)
        );
        assert_eq!(
            decode_png(&small, MAX_PIXELS, 32).err(),
            Some(ImageError::TooLarge)
        );
    }

    #[test]
    fn decode_palette_grey_16bit_to_rgba8() {
        let p = encode(
            2,
            1,
            ColorType::Indexed,
            BitDepth::Eight,
            &[0, 1],
            Some(&[255, 0, 0, 0, 0, 255]),
        );
        let d = decode_png(&p, MAX_PIXELS, MAX_RGBA_BYTES).unwrap();
        assert_eq!((d.width, d.height), (2, 1));
        assert_eq!(d.rgba, vec![255, 0, 0, 255, 0, 0, 255, 255]);
        let g = encode(
            1,
            1,
            ColorType::Grayscale,
            BitDepth::Sixteen,
            &[0x80, 0x00],
            None,
        );
        let d = decode_png(&g, MAX_PIXELS, MAX_RGBA_BYTES).unwrap();
        assert_eq!(d.rgba, vec![0x80, 0x80, 0x80, 255]);
        let c = encode(
            1,
            1,
            ColorType::Rgb,
            BitDepth::Sixteen,
            &[0x10, 0, 0x20, 0, 0x30, 0],
            None,
        );
        assert_eq!(
            decode_png(&c, MAX_PIXELS, MAX_RGBA_BYTES).unwrap().rgba,
            vec![0x10, 0x20, 0x30, 255]
        );
        let ga = encode(
            1,
            1,
            ColorType::GrayscaleAlpha,
            BitDepth::Eight,
            &[7, 9],
            None,
        );
        assert_eq!(
            decode_png(&ga, MAX_PIXELS, MAX_RGBA_BYTES).unwrap().rgba,
            vec![7, 7, 7, 9]
        );
    }

    #[test]
    fn decode_rejects_corrupt_and_truncated() {
        assert_eq!(
            decode_png(b"not a png", MAX_PIXELS, MAX_RGBA_BYTES).err(),
            Some(ImageError::Corrupt)
        );
        let ok = encode(8, 8, ColorType::Rgba, BitDepth::Eight, &[1; 256], None);
        let cut = &ok[..ok.len() / 2];
        assert!(decode_png(cut, MAX_PIXELS, MAX_RGBA_BYTES).is_err());
    }

    fn solid(w: u32, h: u32) -> DecodedImage {
        DecodedImage {
            rgba: vec![200; (w * h * 4) as usize],
            width: w,
            height: h,
        }
    }

    #[test]
    fn thumbnail_bounded_and_aspect_kept() {
        for ((w, h), want) in [
            ((1000, 10), (256, 3)),
            ((10, 10), (10, 10)),
            ((10, 1000), (3, 256)),
            ((100_000, 1), (256, 1)),
        ] {
            let t = thumbnail(&solid(w, h), 256);
            let d = decode_png(&t, MAX_PIXELS, MAX_RGBA_BYTES).unwrap();
            assert_eq!((d.width, d.height), want);
        }
    }
}
