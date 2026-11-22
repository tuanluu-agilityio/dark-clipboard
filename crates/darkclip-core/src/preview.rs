//! Single-line previews. Never logged (GC-06).

use crate::model::PREVIEW_MAX_CHARS;

/// At most 512 chars, whitespace runs collapsed to one space, no newlines.
pub fn make_preview(text: &str) -> String {
    let mut out = String::new();
    let mut count = 0usize;
    for word in text.split_whitespace() {
        if !out.is_empty() {
            if count >= PREVIEW_MAX_CHARS {
                break;
            }
            out.push(' ');
            count += 1;
        }
        for c in word.chars() {
            if count >= PREVIEW_MAX_CHARS {
                break;
            }
            out.push(c);
            count += 1;
        }
        if count >= PREVIEW_MAX_CHARS {
            break;
        }
    }
    out
}

/// `"Image 1280x720"` (with the multiplication sign). The UI localizes the word "Image".
pub fn image_preview(w: u32, h: u32) -> String {
    format!("Image {w}\u{00d7}{h}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preview_truncates_to_512_chars_on_char_boundary() {
        let text = "Tie\u{0302}\u{0301}ng Vie\u{0323}\u{0302}t ".repeat(200);
        let p = make_preview(&text);
        assert_eq!(p.chars().count(), 512);
        assert!(std::str::from_utf8(p.as_bytes()).is_ok());
        assert!(!p.contains('\n'));
    }

    #[test]
    fn preview_collapses_whitespace() {
        assert_eq!(make_preview("  a \n\t b\r\n\r\nc  "), "a b c");
    }

    #[test]
    fn image_preview_format() {
        assert_eq!(image_preview(1280, 720), "Image 1280\u{00d7}720");
    }
}
