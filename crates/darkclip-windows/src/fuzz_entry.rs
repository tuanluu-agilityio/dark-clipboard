//! Fuzz entry points (Task 406). Built only for tests and with the `fuzzing` feature (the
//! `fuzz/` cargo-fuzz targets enable it); never part of a normal build.
//!
//! Each function takes arbitrary bytes, drives one converter or parser of this adapter and
//! checks its invariants with `assert!`; the messages never include the input (GC-06). The
//! `#[ignore]` stable harness in `harness` calls the same functions. No OS calls here: every
//! function under test is pure.

use crate::dib::{dib_to_png, png_to_dibs, DibError};
use crate::text::{
    bytes_capped, parse_dropfiles, uri_list, utf16_capped, utf8_capped, MAX_DROP_FILES,
};
use darkclip_core::transform::Transform;

/// Encoded-PNG caps the DIB target picks from (selector byte).
const PNG_CAPS: [u64; 4] = [0, 64, 4096, 256 * 1024 * 1024];
/// UTF-16 unit budgets the DROPFILES target picks from.
const UNIT_CAPS: [usize; 6] = [0, 1, 16, 260, 4096, 1 << 20];
/// Output caps the text targets pick from.
const TEXT_CAPS: [usize; 8] = [0, 1, 2, 3, 4, 16, 1024, 1 << 20];

fn split(data: &[u8]) -> (u8, &[u8]) {
    match data.split_first() {
        Some((&sel, rest)) => (sel, rest),
        None => (0, &[]),
    }
}

/// `items[sel % N]`; the tables above are non-empty constants.
fn pick<T: Copy, const N: usize>(items: &[T; N], sel: u8) -> T {
    items[usize::from(sel) % N]
}

/// Width and height of a PNG, read back with the `png` crate.
fn png_size(png_bytes: &[u8]) -> Option<(u32, u32)> {
    let d = png::Decoder::new(std::io::Cursor::new(png_bytes));
    let r = d.read_info().ok()?;
    let info = r.info();
    Some((info.width, info.height))
}

/// Packed DIB → PNG. The first byte selects the encoded-PNG cap.
pub fn dib(data: &[u8]) {
    let (sel, dib) = split(data);
    let cap = pick(&PNG_CAPS, sel);
    match dib_to_png(dib, cap) {
        Ok(img) => {
            assert!(img.png.len() as u64 <= cap, "PNG over cap");
            assert!(img.width > 0 && img.height > 0, "zero dimension");
            assert_eq!(
                png_size(&img.png),
                Some((img.width, img.height)),
                "PNG readable"
            );
        }
        Err(DibError::TooLarge { .. } | DibError::Unsupported(_) | DibError::Malformed(_)) => {}
    }
}

/// PNG → `CF_DIBV5` + `CF_DIB` (the copy-back path; the stored PNG is untrusted). Both
/// results must convert back to a PNG of the same size.
pub fn png_dibs(data: &[u8]) {
    let Ok(pair) = png_to_dibs(data) else {
        return;
    };
    let size = png_size(data);
    assert!(size.is_some(), "accepted PNG is readable");
    for block in [&pair.v5, &pair.dib] {
        let back = dib_to_png(block, u64::MAX);
        assert!(back.is_ok(), "DIB written for a PNG converts back");
        if let Ok(img) = back {
            assert_eq!(Some((img.width, img.height)), size, "round trip size");
        }
    }
}

/// `CF_HDROP` (`DROPFILES`) parser, then the `text/uri-list` built from its names, then (on
/// Windows) that list back to a `DROPFILES` block. First byte: bit 0 `complete`, the rest
/// the unit budget.
pub fn dropfiles(data: &[u8]) {
    let (sel, block) = split(data);
    let complete = sel & 1 != 0;
    let max_units = pick(&UNIT_CAPS, sel >> 1);
    let Some(files) = parse_dropfiles(block, complete, max_units) else {
        return;
    };
    assert!(files.names.len() <= MAX_DROP_FILES, "too many names");
    let total: usize = files.names.iter().map(|n| n.len()).sum();
    assert!(total <= max_units, "unit budget");
    assert!(
        files.names.iter().all(|n| !n.is_empty() && !n.contains(&0)),
        "empty name or NUL"
    );
    let cap = pick(&TEXT_CAPS, sel >> 4);
    let (list, _) = uri_list(&files.names, cap);
    assert!(list.len() <= cap, "uri-list over cap");
    #[cfg(windows)]
    uri_list_block(list.as_bytes());
}

/// A stored `text/uri-list` (untrusted: it may come from an import) as a `DROPFILES` block
/// for the paste path. Windows only (the writer lives with the Win32 code); elsewhere it
/// does nothing.
pub fn uri_list_block(data: &[u8]) {
    #[cfg(windows)]
    if let Some(block) = crate::access::uri_list_to_dropfiles(data) {
        let parsed = parse_dropfiles(&block, true, usize::MAX);
        assert!(parsed.is_some(), "written DROPFILES parses");
        if let Some(files) = parsed {
            assert!(!files.more, "nothing left out");
            assert!(!files.names.is_empty(), "at least one name");
        }
    }
    #[cfg(not(windows))]
    let _ = data;
}

/// The `HTML Format` (CF_HTML) read path, and the other capped text reads. First byte: the
/// cap. Windows stores the CF_HTML payload whole, header included (there is no separate
/// header parser); what reads it is the capped UTF-8 copy, then the preview and the
/// HTML-to-text transform.
pub fn cf_html(data: &[u8]) {
    let (sel, raw) = split(data);
    let cap = pick(&TEXT_CAPS, sel);
    let until_nul = raw.split(|&b| b == 0).next().unwrap_or_default();
    let lossy = String::from_utf8_lossy(until_nul);

    let (html, cut) = utf8_capped(raw, cap);
    assert!(html.len() <= cap, "utf8 over cap");
    let html = std::str::from_utf8(&html).ok();
    assert!(html.is_some(), "utf8 output is UTF-8");
    let html = html.unwrap_or_default();
    if cut {
        assert!(
            lossy.starts_with(html) && lossy.len() > html.len(),
            "cut is a prefix"
        );
    } else {
        assert_eq!(html, lossy, "uncut copy is the whole text");
    }

    let (text, cut16) = utf16_capped(raw, cap);
    assert!(text.len() <= cap, "utf16 over cap");
    if !cut16 {
        assert!(!text.contains('\0'), "stops at NUL");
    }

    let (rtf, cut_b) = bytes_capped(raw, cap);
    assert!(
        rtf.len() <= cap && until_nul.starts_with(&rtf),
        "bytes prefix"
    );
    assert_eq!(cut_b, until_nul.len() > cap, "bytes cut flag");

    let preview = darkclip_core::make_preview(html);
    assert!(!preview.contains('\n'), "preview newline");
    let _ = Transform::HtmlToText.apply(html);
}

#[cfg(test)]
mod harness;
