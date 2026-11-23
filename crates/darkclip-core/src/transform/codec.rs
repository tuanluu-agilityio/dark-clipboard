//! Percent-encoding and Base64 (standard alphabet).

use super::{TransformError, MAX_INPUT_BYTES};

fn is_unreserved(b: u8) -> bool {
    b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'~')
}

const HEX: &[u8; 16] = b"0123456789ABCDEF";

/// Percent-encodes every UTF-8 byte except the RFC 3986 unreserved set.
pub(super) fn url_encode(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    for &b in input.as_bytes() {
        if is_unreserved(b) {
            out.push(char::from(b));
        } else {
            out.push('%');
            out.push(char::from(HEX[usize::from(b >> 4)]));
            out.push(char::from(HEX[usize::from(b & 0xF)]));
        }
    }
    out
}

fn hex_val(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

/// Decodes `%XX` escapes; `+` stays a plus. A malformed escape or a result that is not
/// UTF-8 is `InvalidUrl`.
pub(super) fn url_decode(input: &str) -> Result<String, TransformError> {
    let b = input.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while let Some(&c) = b.get(i) {
        if c == b'%' {
            let hi = b.get(i + 1).copied().and_then(hex_val);
            let lo = b.get(i + 2).copied().and_then(hex_val);
            match (hi, lo) {
                (Some(h), Some(l)) => out.push((h << 4) | l),
                _ => return Err(TransformError::InvalidUrl),
            }
            i += 3;
        } else {
            out.push(c);
            i += 1;
        }
    }
    String::from_utf8(out).map_err(|_| TransformError::InvalidUrl)
}

const B64: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

pub(super) fn base64_encode(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b0 = u32::from(chunk.first().copied().unwrap_or(0));
        let b1 = u32::from(chunk.get(1).copied().unwrap_or(0));
        let b2 = u32::from(chunk.get(2).copied().unwrap_or(0));
        let n = (b0 << 16) | (b1 << 8) | b2;
        let idx = |shift: u32| char::from(B64[((n >> shift) & 0x3F) as usize]);
        out.push(idx(18));
        out.push(idx(12));
        out.push(if chunk.len() > 1 { idx(6) } else { '=' });
        out.push(if chunk.len() > 2 { idx(0) } else { '=' });
    }
    out
}

fn b64_val(b: u8) -> Option<u32> {
    match b {
        b'A'..=b'Z' => Some(u32::from(b - b'A')),
        b'a'..=b'z' => Some(u32::from(b - b'a') + 26),
        b'0'..=b'9' => Some(u32::from(b - b'0') + 52),
        b'+' => Some(62),
        b'/' => Some(63),
        _ => None,
    }
}

/// Decodes Base64 (whitespace ignored, padding optional). The bytes must be UTF-8 and at
/// most `MAX_INPUT_BYTES`.
pub(super) fn base64_decode(input: &str) -> Result<String, TransformError> {
    let compact: Vec<u8> = input.bytes().filter(|b| !b.is_ascii_whitespace()).collect();
    let pad = compact.iter().rev().take_while(|&&b| b == b'=').count();
    if pad > 2 || (pad > 0 && !compact.len().is_multiple_of(4)) {
        return Err(TransformError::InvalidBase64);
    }
    let data = compact.get(..compact.len() - pad).unwrap_or_default();
    if data.len() % 4 == 1 {
        return Err(TransformError::InvalidBase64);
    }
    let mut out = Vec::with_capacity(data.len() / 4 * 3 + 2);
    for chunk in data.chunks(4) {
        let mut n = 0u32;
        for &b in chunk {
            n = (n << 6) | b64_val(b).ok_or(TransformError::InvalidBase64)?;
        }
        n <<= 6 * (4 - chunk.len() as u32);
        let bytes = [(n >> 16) as u8, (n >> 8) as u8, n as u8];
        out.extend_from_slice(bytes.get(..chunk.len() - 1).unwrap_or_default());
    }
    if out.len() > MAX_INPUT_BYTES {
        return Err(TransformError::TooLarge);
    }
    String::from_utf8(out).map_err(|_| TransformError::NotUtf8)
}
