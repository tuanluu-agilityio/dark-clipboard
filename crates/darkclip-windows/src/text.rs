//! Pure text conversions for `read` (Task 103). No OS calls, compiled on every OS so its tests
//! run everywhere.
//!
//! Every function works on an already bounded raw buffer (the caller reads at most
//! [`utf16_raw_limit`] / [`utf8_raw_limit`] bytes) and never builds an output larger than the
//! cap, so no over-cap copy of content exists. The caller wipes the raw buffer.

#![cfg_attr(not(windows), allow(dead_code))]

/// Most raw bytes read for a UTF-16 format with a UTF-8 cap of `cap` bytes: `4 × cap + 2`.
/// Every UTF-16 unit gives at least one UTF-8 byte, so this always reaches past the cap and a
/// cut is detected.
pub(crate) fn utf16_raw_limit(cap: usize) -> usize {
    cap.saturating_mul(4).saturating_add(2)
}

/// Most raw bytes read for a UTF-8 (or byte) format capped at `cap`: one past the cap, enough
/// to know whether the content was cut.
pub(crate) fn utf8_raw_limit(cap: usize) -> usize {
    cap.saturating_add(1)
}

/// UTF-16LE bytes to UTF-8: stops at the first NUL unit, lone surrogates become U+FFFD, output
/// cut on a char boundary at `cap` bytes. Returns the text and whether it was cut. A trailing
/// odd byte is ignored.
pub(crate) fn utf16_capped(raw: &[u8], cap: usize) -> (String, bool) {
    let units = raw
        .as_chunks::<2>()
        .0
        .iter()
        .map(|&b| u16::from_le_bytes(b))
        .take_while(|&u| u != 0);
    // At most 3 UTF-8 bytes per unit; reserving up front avoids reallocations that would
    // leave copies of content in freed memory.
    let mut out = String::with_capacity(cap.min(raw.len() / 2 * 3));
    for c in char::decode_utf16(units) {
        let c = c.unwrap_or(char::REPLACEMENT_CHARACTER);
        if out.len() + c.len_utf8() > cap {
            return (out, true);
        }
        out.push(c);
    }
    (out, false)
}

/// UTF-8 bytes (CF_HTML): stops at the first NUL, invalid sequences become U+FFFD, output cut
/// on a char boundary at `cap` bytes. Returns the bytes and whether they were cut.
pub(crate) fn utf8_capped(raw: &[u8], cap: usize) -> (Vec<u8>, bool) {
    let raw = until_nul(raw);
    let mut out: Vec<u8> = Vec::with_capacity(cap.min(raw.len().saturating_mul(3)));
    for chunk in raw.utf8_chunks() {
        for c in chunk.valid().chars() {
            if !push_char(&mut out, c, cap) {
                return (out, true);
            }
        }
        if !chunk.invalid().is_empty() && !push_char(&mut out, char::REPLACEMENT_CHARACTER, cap) {
            return (out, true);
        }
    }
    (out, false)
}

fn push_char(out: &mut Vec<u8>, c: char, cap: usize) -> bool {
    let mut buf = [0u8; 4];
    let s = c.encode_utf8(&mut buf);
    if out.len() + s.len() > cap {
        return false;
    }
    out.extend_from_slice(s.as_bytes());
    true
}

/// Raw bytes (RTF) kept as they are: stops at the first NUL and cuts at `cap` bytes; when the
/// cut falls inside a UTF-8 sequence it moves back to the sequence start.
pub(crate) fn bytes_capped(raw: &[u8], cap: usize) -> (Vec<u8>, bool) {
    let raw = until_nul(raw);
    if raw.len() <= cap {
        return (raw.to_vec(), false);
    }
    let head = raw.get(..cap).unwrap_or(raw);
    let end = match std::str::from_utf8(head) {
        Err(e) if e.error_len().is_none() => e.valid_up_to(),
        _ => head.len(),
    };
    (head.get(..end).unwrap_or(head).to_vec(), true)
}

fn until_nul(raw: &[u8]) -> &[u8] {
    match raw.iter().position(|&b| b == 0) {
        Some(n) => raw.get(..n).unwrap_or(raw),
        None => raw,
    }
}

/// A Windows path (UTF-16, no NUL) as a `file:` URI (RFC 8089): drive paths become
/// `file:///C:/...`, UNC paths `file://host/share/...`; `\\?\` prefixes are dropped. Every
/// byte outside the unreserved set (and `/`, and the drive colon) is percent-encoded.
pub(crate) fn file_uri(path: &[u16]) -> String {
    let path = String::from_utf16_lossy(path).replace('\\', "/");
    let path = if let Some(rest) = path.strip_prefix("//?/UNC/") {
        format!("//{rest}")
    } else if let Some(rest) = path.strip_prefix("//?/") {
        rest.to_owned()
    } else {
        path
    };
    let mut out = String::with_capacity(path.len() * 3 + 8);
    if let Some(unc) = path.strip_prefix("//") {
        out.push_str("file://");
        encode_into(&mut out, unc, false);
    } else {
        out.push_str("file:///");
        let drive = {
            let b = path.as_bytes();
            b.len() >= 2 && b[0].is_ascii_alphabetic() && b[1] == b':'
        };
        encode_into(&mut out, path.trim_start_matches('/'), drive);
    }
    out
}

fn encode_into(out: &mut String, s: &str, drive: bool) {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    for (i, b) in s.bytes().enumerate() {
        let keep = b.is_ascii_alphanumeric()
            || matches!(b, b'-' | b'.' | b'_' | b'~' | b'/')
            || (drive && i == 1 && b == b':');
        if keep {
            out.push(char::from(b));
        } else {
            out.push('%');
            out.push(char::from(HEX[usize::from(b >> 4)]));
            out.push(char::from(HEX[usize::from(b & 0x0F)]));
        }
    }
}

/// `text/uri-list` of `paths`, CRLF-separated, capped at `cap` bytes by whole URIs (a cut URI
/// would name another file). Returns the list and whether a URI was left out.
pub(crate) fn uri_list<P: AsRef<[u16]>>(paths: &[P], cap: usize) -> (String, bool) {
    let mut out = String::new();
    for p in paths {
        let uri = file_uri(p.as_ref());
        let sep = if out.is_empty() { 0 } else { 2 };
        if out.len() + sep + uri.len() > cap {
            return (out, true);
        }
        if sep > 0 {
            out.push_str("\r\n");
        }
        out.push_str(&uri);
    }
    (out, false)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn le(units: &[u16]) -> Vec<u8> {
        units.iter().flat_map(|u| u.to_le_bytes()).collect()
    }

    fn le_str(s: &str) -> Vec<u8> {
        le(&s.encode_utf16().collect::<Vec<_>>())
    }

    #[test]
    fn utf16_truncates_on_char_boundary() {
        // "aé€😀" is 1 + 2 + 3 + 4 = 10 UTF-8 bytes.
        let raw = le_str("aé€😀");
        assert_eq!(utf16_capped(&raw, 10), ("aé€😀".to_owned(), false));
        assert_eq!(utf16_capped(&raw, 100), ("aé€😀".to_owned(), false));
        for (cap, want) in [
            (9, "aé€"),
            (6, "aé€"),
            (5, "aé"),
            (3, "aé"),
            (2, "a"),
            (1, "a"),
            (0, ""),
        ] {
            let (s, cut) = utf16_capped(&raw, cap);
            assert_eq!(s, want, "cap {cap}");
            assert!(cut, "cap {cap}");
            assert!(s.len() <= cap);
        }
        // Vietnamese with combining-free precomposed letters.
        let (s, cut) = utf16_capped(&le_str("Tiếng Việt"), 5);
        assert_eq!(s, "Tiế");
        assert!(cut);
    }

    #[test]
    fn utf16_stops_at_nul() {
        let mut raw = le_str("abc");
        raw.extend_from_slice(&[0, 0]);
        raw.extend(le_str("secret garbage after NUL"));
        assert_eq!(utf16_capped(&raw, 100), ("abc".to_owned(), false));
        // An odd trailing byte (GlobalSize rounding) is ignored.
        let mut raw = le_str("xy");
        raw.push(0x41);
        assert_eq!(utf16_capped(&raw, 100), ("xy".to_owned(), false));
        assert_eq!(utf16_capped(&[], 10), (String::new(), false));
    }

    #[test]
    fn utf16_lone_surrogate_lossy() {
        let raw = le(&[0x61, 0xD800, 0x62, 0xDC00, 0x63, 0xD83D]);
        let (s, cut) = utf16_capped(&raw, 100);
        assert_eq!(s, "a\u{FFFD}b\u{FFFD}c\u{FFFD}");
        assert!(!cut);
        // A valid pair still decodes.
        let (s, _) = utf16_capped(&le(&[0xD83D, 0xDE00]), 100);
        assert_eq!(s, "😀");
    }

    #[test]
    fn raw_limits_reach_past_the_cap() {
        assert_eq!(utf16_raw_limit(10), 42);
        assert_eq!(utf16_raw_limit(usize::MAX), usize::MAX);
        assert_eq!(utf8_raw_limit(10), 11);
        assert_eq!(utf8_raw_limit(usize::MAX), usize::MAX);
        // A raw read cut at the limit is always reported as truncated.
        let long = le_str(&"x".repeat(1000));
        let cap = 10;
        let bounded = &long[..utf16_raw_limit(cap)];
        assert_eq!(utf16_capped(bounded, cap), ("x".repeat(10), true));
    }

    #[test]
    fn utf8_html_capped_on_boundary_and_nul() {
        let html = "Version:0.9\r\n<b>Tiếng</b>".as_bytes();
        assert_eq!(utf8_capped(html, 1000), (html.to_vec(), false));
        let mut z = html.to_vec();
        z.push(0);
        z.extend_from_slice(b"junk");
        assert_eq!(utf8_capped(&z, 1000), (html.to_vec(), false));
        // "ế" is 3 bytes; cutting inside it backs off to before it.
        let pos = html.iter().position(|&b| b >= 0x80).unwrap();
        let (out, cut) = utf8_capped(html, pos + 2);
        assert!(cut);
        assert_eq!(out, html[..pos].to_vec());
        // Invalid bytes are replaced.
        let (out, cut) = utf8_capped(&[b'a', 0xFF, b'b'], 100);
        assert_eq!(out, "a\u{FFFD}b".as_bytes());
        assert!(!cut);
    }

    #[test]
    fn rtf_bytes_capped_raw() {
        let rtf = br"{\rtf1\ansi hello}";
        assert_eq!(bytes_capped(rtf, 100), (rtf.to_vec(), false));
        assert_eq!(bytes_capped(rtf, 5), (rtf[..5].to_vec(), true));
        let mut z = rtf.to_vec();
        z.extend_from_slice(&[0, 1, 2]);
        assert_eq!(bytes_capped(&z, 100), (rtf.to_vec(), false));
        // Non-UTF-8 bytes are kept as they are.
        assert_eq!(bytes_capped(&[0xE9, b'x'], 10), (vec![0xE9, b'x'], false));
        // A cut inside a UTF-8 sequence backs off.
        let s = "ab€".as_bytes();
        assert_eq!(bytes_capped(s, 3), (b"ab".to_vec(), true));
    }

    #[test]
    fn uri_list_percent_encodes_unicode_and_spaces() {
        let w = |s: &str| s.encode_utf16().collect::<Vec<u16>>();
        assert_eq!(
            file_uri(&w(r"C:\Users\Tuấn\My File.txt")),
            "file:///C:/Users/Tu%E1%BA%A5n/My%20File.txt"
        );
        assert_eq!(
            file_uri(&w(r"\\server\share\a#b%c.txt")),
            "file://server/share/a%23b%25c.txt"
        );
        assert_eq!(file_uri(&w(r"\\?\D:\x y")), "file:///D:/x%20y");
        assert_eq!(file_uri(&w(r"\\?\UNC\srv\s\f")), "file://srv/s/f");
        let paths = vec![w(r"C:\a b.txt"), w(r"D:\ñ")];
        let (list, cut) = uri_list(&paths, 1000);
        assert_eq!(list, "file:///C:/a%20b.txt\r\nfile:///D:/%C3%B1");
        assert!(!cut);
        // Capped by whole URIs.
        let (list, cut) = uri_list(&paths, 25);
        assert_eq!(list, "file:///C:/a%20b.txt");
        assert!(cut);
        let (list, cut) = uri_list(&paths, 3);
        assert_eq!(list, "");
        assert!(cut);
    }
}
