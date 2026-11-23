//! Stable fuzz harness for the Windows adapter's pure converters (Task 406). `#[ignore]`: run
//! on demand, in release mode, as documented in `docs/testing/fuzzing.md`. Every input is
//! synthetic. No OS calls; this file must not log (GC-06): the runner reports.

use super::*;
use darkclip_fuzzgen::png::{gen_png, PNG_DICT};
use darkclip_fuzzgen::{fix_png_crcs, Fuzz, Rng, TrackingAlloc};
use std::time::Duration;

#[global_allocator]
static ALLOC: TrackingAlloc = TrackingAlloc;

const MIB: usize = 1024 * 1024;
/// The shared decode cap (256 MiB of RGBA).
const RGBA_CAP: usize = darkclip_core::limits::MAX_RGBA_BYTES as usize;

fn le32(out: &mut Vec<u8>, v: u32) {
    out.extend_from_slice(&v.to_le_bytes());
}

fn le16(out: &mut Vec<u8>, v: u16) {
    out.extend_from_slice(&v.to_le_bytes());
}

/// A packed DIB, mostly well formed: header of a known size, masks where they belong,
/// rows of the right length (sometimes short or long).
pub(super) fn gen_dib(rng: &mut Rng) -> Vec<u8> {
    let mut out = vec![rng.next_u64() as u8];
    let header: u32 = *rng
        .pick(&[40, 40, 124, 124, 108, 52, 56, 12, 64])
        .unwrap_or(&40);
    let w = rng.range(1, 40) as i32;
    let h = rng.range(1, 40) as i32;
    let w = if rng.one_in(32) { -w } else { w };
    let h = if rng.one_in(3) { -h } else { h };
    let bpp: u16 = *rng.pick(&[24, 32, 32, 24, 32, 8, 16, 1]).unwrap_or(&32);
    let comp: u32 = *rng.pick(&[0, 3, 0, 3, 0, 1, 4]).unwrap_or(&0);
    let clr_used = if rng.one_in(8) {
        rng.range(1, 16) as u32
    } else {
        0
    };
    let start = out.len();
    le32(&mut out, header);
    le32(&mut out, w as u32);
    le32(&mut out, h as u32);
    le16(&mut out, 1);
    le16(&mut out, bpp);
    le32(&mut out, comp);
    le32(&mut out, 0); // size image
    le32(&mut out, 2835);
    le32(&mut out, 2835);
    le32(&mut out, clr_used);
    le32(&mut out, 0);
    let masks = [
        *rng.pick(&[0x00FF_0000, 0x7C00, 0xF800, 0xFFFF_FFFF, 0])
            .unwrap_or(&0),
        *rng.pick(&[0x0000_FF00, 0x03E0, 0x07E0, 0]).unwrap_or(&0),
        *rng.pick(&[0x0000_00FF, 0x001F, 0x8000_0001]).unwrap_or(&0),
        *rng.pick(&[0xFF00_0000, 0, 0x0F00_0000]).unwrap_or(&0),
    ];
    if header >= 52 {
        for m in masks {
            le32(&mut out, m);
        }
    }
    while out.len() - start < header as usize {
        out.push(0);
    }
    out.truncate(start + header as usize);
    if comp == 3 && header == 40 {
        for m in &masks[..3] {
            le32(&mut out, *m);
        }
    }
    for _ in 0..clr_used {
        le32(&mut out, rng.next_u32());
    }
    let row = (w.unsigned_abs() as usize * usize::from(bpp)).div_ceil(32) * 4;
    let mut n = row * h.unsigned_abs() as usize;
    if rng.one_in(6) {
        n = rng.below(n + 64);
    }
    out.extend(rng.bytes(n));
    out
}

/// A `DROPFILES` block: header (mostly `pFiles = 20`), then names, mostly terminated.
fn gen_dropfiles(rng: &mut Rng) -> Vec<u8> {
    let mut out = vec![rng.next_u64() as u8];
    let wide = !rng.one_in(5);
    let p_files: u32 = if rng.one_in(8) {
        *rng.pick(&[0, 19, 21, 24, 4096, 0xFFFF_FFFF]).unwrap_or(&20)
    } else {
        20
    };
    le32(&mut out, p_files);
    out.extend(rng.bytes(12));
    le32(&mut out, u32::from(wide));
    if p_files > 20 && p_files < 64 {
        out.extend(rng.bytes(p_files as usize - 20));
    }
    let parts: [&str; 12] = [
        "C:\\",
        "D:\\data\\",
        "\\\\server\\share\\",
        "\\\\?\\C:\\",
        "\\\\?\\UNC\\srv\\s\\",
        "file name.txt",
        "Tiếng Việt",
        "a%20b#c?d",
        "..\\..\\",
        "😀",
        " ",
        "x",
    ];
    for _ in 0..rng.below(6) {
        let mut units: Vec<u16> = Vec::new();
        for _ in 0..rng.range(1, 4) {
            units.extend(parts[rng.below(parts.len())].encode_utf16());
        }
        if rng.one_in(10) {
            units.push(*rng.pick(&[0xD800u16, 0xDC00, 0xFFFF]).unwrap_or(&0xD800));
        }
        for u in units {
            if wide {
                out.extend_from_slice(&u.to_le_bytes());
            } else {
                out.push(u as u8);
            }
        }
        if wide {
            out.extend_from_slice(&[0, 0]);
        } else {
            out.push(0);
        }
    }
    if !rng.one_in(6) {
        if wide {
            out.extend_from_slice(&[0, 0]);
        } else {
            out.push(0);
        }
    }
    if rng.one_in(8) {
        out.push(rng.next_u64() as u8);
    }
    out
}

/// A `text/uri-list` of `file:` URIs (and some that are not).
fn gen_uri_list(rng: &mut Rng) -> Vec<u8> {
    let lines: [&str; 14] = [
        "file:///C:/a%20b.txt",
        "file:///D:/%C3%B1",
        "file://srv/share/x",
        "file://localhost/C:/x",
        "FILE:///Z:",
        "# comment",
        "http://example.com/x",
        "file:///%ZZ",
        "file:///C:/%00",
        "file://h%2Fx/s",
        "file:///relative",
        "file:///C:/x?q#f",
        "file:",
        "",
    ];
    let mut out = String::new();
    for i in 0..rng.range(1, 6) {
        if i > 0 {
            out.push_str(if rng.one_in(2) { "\r\n" } else { "\n" });
        }
        out.push_str(lines[rng.below(lines.len())]);
        if rng.one_in(3) {
            let tails = ["%41", "%e1%ba%bf", "/sub", " ", "%", "%F0%9F"];
            out.push_str(tails[rng.below(tails.len())]);
        }
    }
    out.into_bytes()
}

/// A CF_HTML payload: the `Version`/offset header, then a fragment built from HTML pieces;
/// sometimes a NUL or invalid UTF-8 inside.
fn gen_cf_html(rng: &mut Rng) -> Vec<u8> {
    let pieces: [&[u8]; 16] = [
        b"<b>bold</b>",
        b"<p>para</p>",
        b"<script>x()</script>",
        b"<!-- c -->",
        b"&amp;&lt;&#x1F600;",
        b"Ti\xe1\xba\xbfng Vi\xe1\xbb\x87t",
        b"<a href='x>y'>l</a>",
        b"\r\n",
        b"\0",
        b"\xff\xfe",
        b"\xe1\xba",
        b"\xf0\x9f\x98\x80",
        b"<br>",
        b"<style>p{}</style>",
        b"  \t ",
        b"<table><tr><td>1</td></tr></table>",
    ];
    let mut frag = Vec::new();
    for _ in 0..rng.range(0, 12) {
        frag.extend_from_slice(pieces[rng.below(pieces.len())]);
    }
    let pre = b"<html><body>\r\n<!--StartFragment-->";
    let post = b"<!--EndFragment-->\r\n</body>\r\n</html>";
    let header_len = 105usize;
    let start_frag = header_len + pre.len();
    let end_frag = start_frag + frag.len();
    let end_html = end_frag + post.len();
    let header = format!(
        "Version:0.9\r\nStartHTML:{header_len:010}\r\nEndHTML:{end_html:010}\r\nStartFragment:{start_frag:010}\r\nEndFragment:{end_frag:010}\r\n"
    );
    let mut out = vec![rng.next_u64() as u8];
    out.extend_from_slice(header.as_bytes());
    out.extend_from_slice(pre);
    out.extend_from_slice(&frag);
    out.extend_from_slice(post);
    out
}

const DIB_DICT: &[&[u8]] = &[
    &[40, 0, 0, 0],
    &[124, 0, 0, 0],
    &[108, 0, 0, 0],
    &[0, 0, 0xFF, 0],
    &[0, 0xFF, 0, 0],
    &[0xFF, 0, 0, 0],
    &[0, 0, 0, 0xFF],
    &[0, 0x80, 0, 0],
    &[0xFF, 0xFF, 0xFF, 0x7F],
    &[3, 0, 0, 0],
];

const TEXT_DICT: &[&[u8]] = &[
    b"file:///",
    b"file://",
    b"%",
    b"%2F",
    b"%00",
    b"\\\\",
    b"C:",
    b"\r\n",
    b"\0\0",
    b"#",
    b"Version:0.9\r\n",
    b"StartFragment:",
    b"<!--StartFragment-->",
    b"\xed\xa0\x80",
    b"\x00\xd8",
    b"\x00\xdc",
];

fn fuzz<'a>(name: &'static str, dict: &'a [&'a [u8]], mem: usize, secs: u64) -> Fuzz<'a> {
    Fuzz {
        name,
        max_len: 64 * 1024,
        dict,
        mem_budget: mem,
        time_limit: Duration::from_secs(secs),
        fixup: None,
    }
}

#[test]
#[ignore = "fuzz harness: run on demand in release mode (docs/testing/fuzzing.md)"]
fn fuzz_dib_to_png() {
    // Pixel data must be present, so the RGBA buffer is bounded by the input (64 KiB).
    fuzz("dib_to_png", DIB_DICT, 16 * MIB, 5).run(gen_dib, dib);
}

#[test]
#[ignore = "fuzz harness: run on demand in release mode (docs/testing/fuzzing.md)"]
fn fuzz_png_to_dibs() {
    let mut f = fuzz("png_to_dibs", PNG_DICT, 3 * RGBA_CAP + 16 * MIB, 60);
    f.fixup = Some(fix_png_crcs);
    f.run(gen_png, png_dibs);
}

#[test]
#[ignore = "fuzz harness: run on demand in release mode (docs/testing/fuzzing.md)"]
fn fuzz_dropfiles() {
    fuzz("dropfiles", TEXT_DICT, 8 * MIB, 5).run(gen_dropfiles, dropfiles);
}

#[test]
#[ignore = "fuzz harness: run on demand in release mode (docs/testing/fuzzing.md)"]
fn fuzz_uri_list() {
    fuzz("uri_list", TEXT_DICT, 8 * MIB, 5).run(gen_uri_list, uri_list_block);
}

#[test]
#[ignore = "fuzz harness: run on demand in release mode (docs/testing/fuzzing.md)"]
fn fuzz_cf_html() {
    // A 1 MiB cap on a 64 KiB input: the outputs are bounded by the input.
    fuzz("cf_html", TEXT_DICT, 8 * MIB, 5).run(gen_cf_html, cf_html);
}

/// Writes the committed seed corpus (`fuzz/corpus/<target>/`). Run by hand when the
/// generators change; the files are small and synthetic.
#[test]
#[ignore = "writes fuzz/corpus; run by hand"]
fn write_seed_corpus() {
    type Gen = fn(&mut Rng) -> Vec<u8>;
    let gens: [(&str, Gen); 5] = [
        ("dib_to_png", gen_dib),
        ("png_to_dibs", gen_png),
        ("dropfiles", gen_dropfiles),
        ("uri_list", gen_uri_list),
        ("cf_html", gen_cf_html),
    ];
    for (i, (name, gen)) in gens.into_iter().enumerate() {
        let mut rng = Rng::new(100 + i as u64);
        let mut inputs: Vec<Vec<u8>> = (0..6).map(|_| gen(&mut rng)).collect();
        if name == "png_to_dibs" {
            // The minimised Task 406 finding: a 4-byte PLTE chunk.
            inputs.push(darkclip_fuzzgen::png::indexed_1x1(&[1; 4]));
        }
        darkclip_fuzzgen::write_corpus(name, &inputs).unwrap();
    }
}

/// The generators must produce inputs the converters accept, or the runs above only
/// exercise the first rejection. Not ignored: fast and deterministic.
#[test]
fn generators_reach_the_converters() {
    let mut rng = Rng::new(42);
    let (mut dibs, mut pngs, mut drops, mut uris) = (0, 0, 0, 0);
    for _ in 0..300 {
        let d = gen_dib(&mut rng);
        dibs += usize::from(dib_to_png(&d[1..], u64::MAX).is_ok());
        pngs += usize::from(png_to_dibs(&gen_png(&mut rng)).is_ok());
        let f = gen_dropfiles(&mut rng);
        drops += usize::from(parse_dropfiles(&f[1..], true, 1 << 20).is_some());
        #[cfg(windows)]
        {
            uris += usize::from(
                crate::access::uri_list_to_dropfiles(&gen_uri_list(&mut rng)).is_some(),
            );
        }
    }
    assert!(dibs >= 50, "DIBs converted: {dibs}");
    assert!(pngs >= 200, "PNGs converted: {pngs}");
    assert!(drops >= 100, "DROPFILES parsed: {drops}");
    if cfg!(windows) {
        assert!(uris >= 20, "uri-lists converted: {uris}");
    }
}
