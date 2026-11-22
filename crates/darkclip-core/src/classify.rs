//! Classification of captured representations into a `ClipType`.

use crate::model::{ClipMetadata, ClipType, Mime};
use crate::preview::{image_preview, make_preview};

/// One format as read from the OS clipboard, before storage.
#[derive(Clone)]
pub struct RawRepresentation {
    pub mime: Mime,
    pub platform_format: String,
    pub bytes: Vec<u8>,
    pub width: Option<u32>,
    pub height: Option<u32>,
    /// The adapter cut this format at the plan's byte cap.
    pub truncated: bool,
}

/// Prints mime, format and length only (GC-06).
impl std::fmt::Debug for RawRepresentation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RawRepresentation")
            .field("mime", &self.mime)
            .field("platform_format", &self.platform_format)
            .field("len", &self.bytes.len())
            .finish()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Classified {
    pub kind: ClipType,
    pub preview: String,
    pub metadata: ClipMetadata,
    pub plain_text: Option<String>,
}

fn is_url(t: &str) -> bool {
    if t.is_empty() || t.len() > 2048 || t.chars().any(char::is_whitespace) {
        return false;
    }
    let Some((scheme, rest)) = t.split_once(':') else {
        return false;
    };
    match scheme.to_ascii_lowercase().as_str() {
        "http" | "https" | "ftp" | "file" => rest.starts_with("//") && rest.len() > 2,
        "mailto" => !rest.is_empty(),
        _ => false,
    }
}

fn is_color(t: &str) -> bool {
    if let Some(hex) = t.strip_prefix('#') {
        return matches!(hex.len(), 3 | 6 | 8) && hex.chars().all(|c| c.is_ascii_hexdigit());
    }
    let lower = t.to_ascii_lowercase();
    for prefix in ["rgb(", "rgba(", "hsl("] {
        if let Some(inner) = lower.strip_prefix(prefix).and_then(|r| r.strip_suffix(')')) {
            return inner.chars().any(|c| c.is_ascii_digit())
                && inner
                    .chars()
                    .all(|c| c.is_ascii_digit() || matches!(c, ' ' | ',' | '.' | '%' | '/' | '-'));
        }
    }
    false
}

fn find(reps: &[RawRepresentation], mime: Mime) -> Option<&RawRepresentation> {
    reps.iter().find(|r| r.mime == mime)
}

fn count_u64(n: usize) -> u64 {
    u64::try_from(n).unwrap_or(u64::MAX)
}

pub fn classify(reps: &[RawRepresentation]) -> Classified {
    let truncated = reps.iter().any(|r| r.truncated);
    if let Some(r) = find(reps, Mime::UriList) {
        let text = String::from_utf8_lossy(&r.bytes);
        let lines: Vec<&str> = text
            .lines()
            .map(str::trim)
            .filter(|l| !l.is_empty() && !l.starts_with('#'))
            .collect();
        return Classified {
            kind: ClipType::Files,
            preview: make_preview(&lines.join(" ")),
            metadata: ClipMetadata {
                file_count: Some(count_u64(lines.len())),
                truncated,
                ..ClipMetadata::default()
            },
            plain_text: None,
        };
    }
    let plain = find(reps, Mime::TextPlain).map(|r| String::from_utf8_lossy(&r.bytes).into_owned());
    if plain.is_none() {
        if let Some(img) = find(reps, Mime::ImagePng) {
            let preview = match (img.width, img.height) {
                (Some(w), Some(h)) => image_preview(w, h),
                _ => "Image".to_owned(),
            };
            return Classified {
                kind: ClipType::Image,
                preview,
                metadata: ClipMetadata {
                    truncated,
                    ..ClipMetadata::default()
                },
                plain_text: None,
            };
        }
    }
    let text = plain.unwrap_or_default();
    let trimmed = text.trim();
    let kind = if find(reps, Mime::TextHtml).is_some() || find(reps, Mime::TextRtf).is_some() {
        ClipType::RichText
    } else if is_url(trimmed) {
        ClipType::Url
    } else if is_color(trimmed) {
        ClipType::Color
    } else {
        ClipType::Text
    };
    Classified {
        kind,
        preview: make_preview(&text),
        metadata: ClipMetadata {
            char_count: Some(count_u64(text.chars().count())),
            line_count: Some(count_u64(text.lines().count())),
            truncated,
            ..ClipMetadata::default()
        },
        plain_text: Some(text),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rep(mime: Mime, s: &str) -> RawRepresentation {
        RawRepresentation {
            mime,
            platform_format: "X".into(),
            bytes: s.as_bytes().to_vec(),
            width: None,
            height: None,
            truncated: false,
        }
    }

    fn kind_of(s: &str) -> ClipType {
        classify(&[rep(Mime::TextPlain, s)]).kind
    }

    #[test]
    fn url_detection() {
        assert_eq!(kind_of("https://example.com/a?b=1"), ClipType::Url);
        assert_eq!(kind_of("  mailto:a@b.c \n"), ClipType::Url);
        assert_eq!(kind_of("see https://x.y"), ClipType::Text);
        assert_eq!(kind_of("example.com"), ClipType::Text);
        assert_eq!(kind_of("javascript:alert(1)"), ClipType::Text);
        let long = format!("https://a.b/{}", "x".repeat(2100));
        assert_eq!(kind_of(&long), ClipType::Text);
    }

    #[test]
    fn color_detection() {
        assert_eq!(kind_of("#1e222a"), ClipType::Color);
        assert_eq!(kind_of("rgb(1, 2, 3)"), ClipType::Color);
        assert_eq!(kind_of("#fff"), ClipType::Color);
        assert_eq!(kind_of("#11223344"), ClipType::Color);
        assert_eq!(kind_of("rgba(1,2,3,0.5)"), ClipType::Color);
        assert_eq!(kind_of("hsl(120, 50%, 50%)"), ClipType::Color);
        assert_eq!(kind_of("#12345"), ClipType::Text);
        assert_eq!(kind_of("#ggg"), ClipType::Text);
    }

    #[test]
    fn rich_text_when_html_present() {
        let c = classify(&[
            rep(Mime::TextPlain, "hello"),
            rep(Mime::TextHtml, "<b>hello</b>"),
        ]);
        assert_eq!(c.kind, ClipType::RichText);
        assert_eq!(c.plain_text.as_deref(), Some("hello"));
        let c = classify(&[
            rep(Mime::TextPlain, "hello"),
            rep(Mime::TextRtf, "{\\rtf1}"),
        ]);
        assert_eq!(c.kind, ClipType::RichText);
    }

    #[test]
    fn plain_text_metadata() {
        let c = classify(&[rep(Mime::TextPlain, "ab\ncd")]);
        assert_eq!(c.kind, ClipType::Text);
        assert_eq!(c.metadata.char_count, Some(5));
        assert_eq!(c.metadata.line_count, Some(2));
        assert_eq!(c.preview, "ab cd");
    }

    #[test]
    fn files_from_uri_list() {
        let c = classify(&[rep(Mime::UriList, "# c\nfile:///a\r\nfile:///b\n")]);
        assert_eq!(c.kind, ClipType::Files);
        assert_eq!(c.metadata.file_count, Some(2));
    }

    #[test]
    fn image_kind_and_preview() {
        let mut r = rep(Mime::ImagePng, "");
        r.width = Some(1280);
        r.height = Some(720);
        let c = classify(&[r]);
        assert_eq!(c.kind, ClipType::Image);
        assert_eq!(c.preview, "Image 1280\u{00d7}720");
        assert_eq!(c.plain_text, None);
    }

    #[test]
    fn raw_representation_debug_hides_bytes() {
        let r = rep(Mime::TextPlain, "topsecretvalue");
        let d = format!("{r:?}");
        assert!(!d.contains("topsecretvalue"));
        assert!(!d.contains("116"));
        assert!(d.contains("len"));
        assert!(d.contains("14"));
    }

    #[test]
    fn truncated_rep_sets_metadata_flag() {
        let mut r = rep(Mime::TextPlain, "hello");
        assert!(!classify(&[r.clone()]).metadata.truncated);
        r.truncated = true;
        assert!(classify(&[r]).metadata.truncated);
        let mut u = rep(Mime::UriList, "file:///a");
        u.truncated = true;
        assert!(classify(&[u]).metadata.truncated);
    }
}
