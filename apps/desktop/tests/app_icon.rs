//! Task 710: the committed `res/dark-clipboard.ico` and the resource script. No window is
//! created (GC-19); the ICO header is parsed by hand (no extra crate).

use std::fs;
use std::path::PathBuf;

fn res(name: &str) -> Vec<u8> {
    let p = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("res")
        .join(name);
    fs::read(&p).unwrap_or_default()
}

fn u16_at(b: &[u8], i: usize) -> usize {
    usize::from(u16::from_le_bytes([b[i], b[i + 1]]))
}

fn u32_at(b: &[u8], i: usize) -> usize {
    u32::from_le_bytes([b[i], b[i + 1], b[i + 2], b[i + 3]]) as usize
}

#[test]
fn ico_has_the_eight_sizes_png_for_256_and_dib_for_the_rest() {
    let ico = res("dark-clipboard.ico");
    assert!(ico.len() > 6 + 8 * 16, "ico missing or truncated");
    assert_eq!((u16_at(&ico, 0), u16_at(&ico, 2)), (0, 1), "not an icon");
    assert_eq!(u16_at(&ico, 4), 8);
    let expected = [16usize, 20, 24, 32, 40, 48, 64, 256];
    for (i, want) in expected.iter().enumerate() {
        let e = 6 + i * 16;
        let dim = |v: u8| if v == 0 { 256 } else { usize::from(v) };
        assert_eq!((dim(ico[e]), dim(ico[e + 1])), (*want, *want));
        assert_eq!(u16_at(&ico, e + 6), 32, "bits per pixel");
        let (len, off) = (u32_at(&ico, e + 8), u32_at(&ico, e + 12));
        assert!(off + len <= ico.len(), "entry {want} runs past the file");
        let blob = &ico[off..off + len];
        if *want == 256 {
            assert_eq!(&blob[..8], b"\x89PNG\r\n\x1a\n", "256 must be PNG");
        } else {
            assert_eq!(u32_at(blob, 0), 40, "{want}: BITMAPINFOHEADER");
            assert_eq!(u32_at(blob, 4), *want, "{want}: DIB width");
            assert_eq!(
                u32_at(blob, 8),
                *want * 2,
                "{want}: DIB height (x2 for the mask)"
            );
        }
    }
}

#[test]
fn resource_script_points_at_the_ico() {
    let rc = String::from_utf8(res("app.rc")).unwrap_or_default();
    assert!(rc.contains("dark-clipboard.ico"), "app.rc: {rc}");
}

#[test]
fn window_icon_png_exists() {
    let png = res("window-64.png");
    assert_eq!(&png[..png.len().min(8)], b"\x89PNG\r\n\x1a\n");
}

/// Task 711: the window icon is raw RGBA set from Rust. A PNG named in a `.slint` file pulls the
/// image decoders into the exe (+116 KiB), so no `.slint` file may embed a raster image.
#[test]
fn window_icon_rgba_is_a_64_square() {
    assert_eq!(res("window-64.rgba").len(), 64 * 64 * 4);
}

#[test]
fn no_slint_file_embeds_a_raster_image() {
    fn walk(dir: &std::path::Path, out: &mut Vec<PathBuf>) {
        for e in fs::read_dir(dir).into_iter().flatten().flatten() {
            let p = e.path();
            if p.is_dir() {
                walk(&p, out);
            } else if p.extension().is_some_and(|x| x == "slint") {
                out.push(p);
            }
        }
    }
    let mut files = Vec::new();
    walk(
        &PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("ui"),
        &mut files,
    );
    assert!(!files.is_empty());
    for f in files {
        let src = fs::read_to_string(&f).unwrap_or_default();
        for line in src.lines().filter(|l| l.contains("@image-url")) {
            let l = line.to_ascii_lowercase();
            assert!(
                !(l.contains(".png\"")
                    || l.contains(".jpg\"")
                    || l.contains(".jpeg\"")
                    || l.contains(".gif\"")
                    || l.contains(".bmp\"")
                    || l.contains(".webp\"")),
                "{}: raster image in markup: {line}",
                f.display()
            );
        }
    }
}
