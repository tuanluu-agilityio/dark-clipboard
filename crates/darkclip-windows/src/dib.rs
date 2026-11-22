//! Packed DIB (`CF_DIB` / `CF_DIBV5`) to PNG (Task 103). Pure, no OS calls, compiled on every
//! OS so its tests run everywhere.
//!
//! Supported: `BITMAPINFOHEADER` (40), V2/V3 (52/56), V4 (108) and V5 (124) headers; 24-bit
//! `BI_RGB`, 32-bit `BI_RGB` and `BI_BITFIELDS`; bottom-up and top-down (negative height).
//! Alpha comes from the header's alpha mask (V3 and later) when present, else the image is
//! opaque; an alpha channel that is zero everywhere is treated as absent (writers that set the
//! mask but never fill it). Everything else is `Unsupported`.
//!
//! Every size is checked before allocating: the decoded size against the shared decode caps
//! (`darkclip_core::limits`: 64 MP, 256 MiB of RGBA, `TooLarge`), and the pixel data the
//! header claims against the bytes actually present (`Malformed`). The stored-image cap
//! applies to the encoded PNG only (D-043). Never panics on a malformed header (GC-10).
//! Errors carry no pixel data (GC-06).

#![cfg_attr(not(windows), allow(dead_code))]

use darkclip_core::limits::{within_decode_caps, MAX_RGBA_BYTES, MAX_SIDE};
use std::io::Write;
use zeroize::Zeroizing;

/// Largest accepted width or height.
pub(crate) const MAX_DIMENSION: u32 = MAX_SIDE;

/// Largest data block a DIB within the decode caps can need: the pixels (a 32-bit image at
/// the RGBA cap; a 24-bit one with row padding is never larger), plus the biggest header,
/// repeated masks, a colour table and the slack `GlobalSize` may add. A larger block is
/// refused from its size before anything is copied.
pub(crate) const MAX_DIB_BLOCK: u64 = MAX_RGBA_BYTES + 64 * 1024;

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

/// Converts a packed DIB to PNG. The decoded size is bounded by the shared decode caps,
/// checked from the header before allocating; `max_bytes` caps the encoded PNG (the stored
/// image, D-043), whose buffer never grows past it.
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

    // Decode caps before any allocation.
    let rgba_len = u64::from(w) * u64::from(h) * 4;
    if !within_decode_caps(w, h) {
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
    // Some writers (and GDI synthesis) append the three masks after a V2+ header as well.
    // `GlobalSize` may exceed the DIB, so the block size cannot tell; the layout is taken only
    // when the 12 bytes after the header repeat the header's masks, the image still fits after
    // them, and a non-zero `biSizeImage` does not contradict it.
    let size_image = u64::from(u32_at(dib, 20).ok_or(SHORT)?);
    if compression == BI_BITFIELDS && header_len >= 52 {
        let after = usize::try_from(start)
            .ok()
            .and_then(|s| Some((s, s.checked_add(12)?)))
            .and_then(|(s, e)| dib.get(s..e));
        let mut repeated = [0u8; 12];
        for (i, m) in [r_mask, g_mask, b_mask].into_iter().enumerate() {
            if let Some(slot) = repeated.get_mut(i * 4..i * 4 + 4) {
                slot.copy_from_slice(&m.to_le_bytes());
            }
        }
        let image_room = if size_image != 0 {
            size_image.max(image_len)
        } else {
            image_len
        };
        if after == Some(&repeated[..])
            && start.saturating_add(12).saturating_add(image_room) <= len
        {
            start += 12;
        }
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

    // Wiped on drop, so an over-cap encoding leaves no copy behind.
    let mut png = encode_png(&rgba, w, h, max_bytes)?;
    Ok(DibImage {
        png: std::mem::take(&mut *png),
        width: w,
        height: h,
    })
}

/// A writer into a buffer reserved up front that never grows past its capacity: a write that
/// would not fit fails and sets `over`, so the buffer never reallocates (no copy of the image
/// left in freed memory) and never exceeds the cap.
struct Bounded<'a> {
    out: &'a mut Vec<u8>,
    /// At most the buffer's capacity.
    limit: usize,
    over: bool,
}

impl Write for Bounded<'_> {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        let room = self.limit.min(self.out.capacity());
        if buf.len() > room.saturating_sub(self.out.len()) {
            self.over = true;
            return Err(std::io::Error::other("image cap"));
        }
        self.out.extend_from_slice(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

/// Encodes RGBA as PNG into a buffer wiped on drop, reserved once for the smaller of the
/// worst case (stored deflate blocks) and `max_bytes`, and never grown, so it does not
/// reallocate and leave copies in freed memory. A PNG that would exceed `max_bytes` is
/// `TooLarge` with `bytes` = the binding limit + 1 (at least that many were needed). The encoder's
/// own internal buffers are outside this crate's control.
fn encode_png(rgba: &[u8], w: u32, h: u32, max_bytes: u64) -> Result<Zeroizing<Vec<u8>>, DibError> {
    let rows = usize::try_from(h).unwrap_or(usize::MAX);
    let raw = rgba.len().saturating_add(rows); // one filter byte per row
    let worst = raw
        .saturating_add(raw / 16_000 * 5)
        .saturating_add(raw / 1_000)
        .saturating_add(1024);
    let limit = worst.min(usize::try_from(max_bytes).unwrap_or(usize::MAX));
    let mut out = Zeroizing::new(Vec::with_capacity(limit));
    let mut sink = Bounded {
        out: &mut out,
        limit,
        over: false,
    };
    let result = encode_into(&mut sink, rgba, w, h);
    let over = sink.over;
    match result {
        Ok(()) => Ok(out),
        // The binding limit + 1 is reported: a lower bound of the real size. It is `max_bytes + 1`
        // unless the worst-case estimate was smaller (a valid encoding always fits that).
        Err(_) if over => Err(DibError::TooLarge {
            bytes: u64::try_from(limit).unwrap_or(u64::MAX).saturating_add(1),
        }),
        Err(_) => Err(DibError::Malformed("PNG encoder refused the image")),
    }
}

fn encode_into(
    sink: &mut Bounded<'_>,
    rgba: &[u8],
    w: u32,
    h: u32,
) -> Result<(), png::EncodingError> {
    let mut enc = png::Encoder::new(sink, w, h);
    enc.set_color(png::ColorType::Rgba);
    enc.set_depth(png::BitDepth::Eight);
    let mut writer = enc.write_header()?;
    writer.write_image_data(rgba)?;
    writer.finish()
}

/// `CF_DIBV5` and `CF_DIB` renderings of one PNG (Task 104 copy-back), wiped on drop.
pub(crate) struct DibPair {
    /// Packed DIB: `BITMAPV5HEADER`, 32-bit `BI_BITFIELDS` BGRA with an alpha mask.
    pub v5: Zeroizing<Vec<u8>>,
    /// Packed DIB: `BITMAPINFOHEADER`, 32-bit `BI_RGB` BGRA (alpha in the reserved byte).
    pub dib: Zeroizing<Vec<u8>>,
}

/// Working memory the PNG decoder may use besides the output (D-035).
const DECODER_WORKING_BYTES: usize = 8 * 1024 * 1024;
const BITMAPV5HEADER_LEN: u32 = 124;
const BITMAPINFOHEADER_LEN: u32 = 40;
/// `LCS_sRGB` ('sRGB').
const LCS_SRGB: u32 = 0x7352_4742;
/// `LCS_GM_IMAGES`.
const LCS_GM_IMAGES: u32 = 4;

/// Decodes a PNG and renders it as `CF_DIBV5` + `CF_DIB` (copy-back). The header is checked
/// against [`MAX_DIMENSION`] and the shared decode caps (64 MP, 256 MiB of RGBA) before any
/// pixel buffer exists; over them it is `TooLarge` (the caller then writes the PNG alone,
/// D-043): the stored PNG is untrusted input, a small file can claim a huge image.
/// Metadata chunks (iCCP, text) are ignored. Every pixel buffer is wiped on drop. Never panics
/// on a malformed PNG (GC-10); errors carry no pixel data (GC-06).
pub(crate) fn png_to_dibs(png_bytes: &[u8]) -> Result<DibPair, DibError> {
    const CORRUPT: DibError = DibError::Malformed("not a decodable PNG");
    let mut decoder = png::Decoder::new_with_limits(
        std::io::Cursor::new(png_bytes),
        png::Limits {
            bytes: DECODER_WORKING_BYTES,
        },
    );
    decoder.set_ignore_iccp_chunk(true);
    decoder.set_ignore_text_chunk(true);
    decoder.set_transformations(png::Transformations::normalize_to_color8());
    let info = decoder.read_header_info().map_err(|_| CORRUPT)?;
    let (w, h) = (info.width, info.height);
    if w == 0 || h == 0 || w > MAX_DIMENSION || h > MAX_DIMENSION {
        return Err(DibError::Malformed("dimension out of range"));
    }
    let pixel_bytes = u64::from(w) * u64::from(h) * 4;
    // Decode caps before any allocation (and before `read_info`, which sizes its buffers).
    if !within_decode_caps(w, h) {
        return Err(DibError::TooLarge { bytes: pixel_bytes });
    }
    // The DIB header stores the image size as a u32.
    let size_image =
        u32::try_from(pixel_bytes).map_err(|_| DibError::TooLarge { bytes: pixel_bytes })?;
    let px_len =
        usize::try_from(pixel_bytes).map_err(|_| DibError::TooLarge { bytes: pixel_bytes })?;
    let mut reader = decoder.read_info().map_err(|_| CORRUPT)?;
    let (color, depth) = reader.output_color_type();
    if depth != png::BitDepth::Eight {
        return Err(CORRUPT);
    }
    let channels: usize = match color {
        png::ColorType::Grayscale => 1,
        png::ColorType::GrayscaleAlpha => 2,
        png::ColorType::Rgb => 3,
        png::ColorType::Rgba => 4,
        png::ColorType::Indexed => return Err(CORRUPT),
    };
    // At most 4 bytes per pixel: bounded by `pixel_bytes`.
    let size = reader.output_buffer_size().ok_or(CORRUPT)?;
    let mut buf = Zeroizing::new(vec![0u8; size]);
    let frame = reader.next_frame(&mut buf).map_err(|_| CORRUPT)?;
    let line = frame.line_size;
    let (wu, hu) = (w as usize, h as usize);
    if line < wu.saturating_mul(channels) {
        return Err(CORRUPT);
    }

    let mut v5 = Zeroizing::new(Vec::with_capacity(
        (BITMAPV5HEADER_LEN as usize).saturating_add(px_len),
    ));
    let mut dib = Zeroizing::new(Vec::with_capacity(
        (BITMAPINFOHEADER_LEN as usize).saturating_add(px_len),
    ));
    push_header(&mut v5, BITMAPV5HEADER_LEN, w, h, BI_BITFIELDS, size_image);
    for v in [
        0x00FF_0000u32,
        0x0000_FF00,
        0x0000_00FF,
        0xFF00_0000,
        LCS_SRGB,
    ] {
        v5.extend_from_slice(&v.to_le_bytes());
    }
    v5.extend_from_slice(&[0u8; 36 + 12]); // bV5Endpoints, bV5Gamma{Red,Green,Blue}
    v5.extend_from_slice(&LCS_GM_IMAGES.to_le_bytes());
    v5.extend_from_slice(&[0u8; 12]); // bV5ProfileData, bV5ProfileSize, bV5Reserved
    push_header(&mut dib, BITMAPINFOHEADER_LEN, w, h, BI_RGB, size_image);

    // Bottom-up rows, BGRA (straight alpha). 32-bit rows need no padding.
    for y in (0..hu).rev() {
        let start = y.checked_mul(line).ok_or(CORRUPT)?;
        let row = buf
            .get(start..start.saturating_add(wu * channels))
            .ok_or(CORRUPT)?;
        for p in row.chunks_exact(channels) {
            let px = match *p {
                [g] => [g, g, g, 255],
                [g, a] => [g, g, g, a],
                [r, g, b] => [b, g, r, 255],
                [r, g, b, a] => [b, g, r, a],
                _ => return Err(CORRUPT),
            };
            v5.extend_from_slice(&px);
            dib.extend_from_slice(&px);
        }
    }
    Ok(DibPair { v5, dib })
}

/// The 40 bytes shared by `BITMAPINFOHEADER` and `BITMAPV5HEADER`: size, width, height
/// (positive: bottom-up), 1 plane, 32 bits, compression, image size, no resolution, no
/// colour table.
fn push_header(out: &mut Vec<u8>, header: u32, w: u32, h: u32, compression: u32, size: u32) {
    out.extend_from_slice(&header.to_le_bytes());
    out.extend_from_slice(&w.to_le_bytes());
    out.extend_from_slice(&h.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&32u16.to_le_bytes());
    out.extend_from_slice(&compression.to_le_bytes());
    out.extend_from_slice(&size.to_le_bytes());
    out.extend_from_slice(&[0u8; 16]); // X/Y pixels per metre, colours used, important
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
    fn dib_top_down_24bit() {
        // 1x2 top-down: first row red, second row blue; rows padded to 4 bytes.
        let px = [0, 0, 255, 0, 255, 0, 0, 0];
        let img = dib_to_png(&dib(40, 1, -2, 24, BI_RGB, None, &px), BIG).unwrap();
        let (w, h, rgba) = decode(&img.png);
        assert_eq!((w, h), (1, 2));
        assert_eq!(rgba, vec![255, 0, 0, 255, 0, 0, 255, 255]);
    }

    #[test]
    fn dib_24bit_odd_width() {
        // 3x2 bottom-up: 9 pixel bytes + 3 padding bytes per row.
        let mut px = Vec::new();
        px.extend_from_slice(&[1, 2, 3, 4, 5, 6, 7, 8, 9, 0xEE, 0xEE, 0xEE]); // bottom
        px.extend_from_slice(&[10, 20, 30, 40, 50, 60, 70, 80, 90, 0xEE, 0xEE, 0xEE]); // top
        let img = dib_to_png(&dib(40, 3, 2, 24, BI_RGB, None, &px), BIG).unwrap();
        let (w, h, rgba) = decode(&img.png);
        assert_eq!((w, h), (3, 2));
        assert_eq!(
            rgba,
            vec![
                30, 20, 10, 255, 60, 50, 40, 255, 90, 80, 70, 255, // top
                3, 2, 1, 255, 6, 5, 4, 255, 9, 8, 7, 255, // bottom
            ]
        );
    }

    /// `d` with `biClrUsed = n` and an `n`-entry colour table inserted after the header
    /// (and after the masks of a 40-byte BI_BITFIELDS header).
    fn with_table(mut d: Vec<u8>, at: usize, n: u32) -> Vec<u8> {
        d[32..36].copy_from_slice(&n.to_le_bytes());
        let table: Vec<u8> = (0..n * 4).map(|i| 0xA0 | (i as u8 & 0x0F)).collect();
        d.splice(at..at, table);
        d
    }

    #[test]
    fn dib_nonzero_clr_used_table_skipped() {
        // 24-bit with a 2-entry optimisation palette.
        let d = with_table(dib(40, 1, 1, 24, BI_RGB, None, &[1, 2, 3, 0]), 40, 2);
        let (_, _, rgba) = decode(&dib_to_png(&d, BIG).unwrap().png);
        assert_eq!(rgba, vec![3, 2, 1, 255]);
        // 32-bit BI_RGB with a 3-entry palette.
        let d = with_table(dib(40, 1, 1, 32, BI_RGB, None, &[4, 5, 6, 0]), 40, 3);
        let (_, _, rgba) = decode(&dib_to_png(&d, BIG).unwrap().png);
        assert_eq!(rgba, vec![6, 5, 4, 255]);
        // 32-bit BI_BITFIELDS (40-byte header): masks, then the palette, then the pixels.
        let masks = [0x00FF_0000, 0x0000_FF00, 0x0000_00FF, 0];
        let d = with_table(
            dib(40, 1, 1, 32, BI_BITFIELDS, Some(masks), &[7, 8, 9, 0]),
            52,
            1,
        );
        let (_, _, rgba) = decode(&dib_to_png(&d, BIG).unwrap().png);
        assert_eq!(rgba, vec![9, 8, 7, 255]);
    }

    #[test]
    fn dibv5_repeated_masks_with_padded_block() {
        // GlobalSize may exceed the DIB: masks repeated after the V5 header, then the pixels,
        // then padding. The masks must not be read as pixels.
        let masks = [0x00FF_0000, 0x0000_FF00, 0x0000_00FF, 0];
        let mut d = dib(124, 2, -1, 32, BI_BITFIELDS, Some(masks), &[]);
        for v in &masks[..3] {
            d.extend_from_slice(&v.to_le_bytes());
        }
        d.extend_from_slice(&[1, 2, 3, 0, 4, 5, 6, 0]);
        d.extend_from_slice(&[0u8; 16]);
        let (_, _, rgba) = decode(&dib_to_png(&d, BIG).unwrap().png);
        assert_eq!(rgba, vec![3, 2, 1, 255, 6, 5, 4, 255]);
        // Same with biSizeImage set.
        d[20..24].copy_from_slice(&8u32.to_le_bytes());
        let (_, _, rgba) = decode(&dib_to_png(&d, BIG).unwrap().png);
        assert_eq!(rgba, vec![3, 2, 1, 255, 6, 5, 4, 255]);
        // No repeated masks, padded block: pixels start right after the header.
        let mut d = dib(
            124,
            2,
            -1,
            32,
            BI_BITFIELDS,
            Some(masks),
            &[1, 2, 3, 0, 4, 5, 6, 0],
        );
        d.extend_from_slice(&[0u8; 16]);
        let (_, _, rgba) = decode(&dib_to_png(&d, BIG).unwrap().png);
        assert_eq!(rgba, vec![3, 2, 1, 255, 6, 5, 4, 255]);
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
        // 16 384 × 16 384 × 4 = 1 GiB of RGBA: refused before allocating or reading pixels
        // (no pixel data is even present).
        let d = dib(40, 16_384, 16_384, 32, BI_RGB, None, &[]);
        assert_eq!(
            dib_to_png(&d, BIG).err(),
            Some(DibError::TooLarge {
                bytes: 16_384 * 16_384 * 4
            })
        );
        // The cap applies to the encoded PNG (D-043): a cap that holds it is fine; a cap below
        // it is refused with `cap + 1` (here 8), the least that would have been needed.
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

    /// A PNG of `w` x `h` RGBA pixels.
    fn png_rgba(w: u32, h: u32, rgba: &[u8]) -> Vec<u8> {
        let mut out = Vec::new();
        {
            let mut enc = png::Encoder::new(&mut out, w, h);
            enc.set_color(png::ColorType::Rgba);
            enc.set_depth(png::BitDepth::Eight);
            let mut wr = enc.write_header().unwrap();
            wr.write_image_data(rgba).unwrap();
        }
        out
    }

    #[test]
    fn png_to_dibv5_roundtrip() {
        // 3x2, distinct pixels, varied alpha (straight, not premultiplied).
        let rgba: Vec<u8> = vec![
            255, 0, 0, 255, 0, 255, 0, 128, 0, 0, 255, 0, //
            10, 20, 30, 40, 50, 60, 70, 255, 200, 100, 50, 1,
        ];
        let pair = png_to_dibs(&png_rgba(3, 2, &rgba)).unwrap();

        // CF_DIBV5: 124-byte header, 32-bit BI_BITFIELDS with an alpha mask, sRGB.
        let v5 = &pair.v5[..];
        assert_eq!(u32_at(v5, 0), Some(124));
        assert_eq!(i32_at(v5, 4), Some(3));
        assert_eq!(i32_at(v5, 8), Some(2), "bottom-up");
        assert_eq!(u16_at(v5, 12), Some(1));
        assert_eq!(u16_at(v5, 14), Some(32));
        assert_eq!(u32_at(v5, 16), Some(BI_BITFIELDS));
        assert_eq!(u32_at(v5, 20), Some(3 * 2 * 4));
        assert_eq!(u32_at(v5, 52), Some(0xFF00_0000));
        assert_eq!(u32_at(v5, 56), Some(0x7352_4742), "LCS_sRGB");
        assert_eq!(v5.len(), 124 + 3 * 2 * 4);
        let img = dib_to_png(v5, BIG).unwrap();
        assert_eq!((img.width, img.height), (3, 2));
        let (_, _, back) = decode(&img.png);
        assert_eq!(back, rgba);

        // CF_DIB: 40-byte header, 32-bit BI_RGB; read back as opaque with equal colours.
        let d = &pair.dib[..];
        assert_eq!(u32_at(d, 0), Some(40));
        assert_eq!(u16_at(d, 14), Some(32));
        assert_eq!(u32_at(d, 16), Some(BI_RGB));
        assert_eq!(d.len(), 40 + 3 * 2 * 4);
        let img = dib_to_png(d, BIG).unwrap();
        let (_, _, back) = decode(&img.png);
        let opaque: Vec<u8> = rgba
            .chunks(4)
            .flat_map(|p| [p[0], p[1], p[2], 255])
            .collect();
        assert_eq!(back, opaque);
    }

    #[test]
    fn png_to_dibs_converts_rgb_and_gray16() {
        // RGB without alpha: opaque.
        let mut out = Vec::new();
        {
            let mut enc = png::Encoder::new(&mut out, 1, 1);
            enc.set_color(png::ColorType::Rgb);
            enc.set_depth(png::BitDepth::Eight);
            enc.write_header()
                .unwrap()
                .write_image_data(&[1, 2, 3])
                .unwrap();
        }
        let pair = png_to_dibs(&out).unwrap();
        let (_, _, back) = decode(&dib_to_png(&pair.v5, BIG).unwrap().png);
        assert_eq!(back, vec![1, 2, 3, 255]);
        // 16-bit grayscale with alpha: stripped to 8 bits.
        let mut out = Vec::new();
        {
            let mut enc = png::Encoder::new(&mut out, 1, 1);
            enc.set_color(png::ColorType::GrayscaleAlpha);
            enc.set_depth(png::BitDepth::Sixteen);
            enc.write_header()
                .unwrap()
                .write_image_data(&[0x80, 0, 0x40, 0])
                .unwrap();
        }
        let pair = png_to_dibs(&out).unwrap();
        let (_, _, back) = decode(&dib_to_png(&pair.v5, BIG).unwrap().png);
        assert_eq!(back, vec![0x80, 0x80, 0x80, 0x40]);
    }

    #[test]
    fn png_to_dibs_rejects_garbage() {
        assert!(png_to_dibs(b"not a png").is_err());
        assert!(png_to_dibs(&[]).is_err());
        let mut cut = png_rgba(2, 2, &[7; 16]);
        cut.truncate(cut.len() - 20);
        assert!(png_to_dibs(&cut).is_err());
    }
}
