//! Packed DIB (`CF_DIB` / `CF_DIBV5`) to PNG (Task 103). Pure, no OS calls, compiled on every
//! OS so its tests run everywhere.
//!
//! Supported: `BITMAPINFOHEADER` (40), V2/V3 (52/56), V4 (108) and V5 (124) headers; 24-bit
//! `BI_RGB`, 32-bit `BI_RGB` and `BI_BITFIELDS`; bottom-up and top-down (negative height).
//! Alpha comes from the header's alpha mask (V3 and later) when present, else the image is
//! opaque; an alpha channel that is zero everywhere is treated as absent (writers that set the
//! mask but never fill it). Everything else is `Unsupported`.
//!
//! Every size is checked before allocating: the RGBA size against the cap (`TooLarge`), and
//! the pixel data the header claims against the bytes actually present (`Malformed`). Never
//! panics on a malformed header (GC-10). Errors carry no pixel data (GC-06).

#![cfg_attr(not(windows), allow(dead_code))]

use zeroize::Zeroizing;

/// Largest accepted width or height.
pub(crate) const MAX_DIMENSION: u32 = 32_768;

const BI_RGB: u32 = 0;
const BI_BITFIELDS: u32 = 3;

/// A converted image.
pub(crate) struct DibImage {
    pub png: Vec<u8>,
    pub width: u32,
    pub height: u32,
}

/// Why a DIB was not converted. No content.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum DibError {
    /// The decoded (or encoded) image exceeds the cap; `bytes` is the size that was refused.
    TooLarge { bytes: u64 },
    /// A valid DIB in a layout this adapter does not convert.
    Unsupported(&'static str),
    /// The header is inconsistent with itself or with the data.
    Malformed(&'static str),
}

fn u16_at(b: &[u8], off: usize) -> Option<u16> {
    let s = b.get(off..off.checked_add(2)?)?;
    Some(u16::from_le_bytes([*s.first()?, *s.get(1)?]))
}

fn u32_at(b: &[u8], off: usize) -> Option<u32> {
    let s = b.get(off..off.checked_add(4)?)?;
    let mut a = [0u8; 4];
    a.copy_from_slice(s);
    Some(u32::from_le_bytes(a))
}

fn i32_at(b: &[u8], off: usize) -> Option<i32> {
    u32_at(b, off).map(|v| i32::from_le_bytes(v.to_le_bytes()))
}

const SHORT: DibError = DibError::Malformed("header shorter than declared");

/// One colour channel: mask, shift and width in bits.
#[derive(Clone, Copy)]
struct Channel {
    mask: u32,
    shift: u32,
    bits: u32,
}

impl Channel {
    fn new(mask: u32) -> Self {
        if mask == 0 {
            return Self {
                mask: 0,
                shift: 0,
                bits: 0,
            };
        }
        let shift = mask.trailing_zeros();
        let bits = (mask >> shift).trailing_ones();
        Self { mask, shift, bits }
    }

    /// The channel value scaled to 8 bits.
    fn get(self, px: u32) -> u8 {
        if self.bits == 0 {
            return 0;
        }
        let v = (px & self.mask) >> self.shift;
        let v = u64::from(v);
        let max = (1u64 << self.bits.min(32)) - 1;
        u8::try_from((v.min(max) * 255 + max / 2) / max).unwrap_or(u8::MAX)
    }
}

/// Converts a packed DIB to PNG. `max_bytes` caps both the RGBA size (`width Ã— height Ã— 4`,
/// checked before allocating) and the encoded PNG.
pub(crate) fn dib_to_png(dib: &[u8], max_bytes: u64) -> Result<DibImage, DibError> {
    let header_size = u32_at(dib, 0).ok_or(SHORT)?;
    let header_len = match header_size {
        40 | 52 | 56 | 108 | 124 => header_size as usize,
        12 => return Err(DibError::Unsupported("BITMAPCOREHEADER")),
        _ => return Err(DibError::Malformed("unknown header size")),
    };
    if dib.len() < header_len {
        return Err(SHORT);
    }
    let width = i32_at(dib, 4).ok_or(SHORT)?;
    let height = i32_at(dib, 8).ok_or(SHORT)?;
    let bpp = u16_at(dib, 14).ok_or(SHORT)?;
    let compression = u32_at(dib, 16).ok_or(SHORT)?;
    let clr_used = u32_at(dib, 32).ok_or(SHORT)?;

    let w = u32::try_from(width).map_err(|_| DibError::Malformed("negative width"))?;
    let top_down = height < 0;
    let h = height.unsigned_abs();
    if w == 0 || h == 0 || w > MAX_DIMENSION || h > MAX_DIMENSION {
        return Err(DibError::Malformed("dimension out of range"));
    }
    match (bpp, compression) {
        (24, BI_RGB) | (32, BI_RGB) | (32, BI_BITFIELDS) => {}
        (24 | 32, _) => return Err(DibError::Unsupported("DIB compression")),
        _ => return Err(DibError::Unsupported("DIB bit depth")),
    }

    // Cap before any allocation.
    let rgba_len = u64::from(w) * u64::from(h) * 4;
    if rgba_len > max_bytes {
        return Err(DibError::TooLarge { bytes: rgba_len });
    }

    // Masks: in the header from V2 on; after a 40-byte header for BI_BITFIELDS.
    let (r_mask, g_mask, b_mask, offset) = if compression == BI_BITFIELDS {
        if header_len >= 52 {
            (
                u32_at(dib, 40).ok_or(SHORT)?,
                u32_at(dib, 44).ok_or(SHORT)?,
                u32_at(dib, 48).ok_or(SHORT)?,
                header_len,
            )
        } else {
            (
                u32_at(dib, 40).ok_or(SHORT)?,
                u32_at(dib, 44).ok_or(SHORT)?,
                u32_at(dib, 48).ok_or(SHORT)?,
                header_len + 12,
            )
        }
    } else {
        (0x00FF_0000, 0x0000_FF00, 0x0000_00FF, header_len)
    };
    let a_mask = if header_len >= 56 {
        u32_at(dib, 52).ok_or(SHORT)?
    } else {
        0
    };
    // Optional colour table (an optimisation palette for 24/32-bit DIBs).
    let table = u64::from(clr_used) * 4;
    let mut start = u64::try_from(offset)
        .unwrap_or(u64::MAX)
        .saturating_add(table);

    let stride = (u64::from(w) * u64::from(bpp)).div_ceil(32) * 4;
    let image_len = stride * u64::from(h);
    let len = u64::try_from(dib.len()).unwrap_or(u64::MAX);
    // Some writers append the three masks after a V4/V5 header as well: accept that layout
    // only when the size matches it exactly.
    if compression == BI_BITFIELDS
        && header_len >= 52
        && start.saturating_add(12).saturating_add(image_len) == len
    {
        start += 12;
    }
    if start.saturating_add(image_len) > len {
        return Err(DibError::Malformed(
            "header claims more bytes than the data",
        ));
    }
    let start = usize::try_from(start).map_err(|_| SHORT)?;
    let stride = usize::try_from(stride).map_err(|_| SHORT)?;
    let pixels = dib.get(start..).ok_or(SHORT)?;

    let (r, g, b, a) = (
        Channel::new(r_mask),
        Channel::new(g_mask),
        Channel::new(b_mask),
        Channel::new(a_mask),
    );
    let (wu, hu) = (w as usize, h as usize);
    let rgba_cap = usize::try_from(rgba_len).map_err(|_| DibError::TooLarge { bytes: rgba_len })?;
    let mut rgba = Zeroizing::new(Vec::with_capacity(rgba_cap));
    let mut any_alpha = false;
    for y in 0..hu {
        let src_row = if top_down { y } else { hu - 1 - y };
        let row_start = src_row.checked_mul(stride).ok_or(SHORT)?;
        let row_end = row_start.checked_add(stride).ok_or(SHORT)?;
        let row = pixels.get(row_start..row_end).ok_or(SHORT)?;
        for x in 0..wu {
            if bpp == 24 {
                let p = row.get(x * 3..x * 3 + 3).ok_or(SHORT)?;
                rgba.extend_from_slice(&[
                    *p.get(2).ok_or(SHORT)?,
                    *p.get(1).ok_or(SHORT)?,
                    *p.first().ok_or(SHORT)?,
                    255,
                ]);
            } else {
                let px = u32_at(row, x * 4).ok_or(SHORT)?;
                let alpha = if a.bits > 0 { a.get(px) } else { 255 };
                any_alpha |= a.bits > 0 && alpha != 0;
                rgba.extend_from_slice(&[r.get(px), g.get(px), b.get(px), alpha]);
            }
        }
    }
    if a.bits > 0 && !any_alpha {
        for px in rgba.as_chunks_mut::<4>().0 {
            px[3] = 255;
        }
    }

    let png = encode_png(&rgba, w, h)?;
    let png_len = u64::try_from(png.len()).unwrap_or(u64::MAX);
    if png_len > max_bytes {
        return Err(DibError::TooLarge { bytes: png_len });
    }
    Ok(DibImage {
        png,
        width: w,
        height: h,
    })
}

fn encode_png(rgba: &[u8], w: u32, h: u32) -> Result<Vec<u8>, DibError> {
    let mut out = Vec::new();
    {
        let mut enc = png::Encoder::new(&mut out, w, h);
        enc.set_color(png::ColorType::Rgba);
        enc.set_depth(png::BitDepth::Eight);
        let mut writer = enc
            .write_header()
            .map_err(|_| DibError::Malformed("PNG encoder refused the image"))?;
        writer
            .write_image_data(rgba)
            .map_err(|_| DibError::Malformed("PNG encoder refused the image"))?;
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    const BIG: u64 = 64 * 1024 * 1024;

    /// A packed DIB: header of `header_size` bytes (40 or 124), optional masks (placed in the
    /// header for V5, after it for a 40-byte header), then `pixels`.
    fn dib(
        header_size: u32,
        w: i32,
        h: i32,
        bpp: u16,
        compression: u32,
        masks: Option<[u32; 4]>,
        pixels: &[u8],
    ) -> Vec<u8> {
        let mut d = Vec::new();
        d.extend_from_slice(&header_size.to_le_bytes());
        d.extend_from_slice(&w.to_le_bytes());
        d.extend_from_slice(&h.to_le_bytes());
        d.extend_from_slice(&1u16.to_le_bytes());
        d.extend_from_slice(&bpp.to_le_bytes());
        d.extend_from_slice(&compression.to_le_bytes());
        d.extend_from_slice(&0u32.to_le_bytes()); // biSizeImage
        d.extend_from_slice(&[0u8; 8]); // resolution
        d.extend_from_slice(&0u32.to_le_bytes()); // biClrUsed
        d.extend_from_slice(&0u32.to_le_bytes()); // biClrImportant
        assert_eq!(d.len(), 40);
        if header_size > 40 {
            let m = masks.unwrap_or([0; 4]);
            for v in m {
                d.extend_from_slice(&v.to_le_bytes());
            }
            d.resize(header_size as usize, 0);
        } else if let Some(m) = masks {
            for v in &m[..3] {
                d.extend_from_slice(&v.to_le_bytes());
            }
        }
        d.extend_from_slice(pixels);
        d
    }

    fn decode(png_bytes: &[u8]) -> (u32, u32, Vec<u8>) {
        let mut dec = png::Decoder::new(std::io::Cursor::new(png_bytes));
        dec.set_transformations(png::Transformations::EXPAND);
        let mut reader = dec.read_info().unwrap();
        let mut buf = vec![0; reader.output_buffer_size().unwrap()];
        let info = reader.next_frame(&mut buf).unwrap();
        assert_eq!(info.color_type, png::ColorType::Rgba);
        buf.truncate(info.buffer_size());
        (info.width, info.height, buf)
    }

    #[test]
    fn dib_bottom_up_24bit() {
        // 2x2, rows padded to 8 bytes. Bottom row first: blue, white; top row: red, green.
        let mut px = Vec::new();
        px.extend_from_slice(&[255, 0, 0, 255, 255, 255, 0, 0]); // bottom: blue, white
        px.extend_from_slice(&[0, 0, 255, 0, 255, 0, 0, 0]); // top: red, green
        let img = dib_to_png(&dib(40, 2, 2, 24, BI_RGB, None, &px), BIG).unwrap();
        assert_eq!((img.width, img.height), (2, 2));
        let (w, h, rgba) = decode(&img.png);
        assert_eq!((w, h), (2, 2));
        assert_eq!(
            rgba,
            vec![
                255, 0, 0, 255, 0, 255, 0, 255, // top: red, green
                0, 0, 255, 255, 255, 255, 255, 255, // bottom: blue, white
            ]
        );
    }

    #[test]
    fn dib_top_down_32bit() {
        // 2x1 then a second row: top-down, BGRX, the X byte is ignored (opaque).
        let px = [
            0, 0, 255, 7, 0, 255, 0, 7, // first (top) row: red, green
            255, 0, 0, 7, 10, 20, 30, 7, // second row: blue, (30,20,10)
        ];
        let img = dib_to_png(&dib(40, 2, -2, 32, BI_RGB, None, &px), BIG).unwrap();
        let (w, h, rgba) = decode(&img.png);
        assert_eq!((w, h), (2, 2));
        assert_eq!(
            rgba,
            vec![255, 0, 0, 255, 0, 255, 0, 255, 0, 0, 255, 255, 30, 20, 10, 255]
        );
    }

    #[test]
    fn dibv5_alpha_preserved() {
        let masks = [0x00FF_0000, 0x0000_FF00, 0x0000_00FF, 0xFF00_0000];
        let px = [
            0, 0, 255, 128, // red, half alpha
            0, 255, 0, 0, // green, transparent
        ];
        let img = dib_to_png(&dib(124, 2, -1, 32, BI_BITFIELDS, Some(masks), &px), BIG).unwrap();
        let (_, _, rgba) = decode(&img.png);
        assert_eq!(rgba, vec![255, 0, 0, 128, 0, 255, 0, 0]);
        // Alpha mask set but alpha zero everywhere: treated as opaque.
        let px = [0, 0, 255, 0, 0, 255, 0, 0];
        let img = dib_to_png(&dib(124, 2, -1, 32, BI_BITFIELDS, Some(masks), &px), BIG).unwrap();
        let (_, _, rgba) = decode(&img.png);
        assert_eq!(rgba, vec![255, 0, 0, 255, 0, 255, 0, 255]);
        // V5 BI_RGB with an alpha mask also keeps alpha.
        let px = [0, 0, 255, 64];
        let img = dib_to_png(&dib(124, 1, 1, 32, BI_RGB, Some(masks), &px), BIG).unwrap();
        let (_, _, rgba) = decode(&img.png);
        assert_eq!(rgba, vec![255, 0, 0, 64]);
    }

    #[test]
    fn dib_1x1() {
        let img = dib_to_png(&dib(40, 1, 1, 24, BI_RGB, None, &[1, 2, 3, 0]), BIG).unwrap();
        assert_eq!((img.width, img.height), (1, 1));
        let (w, h, rgba) = decode(&img.png);
        assert_eq!((w, h), (1, 1));
        assert_eq!(rgba, vec![3, 2, 1, 255]);
    }

    #[test]
    fn dib_16k_wide_too_large() {
        // 16 384 Ã— 16 384 Ã— 4 = 1 GiB of RGBA: refused before allocating or reading pixels
        // (no pixel data is even present).
        let d = dib(40, 16_384, 16_384, 32, BI_RGB, None, &[]);
        assert_eq!(
            dib_to_png(&d, BIG).err(),
            Some(DibError::TooLarge {
                bytes: 16_384 * 16_384 * 4
            })
        );
        // Just at the cap is fine; one byte under is not.
        let d = dib(40, 2, 1, 32, BI_RGB, None, &[0; 8]);
        assert!(dib_to_png(&d, 8 + 200).is_ok());
        assert!(matches!(
            dib_to_png(&d, 7),
            Err(DibError::TooLarge { bytes: 8 })
        ));
        // Dimensions above 32 768 are malformed whatever the cap.
        let d = dib(40, 32_769, 1, 32, BI_RGB, None, &[]);
        assert!(matches!(
            dib_to_png(&d, u64::MAX),
            Err(DibError::Malformed(_))
        ));
    }

    #[test]
    fn dib_header_lies_about_size() {
        // Claims 100x100x32 (40 000 bytes) but carries 10.
        let d = dib(40, 100, 100, 32, BI_RGB, None, &[0; 10]);
        assert!(matches!(dib_to_png(&d, BIG), Err(DibError::Malformed(_))));
        // Truncated headers, zero and negative dimensions, i32::MIN height: errors, no panic.
        for cut in 0..40 {
            let d = dib(40, 1, 1, 24, BI_RGB, None, &[0; 4]);
            assert!(dib_to_png(&d[..cut], BIG).is_err(), "cut {cut}");
        }
        let v5 = dib(124, 1, 1, 32, BI_RGB, None, &[0; 4]);
        assert!(dib_to_png(&v5[..100], BIG).is_err());
        for (w, h) in [(0, 1), (1, 0), (-1, 1), (1, i32::MIN), (i32::MIN, 1)] {
            let d = dib(40, w, h, 24, BI_RGB, None, &[0; 4]);
            assert!(
                matches!(dib_to_png(&d, BIG), Err(DibError::Malformed(_))),
                "{w}x{h}"
            );
        }
        // A colour-table count that runs past the data.
        let mut d = dib(40, 1, 1, 24, BI_RGB, None, &[0; 4]);
        d[32..36].copy_from_slice(&u32::MAX.to_le_bytes());
        assert!(matches!(dib_to_png(&d, BIG), Err(DibError::Malformed(_))));
        // Unknown header size.
        let mut d = dib(40, 1, 1, 24, BI_RGB, None, &[0; 4]);
        d[0..4].copy_from_slice(&1000u32.to_le_bytes());
        assert!(dib_to_png(&d, BIG).is_err());
    }

    #[test]
    fn dib_bitfields_32bit() {
        // 40-byte header followed by three masks; pixel layout RGBX (red in the low byte).
        let masks = [0x0000_00FF, 0x0000_FF00, 0x00FF_0000, 0];
        let px = [10, 20, 30, 99, 200, 100, 50, 99];
        let img = dib_to_png(&dib(40, 2, 1, 32, BI_BITFIELDS, Some(masks), &px), BIG).unwrap();
        let (_, _, rgba) = decode(&img.png);
        assert_eq!(rgba, vec![10, 20, 30, 255, 200, 100, 50, 255]);
        // 10-bit channels (2-10-10-10) scale to 8 bits.
        let masks = [0x3FF0_0000, 0x000F_FC00, 0x0000_03FF, 0];
        let px = 0x3FF0_0000u32.to_le_bytes(); // full red
        let img = dib_to_png(&dib(40, 1, 1, 32, BI_BITFIELDS, Some(masks), &px), BIG).unwrap();
        let (_, _, rgba) = decode(&img.png);
        assert_eq!(rgba, vec![255, 0, 0, 255]);
        // V5 header with the masks repeated after it (seen from some writers).
        let masks = [0x00FF_0000, 0x0000_FF00, 0x0000_00FF, 0];
        let mut d = dib(124, 1, 1, 32, BI_BITFIELDS, Some(masks), &[]);
        for v in &masks[..3] {
            d.extend_from_slice(&v.to_le_bytes());
        }
        d.extend_from_slice(&[1, 2, 3, 0]);
        let img = dib_to_png(&d, BIG).unwrap();
        let (_, _, rgba) = decode(&img.png);
        assert_eq!(rgba, vec![3, 2, 1, 255]);
    }

    #[test]
    fn dib_8bit_unsupported() {
        let d = dib(40, 1, 1, 8, BI_RGB, None, &[0; 8]);
        assert_eq!(
            dib_to_png(&d, BIG).err(),
            Some(DibError::Unsupported("DIB bit depth"))
        );
        // 16-bit, and 24-bit with BI_BITFIELDS, and RLE / BI_PNG compressions.
        let d = dib(40, 1, 1, 16, BI_RGB, None, &[0; 4]);
        assert!(matches!(dib_to_png(&d, BIG), Err(DibError::Unsupported(_))));
        for c in [1, 2, 4, 5] {
            let d = dib(40, 1, 1, 32, c, None, &[0; 4]);
            assert!(
                matches!(dib_to_png(&d, BIG), Err(DibError::Unsupported(_))),
                "{c}"
            );
        }
        let d = dib(40, 1, 1, 24, BI_BITFIELDS, Some([1, 2, 4, 0]), &[0; 4]);
        assert!(matches!(dib_to_png(&d, BIG), Err(DibError::Unsupported(_))));
    }
}
