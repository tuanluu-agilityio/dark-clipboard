//! Pure text conversions for `read` (Task 103). No OS calls, compiled on every OS so its tests
//! run everywhere.
//!
//! Every function works on an already bounded raw buffer (the caller reads at most
//! [`utf16_raw_limit`] / [`utf8_raw_limit`] bytes) and never builds an output larger than the
//! cap, so no over-cap copy of content exists. The caller wipes the raw buffer.

#![cfg_attr(not(windows), allow(dead_code))]

use zeroize::Zeroizing;

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
/// Intermediate and result buffers are reserved up front (no reallocation copies) and wiped
/// on drop.
pub(crate) fn file_uri(path: &[u16]) -> Zeroizing<String> {
    // At most 3 UTF-8 bytes per UTF-16 unit.
    let mut slashed = Zeroizing::new(String::with_capacity(path.len().saturating_mul(3)));
    for c in char::decode_utf16(path.iter().copied()) {
        let c = c.unwrap_or(char::REPLACEMENT_CHARACTER);
        slashed.push(if c == '\\' { '/' } else { c });
    }
    let p: &str = slashed.as_str();
    let (unc, rest) = if let Some(r) = p.strip_prefix("//?/UNC/") {
        (true, r)
    } else if let Some(r) = p.strip_prefix("//?/") {
        (false, r)
    } else if let Some(r) = p.strip_prefix("//") {
        (true, r)
    } else {
        (false, p.trim_start_matches('/'))
    };
    // Every byte becomes at most 3 characters.
    let mut out = Zeroizing::new(String::with_capacity(
        rest.len().saturating_mul(3).saturating_add(8),
    ));
    if unc {
        out.push_str("file://");
        encode_into(&mut out, rest, false);
    } else {
        out.push_str("file:///");
        let b = rest.as_bytes();
        let drive = matches!(b, [d, b':', ..] if d.is_ascii_alphabetic());
        encode_into(&mut out, rest, drive);
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

/// File names of a `CF_HDROP` (UTF-16, no NUL), wiped on drop.
pub(crate) struct FileNames {
    pub names: Vec<Zeroizing<Vec<u16>>>,
    /// Names were left out (budget, name count or a cut copy).
    pub more: bool,
}

/// Size of the `DROPFILES` header.
const DROPFILES_LEN: usize = 20;
/// Room allowed for a `pFiles` offset beyond the plain header.
const DROPFILES_SLACK: usize = 4096;
/// Most names taken from one block.
pub(crate) const MAX_DROP_FILES: usize = 65_536;

/// Most bytes copied from a `CF_HDROP` block for a cap of `cap` bytes.
pub(crate) fn dropfiles_raw_limit(cap: usize) -> usize {
    utf16_raw_limit(cap).saturating_add(DROPFILES_SLACK)
}

/// Parses a `DROPFILES` block copied from `CF_HDROP` (bounded by `GlobalSize` and
/// [`dropfiles_raw_limit`]) without trusting it: `pFiles` must point past the header and
/// inside the block; names are NUL-terminated and the list ends with an empty name (double
/// NUL). `complete` says whether `block` is the whole `GlobalSize` block: then a missing
/// terminator is malformed (`None`); otherwise the copy was cut, the partial name is dropped
/// and `more` is set. Takes names until the next would pass `max_units` UTF-16 units in
/// total, or [`MAX_DROP_FILES`] names. ANSI blocks (`fWide = 0`) are accepted only when every
/// name is ASCII (other bytes need the system code page). Never panics.
pub(crate) fn parse_dropfiles(block: &[u8], complete: bool, max_units: usize) -> Option<FileNames> {
    let u32_at = |off: usize| -> Option<u32> {
        let s = block.get(off..off.checked_add(4)?)?;
        Some(u32::from_le_bytes([
            *s.first()?,
            *s.get(1)?,
            *s.get(2)?,
            *s.get(3)?,
        ]))
    };
    if block.len() < DROPFILES_LEN {
        return None;
    }
    let p_files = usize::try_from(u32_at(0)?).ok()?;
    let wide = u32_at(16)? != 0;
    if p_files < DROPFILES_LEN || p_files >= block.len() {
        return None;
    }
    let list = block.get(p_files..)?;
    let wide_units = list.as_chunks::<2>().0;
    let count = if wide { wide_units.len() } else { list.len() };
    let unit = |i: usize| -> Option<u16> {
        if wide {
            wide_units.get(i).map(|&b| u16::from_le_bytes(b))
        } else {
            list.get(i).map(|&b| u16::from(b))
        }
    };

    let mut names: Vec<Zeroizing<Vec<u16>>> = Vec::new();
    let mut total = 0usize;
    let mut pos = 0usize;
    loop {
        // Find the end of the next name without copying it.
        let mut end = pos;
        while end < count && unit(end)? != 0 {
            if !wide && unit(end)? >= 0x80 {
                return None;
            }
            end += 1;
        }
        if end >= count {
            // No NUL before the end of the copy.
            return if complete {
                None
            } else {
                Some(FileNames { names, more: true })
            };
        }
        let len = end - pos;
        if len == 0 {
            // The empty name: end of the list.
            return Some(FileNames { names, more: false });
        }
        if names.len() >= MAX_DROP_FILES || total.saturating_add(len) > max_units {
            return Some(FileNames { names, more: true });
        }
        let mut name = Zeroizing::new(Vec::with_capacity(len));
        for i in pos..end {
            name.push(unit(i)?);
        }
        total += len;
        names.push(name);
        pos = end + 1;
    }
}

/// Upper bound of one URI's length: 9 characters per UTF-16 unit (3 UTF-8 bytes, each
/// percent-encoded) plus the `file:///` prefix.
fn uri_bound(units: usize) -> usize {
    units.saturating_mul(9).saturating_add(8)
}

/// `text/uri-list` of `paths`, CRLF-separated, capped at `cap` bytes by whole URIs (a cut URI
/// would name another file). Returns the list and whether a URI was left out. The output is
/// reserved up front (no reallocation copies); a left-out URI is wiped.
pub(crate) fn uri_list<P: AsRef<[u16]>>(paths: &[P], cap: usize) -> (String, bool) {
    let bound = paths.iter().fold(0usize, |acc, p| {
        acc.saturating_add(uri_bound(p.as_ref().len()))
            .saturating_add(2)
    });
    let mut out = String::with_capacity(cap.min(bound));
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
pub(crate) mod tests {
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
            file_uri(&w(r"C:\Users\Tuấn\My File.txt")).as_str(),
            "file:///C:/Users/Tu%E1%BA%A5n/My%20File.txt"
        );
        assert_eq!(
            file_uri(&w(r"\\server\share\a#b%c.txt")).as_str(),
            "file://server/share/a%23b%25c.txt"
        );
        assert_eq!(file_uri(&w(r"\\?\D:\x y")).as_str(), "file:///D:/x%20y");
        assert_eq!(file_uri(&w(r"\\?\UNC\srv\s\f")).as_str(), "file://srv/s/f");
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

    /// A DROPFILES block: 20-byte header (`pFiles`, `pt`, `fNC`, `fWide`), then the names,
    /// each NUL-terminated, then a final NUL.
    pub(crate) fn dropfiles(names: &[&str], wide: bool) -> Vec<u8> {
        let mut b = Vec::new();
        b.extend_from_slice(&20u32.to_le_bytes());
        b.extend_from_slice(&[0u8; 12]);
        b.extend_from_slice(&u32::from(wide).to_le_bytes());
        for n in names {
            if wide {
                b.extend(n.encode_utf16().chain([0]).flat_map(u16::to_le_bytes));
            } else {
                b.extend_from_slice(n.as_bytes());
                b.push(0);
            }
        }
        if wide {
            b.extend_from_slice(&[0, 0]);
        } else {
            b.push(0);
        }
        b
    }

    fn names(f: &FileNames) -> Vec<String> {
        f.names
            .iter()
            .map(|n| String::from_utf16(n).unwrap())
            .collect()
    }

    #[test]
    fn dropfiles_wide_and_ansi_parsed() {
        let block = dropfiles(&[r"C:\Tuấn\a b.txt", r"\\srv\s\x"], true);
        let f = parse_dropfiles(&block, true, 1000).unwrap();
        assert_eq!(names(&f), vec![r"C:\Tuấn\a b.txt", r"\\srv\s\x"]);
        assert!(!f.more);
        // GlobalSize padding after the terminator is ignored.
        let mut padded = block.clone();
        padded.extend_from_slice(&[0xAB; 7]);
        assert_eq!(
            names(&parse_dropfiles(&padded, true, 1000).unwrap()).len(),
            2
        );
        // ANSI names, ASCII only.
        let f = parse_dropfiles(&dropfiles(&[r"C:\a.txt"], false), true, 1000).unwrap();
        assert_eq!(names(&f), vec![r"C:\a.txt"]);
        // No files at all.
        let f = parse_dropfiles(&dropfiles(&[], true), true, 1000).unwrap();
        assert!(f.names.is_empty() && !f.more);
        // A larger pFiles (extra header bytes) is honoured.
        let mut b = dropfiles(&[r"C:\x"], true);
        b.splice(20..20, [0u8; 8]);
        b[0..4].copy_from_slice(&28u32.to_le_bytes());
        assert_eq!(
            names(&parse_dropfiles(&b, true, 1000).unwrap()),
            vec![r"C:\x"]
        );
    }

    #[test]
    fn dropfiles_malformed_blocks_rejected() {
        let good = dropfiles(&[r"C:\a", r"C:\b"], true);
        // Shorter than the header.
        for cut in 0..20 {
            assert!(
                parse_dropfiles(&good[..cut], true, 1000).is_none(),
                "cut {cut}"
            );
        }
        // pFiles inside the header or past the block.
        for p in [0u32, 19, u32::try_from(good.len()).unwrap(), u32::MAX] {
            let mut b = good.clone();
            b[0..4].copy_from_slice(&p.to_le_bytes());
            assert!(parse_dropfiles(&b, true, 1000).is_none(), "pFiles {p}");
        }
        // A complete block without the double-NUL terminator.
        let no_final = &good[..good.len() - 2];
        assert!(parse_dropfiles(no_final, true, 1000).is_none());
        let no_nul = &good[..good.len() - 4];
        assert!(parse_dropfiles(no_nul, true, 1000).is_none());
        // ANSI names outside ASCII need the code page: rejected.
        let mut ansi = dropfiles(&[r"C:\a"], false);
        ansi[23] = 0xE9;
        assert!(parse_dropfiles(&ansi, true, 1000).is_none());
    }

    #[test]
    fn dropfiles_cut_block_and_budget() {
        let good = dropfiles(&[r"C:\aaaa", r"C:\bbbb"], true);
        // The copy was cut by the read limit inside the second name: first name kept, the
        // partial one dropped, `more` set.
        let cut = &good[..20 + 8 * 2 + 6];
        let f = parse_dropfiles(cut, false, 1000).unwrap();
        assert_eq!(names(&f), vec![r"C:\aaaa"]);
        assert!(f.more);
        // The budget stops before the name that would pass it.
        let f = parse_dropfiles(&good, true, 10).unwrap();
        assert_eq!(names(&f), vec![r"C:\aaaa"]);
        assert!(f.more);
        let f = parse_dropfiles(&good, true, 3).unwrap();
        assert!(f.names.is_empty() && f.more);
        // The read limit leaves room for the header.
        assert!(dropfiles_raw_limit(10) > utf16_raw_limit(10) + 20);
    }
}
