//! Canonical content hash (`docs/architecture/data-model.md` section 5).

use crate::classify::RawRepresentation;
use crate::model::{ClipType, Mime};
use sha2::{Digest, Sha256};

fn kind_name(kind: ClipType) -> &'static str {
    match kind {
        ClipType::Text => "text",
        ClipType::RichText => "rich_text",
        ClipType::Url => "url",
        ClipType::Color => "color",
        ClipType::Code => "code",
        ClipType::Image => "image",
        ClipType::Files => "files",
    }
}

fn find(reps: &[RawRepresentation], mime: Mime) -> Option<&RawRepresentation> {
    reps.iter().find(|r| r.mime == mime)
}

/// Plain text with CRLF and lone CR normalized to LF; trailing newline kept.
fn normalize_text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes)
        .replace("\r\n", "\n")
        .replace('\r', "\n")
}

/// Non-comment, non-empty lines of a `text/uri-list`, sorted, newline-joined.
fn sorted_uris(bytes: &[u8]) -> String {
    let text = normalize_text(bytes);
    let mut lines: Vec<&str> = text
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .collect();
    lines.sort_unstable();
    lines.join("\n")
}

/// For `ClipType::Image`, pass decoded RGBA. Without it the hash falls back to the
/// encoded bytes under a separate `enc\0` tag: dedupe is degraded (the same image in
/// different encodings no longer matches).
pub fn canonical_hash(
    kind: ClipType,
    reps: &[RawRepresentation],
    decoded_rgba: Option<(&[u8], u32, u32)>,
) -> [u8; 32] {
    let mut h = Sha256::new();
    h.update(b"dc1\0");
    h.update(kind_name(kind).as_bytes());
    h.update(b"\0");
    match kind {
        ClipType::Image => match decoded_rgba {
            Some((px, w, hgt)) => {
                h.update(w.to_le_bytes());
                h.update(hgt.to_le_bytes());
                h.update(px);
            }
            // No decoded pixels: degraded dedupe over the encoded bytes, domain-separated
            // from the pixel hash. Callers should pass decoded RGBA.
            None => {
                h.update(b"enc\0");
                if let Some(r) = find(reps, Mime::ImagePng) {
                    h.update(&r.bytes);
                }
            }
        },
        ClipType::Files => {
            if let Some(r) = find(reps, Mime::UriList) {
                h.update(sorted_uris(&r.bytes).as_bytes());
            }
        }
        _ => {
            if let Some(r) = find(reps, Mime::TextPlain) {
                h.update(normalize_text(&r.bytes).as_bytes());
            }
        }
    }
    h.finalize().into()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Mime;

    fn text(s: &str) -> Vec<RawRepresentation> {
        vec![RawRepresentation {
            mime: Mime::TextPlain,
            platform_format: "CF_UNICODETEXT".into(),
            bytes: s.as_bytes().to_vec(),
            width: None,
            height: None,
            truncated: false,
        }]
    }

    fn uris(s: &str) -> Vec<RawRepresentation> {
        vec![RawRepresentation {
            mime: Mime::UriList,
            platform_format: "CF_HDROP".into(),
            bytes: s.as_bytes().to_vec(),
            width: None,
            height: None,
            truncated: false,
        }]
    }

    #[test]
    fn crlf_and_lf_hash_equal() {
        assert_eq!(
            canonical_hash(ClipType::Text, &text("a\r\nb"), None),
            canonical_hash(ClipType::Text, &text("a\nb"), None)
        );
        assert_ne!(
            canonical_hash(ClipType::Text, &text("a"), None),
            canonical_hash(ClipType::Text, &text("b"), None)
        );
    }

    #[test]
    fn trailing_newline_differs() {
        assert_ne!(
            canonical_hash(ClipType::Text, &text("a"), None),
            canonical_hash(ClipType::Text, &text("a\n"), None)
        );
    }

    #[test]
    fn same_pixels_same_hash_regardless_of_format() {
        let px = [1u8, 2, 3, 255, 4, 5, 6, 255];
        let mk = |fmt: &str, bytes: Vec<u8>| {
            vec![RawRepresentation {
                mime: Mime::ImagePng,
                platform_format: fmt.into(),
                bytes,
                width: Some(2),
                height: Some(1),
                truncated: false,
            }]
        };
        let a = canonical_hash(ClipType::Image, &mk("PNG", vec![9, 9]), Some((&px, 2, 1)));
        let b = canonical_hash(ClipType::Image, &mk("CF_DIB", vec![7]), Some((&px, 2, 1)));
        assert_eq!(a, b);
        let px2 = [1u8, 2, 3, 255, 4, 5, 6, 254];
        assert_ne!(
            a,
            canonical_hash(ClipType::Image, &mk("PNG", vec![9, 9]), Some((&px2, 2, 1)))
        );
    }

    #[test]
    fn files_order_independent_hash() {
        assert_eq!(
            canonical_hash(ClipType::Files, &uris("file:///a\nfile:///b"), None),
            canonical_hash(ClipType::Files, &uris("file:///b\r\nfile:///a"), None)
        );
        assert_ne!(
            canonical_hash(ClipType::Files, &uris("file:///a"), None),
            canonical_hash(ClipType::Files, &uris("file:///b"), None)
        );
    }

    #[test]
    fn image_fallback_hash_differs_from_pixel_hash() {
        let px = [1u8, 2, 3, 255];
        let pixel = canonical_hash(ClipType::Image, &[], Some((&px, 1, 1)));
        let mut same_bytes = 1u32.to_le_bytes().to_vec();
        same_bytes.extend_from_slice(&1u32.to_le_bytes());
        same_bytes.extend_from_slice(&px);
        let reps = vec![RawRepresentation {
            mime: Mime::ImagePng,
            platform_format: "PNG".into(),
            bytes: same_bytes,
            width: Some(1),
            height: Some(1),
            truncated: false,
        }];
        assert_ne!(pixel, canonical_hash(ClipType::Image, &reps, None));
    }

    #[test]
    fn kind_is_part_of_the_hash() {
        assert_ne!(
            canonical_hash(ClipType::Text, &text("#fff"), None),
            canonical_hash(ClipType::Color, &text("#fff"), None)
        );
    }
}
