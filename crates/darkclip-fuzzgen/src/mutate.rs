//! Byte-level and token-level mutations, and a PNG checksum fixer.

use crate::Rng;

const BYTES: [u8; 14] = [
    0, 1, 0x7F, 0x80, 0xFF, b'%', b'<', b'>', b'\n', b'\r', b'/', b'\\', b'"', b'.',
];
const U16S: [u16; 8] = [0, 1, 0x7FFF, 0x8000, 0xFFFF, 0xD800, 0xDC00, 0x00FF];
const U32S: [u32; 16] = [
    0,
    1,
    2,
    20,
    40,
    124,
    0xFF,
    0x8000,
    32_768,
    32_769,
    65_536,
    0x7FFF_FFFF,
    0x8000_0000,
    0xFFFF_FFFF,
    0xFFFF_FFFE,
    0x0100_0000,
];

fn write_le(buf: &mut [u8], at: usize, bytes: &[u8]) {
    if let Some(slot) = buf.get_mut(at..at.saturating_add(bytes.len())) {
        slot.copy_from_slice(bytes);
    }
}

/// Applies one random mutation to `buf`, keeping it at most `max_len` bytes.
pub fn mutate(rng: &mut Rng, buf: &mut Vec<u8>, dict: &[&[u8]], max_len: usize) {
    let len = buf.len();
    match rng.below(14) {
        // Flip one bit.
        0 if len > 0 => {
            let i = rng.below(len);
            buf[i] ^= 1 << rng.below(8);
        }
        // Random byte.
        1 if len > 0 => {
            let i = rng.below(len);
            buf[i] = rng.next_u64() as u8;
        }
        // Interesting byte.
        2 if len > 0 => {
            let i = rng.below(len);
            buf[i] = *rng.pick(&BYTES).unwrap_or(&0);
        }
        // Interesting u16 (little endian).
        3 if len >= 2 => {
            let i = rng.below(len - 1);
            let v = *rng.pick(&U16S).unwrap_or(&0);
            write_le(buf, i, &v.to_le_bytes());
        }
        // Interesting u32, little or big endian (PNG is big endian).
        4 if len >= 4 => {
            let i = rng.below(len - 3);
            let v = *rng.pick(&U32S).unwrap_or(&0);
            let b = if rng.one_in(2) {
                v.to_le_bytes()
            } else {
                v.to_be_bytes()
            };
            write_le(buf, i, &b);
        }
        // Small delta on a u32.
        5 if len >= 4 => {
            let i = rng.below(len - 3);
            let mut a = [0u8; 4];
            a.copy_from_slice(&buf[i..i + 4]);
            let d = rng.range(1, 64) as u32;
            let v = if rng.one_in(2) {
                u32::from_le_bytes(a).wrapping_add(d)
            } else {
                u32::from_le_bytes(a).wrapping_sub(d)
            };
            write_le(buf, i, &v.to_le_bytes());
        }
        // Insert random bytes.
        6 => {
            let at = rng.below(len + 1);
            let n = rng.range(1, 16);
            let ins = rng.bytes(n);
            buf.splice(at..at, ins);
        }
        // Delete a range.
        7 if len > 0 => {
            let at = rng.below(len);
            let n = rng.range(1, (len - at).min(64));
            buf.drain(at..at + n);
        }
        // Duplicate a range somewhere else.
        8 if len > 0 => {
            let at = rng.below(len);
            let n = rng.range(1, (len - at).min(256));
            let chunk = buf[at..at + n].to_vec();
            let to = rng.below(len + 1);
            buf.splice(to..to, chunk);
        }
        // Overwrite with a dictionary token.
        9 if len > 0 && !dict.is_empty() => {
            let tok = *rng.pick(dict).unwrap_or(&&[][..]);
            let at = rng.below(len);
            let end = (at + tok.len()).min(len);
            buf[at..end].copy_from_slice(&tok[..end - at]);
        }
        // Insert a dictionary token.
        10 if !dict.is_empty() => {
            let tok = *rng.pick(dict).unwrap_or(&&[][..]);
            let at = rng.below(len + 1);
            buf.splice(at..at, tok.iter().copied());
        }
        // Truncate.
        11 if len > 0 => {
            let n = rng.below(len);
            buf.truncate(n);
        }
        // A token repeated many times (long runs: quadratic behaviour, deep nesting).
        12 if !dict.is_empty() => {
            let tok = *rng.pick(dict).unwrap_or(&&[][..]);
            let times = rng.range(2, 2048);
            let at = rng.below(len + 1);
            let run: Vec<u8> = tok
                .iter()
                .copied()
                .cycle()
                .take(tok.len().saturating_mul(times).min(max_len))
                .collect();
            buf.splice(at..at, run);
        }
        // Swap two bytes.
        _ if len >= 2 => {
            let a = rng.below(len);
            let b = rng.below(len);
            buf.swap(a, b);
        }
        _ => buf.push(rng.next_u64() as u8),
    }
    buf.truncate(max_len);
}

fn crc_table() -> [u32; 256] {
    let mut t = [0u32; 256];
    for (n, slot) in t.iter_mut().enumerate() {
        let mut c = n as u32;
        for _ in 0..8 {
            c = if c & 1 != 0 {
                0xEDB8_8320 ^ (c >> 1)
            } else {
                c >> 1
            };
        }
        *slot = c;
    }
    t
}

/// CRC-32 (ISO-HDLC, as used by PNG and zip).
pub fn crc32(data: &[u8]) -> u32 {
    static TABLE: std::sync::OnceLock<[u32; 256]> = std::sync::OnceLock::new();
    let t = TABLE.get_or_init(crc_table);
    !data.iter().fold(0xFFFF_FFFFu32, |c, &b| {
        t[((c ^ u32::from(b)) & 0xFF) as usize] ^ (c >> 8)
    })
}

/// Recomputes the CRC of every PNG chunk whose length fits in `buf`, so that mutations of
/// chunk data and headers reach the decoder instead of failing the checksum. Stops at the
/// first chunk that does not fit.
#[allow(clippy::ptr_arg)] // Matches the `Fuzz::fixup` signature.
pub fn fix_png_crcs(buf: &mut Vec<u8>) {
    let mut at = 8usize;
    while let Some(len_bytes) = buf.get(at..at + 4) {
        let mut a = [0u8; 4];
        a.copy_from_slice(len_bytes);
        let len = u32::from_be_bytes(a) as usize;
        let data_end = match at.checked_add(8).and_then(|v| v.checked_add(len)) {
            Some(v) => v,
            None => return,
        };
        if data_end + 4 > buf.len() {
            return;
        }
        let crc = crc32(&buf[at + 4..data_end]);
        buf[data_end..data_end + 4].copy_from_slice(&crc.to_be_bytes());
        at = data_end + 4;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crc32_known_value() {
        assert_eq!(crc32(b"IEND"), 0xAE42_6082);
        assert_eq!(crc32(b"123456789"), 0xCBF4_3926);
    }

    #[test]
    fn mutate_respects_max_len() {
        let mut rng = Rng::new(1);
        let dict: &[&[u8]] = &[b"abc", b"<div>"];
        let mut buf = b"hello world".to_vec();
        for _ in 0..10_000 {
            mutate(&mut rng, &mut buf, dict, 300);
            assert!(buf.len() <= 300);
        }
    }
}
