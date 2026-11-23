//! Stable fuzz harness for the core targets (Task 406). `#[ignore]`: run on demand, in
//! release mode, as documented in `docs/testing/fuzzing.md`. Secret-shaped fragments are
//! assembled at runtime (GC-09); every input is synthetic.

use super::*;
use darkclip_fuzzgen::{Fuzz, Rng, TrackingAlloc};
use std::time::Duration;

#[global_allocator]
static ALLOC: TrackingAlloc = TrackingAlloc;

const MIB: usize = 1024 * 1024;

/// Fragments that steer the generator toward the detectors' and transforms' grammar.
fn text_dict() -> Vec<Vec<u8>> {
    let pieces: Vec<String> = vec![
        concat!("sk", "-").into(),
        concat!("sk", "_live_").into(),
        concat!("rk", "_live_").into(),
        concat!("gh", "p_").into(),
        concat!("github", "_pat_").into(),
        concat!("gl", "pat-").into(),
        concat!("xo", "xb-").into(),
        concat!("xa", "pp-").into(),
        concat!("AK", "IA").into(),
        concat!("ey", "J").into(),
        concat!("eyJhbGciOi", "JIUzI1NiJ9").into(),
        concat!("eyJzdWIiOi", "IxMjM0In0").into(),
        concat!("-----BEGIN ", "RSA PRIVATE KEY-----").into(),
        concat!("-----END ", "RSA PRIVATE KEY-----").into(),
        concat!("-----BEGIN ", "OPENSSH PRIVATE KEY-----").into(),
        concat!("---- BEGIN ", "SSH2 ENCRYPTED PRIVATE KEY ----").into(),
        concat!("-----BEGIN PGP ", "PRIVATE KEY BLOCK-----").into(),
        concat!("pass", "word: ").into(),
        concat!("pw", "d=").into(),
        concat!("mật ", "khẩu: ").into(),
        concat!("DB_PASS", "WORD=").into(),
        concat!("{\"pass", "word\": \"").into(),
        "：".into(),
        concat!("4111 1111 ", "1111 1111").into(),
        concat!("5500-0000-", "0000-0004").into(),
        concat!("user", "@example.com").into(),
        "a.b+c@sub.example.org".into(),
        "+84 912 345 678".into(),
        concat!("0912", "345678").into(),
        "(+1) 202-555-0100".into(),
        "<script>".into(),
        "</script>".into(),
        "<style>".into(),
        "<!--".into(),
        "-->".into(),
        "<p>".into(),
        "<br/>".into(),
        "<a href=\"x\">".into(),
        "&amp;".into(),
        "&#x1F600;".into(),
        "&#0;".into(),
        "&nbsp;".into(),
        "&#99999999;".into(),
        "{\"a\":[1,2,{\"b\":null}]}".into(),
        "[[[[[[[[".into(),
        "]]]]".into(),
        "\"\\u00e9\\ud83d\"".into(),
        "-0.5e+10".into(),
        "%E1%BA%BF".into(),
        "%ZZ".into(),
        "%".into(),
        "QUJD".into(),
        "==".into(),
        "\r\n".into(),
        "\r".into(),
        "\t".into(),
        "  ".into(),
        "Tie\u{0302}\u{0301}ng".into(),
        "İ".into(),
        "ß".into(),
        "\u{0345}".into(),
        "ǅ".into(),
        "\u{FB03}".into(),
        "đĐ".into(),
        "\u{200B}".into(),
        "\u{FEFF}".into(),
        "😀".into(),
        "_".into(),
        "-".into(),
    ];
    pieces.into_iter().map(String::into_bytes).collect()
}

/// How many leading entries of [`text_dict`] are secret-shaped.
const SECRET_SHAPED: usize = 17;

/// A structure-aware text input: a selector byte, then a mix of dictionary fragments,
/// random alphanumeric runs, separators and the odd random byte.
fn gen_text(rng: &mut Rng, dict: &[Vec<u8>], max: usize) -> Vec<u8> {
    let mut out = vec![rng.next_u64() as u8];
    let parts = rng.range(1, 24);
    for _ in 0..parts {
        match rng.below(6) {
            0 | 1 => {
                if let Some(t) = rng.pick(dict) {
                    out.extend_from_slice(t);
                }
            }
            2 => {
                const ALNUM: &[u8] =
                    b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789";
                let n = rng.range(1, 64);
                out.extend((0..n).map(|_| ALNUM[rng.below(ALNUM.len())]));
            }
            3 => {
                let n = rng.range(1, 24);
                out.extend((0..n).map(|_| b"0123456789 -"[rng.below(12)]));
            }
            4 => out.push(*rng.pick(b" \n\t.,:;=\"'/<>&").unwrap_or(&b' ')),
            _ => {
                let n = rng.range(1, 4);
                out.extend(rng.bytes(n));
            }
        }
        if out.len() >= max {
            break;
        }
    }
    out.truncate(max);
    out
}

fn fuzz<'a>(name: &'static str, dict: &'a [&'a [u8]], max_len: usize, mem: usize) -> Fuzz<'a> {
    Fuzz {
        name,
        max_len,
        dict,
        mem_budget: mem,
        time_limit: Duration::from_secs(2),
        fixup: None,
    }
}

fn run_text(name: &'static str, max_len: usize, mem: usize, target: fn(&[u8])) {
    let owned = text_dict();
    let dict: Vec<&[u8]> = owned.iter().map(Vec::as_slice).collect();
    fuzz(name, &dict, max_len, mem).run(|rng| gen_text(rng, &owned, max_len), target);
}

#[test]
#[ignore = "fuzz harness: run on demand in release mode (docs/testing/fuzzing.md)"]
fn fuzz_sensitive() {
    run_text("sensitive", 16 * 1024, 4 * MIB, sensitive);
}

#[test]
#[ignore = "fuzz harness: run on demand in release mode (docs/testing/fuzzing.md)"]
fn fuzz_transforms() {
    run_text("transforms", 16 * 1024, 16 * MIB, transforms);
}

#[test]
#[ignore = "fuzz harness: run on demand in release mode (docs/testing/fuzzing.md)"]
fn fuzz_tag_name() {
    run_text("tag_name", 4 * 1024, 4 * MIB, tag_name);
}

/// Long inputs at the scanner window and the transform cap, a few hundred times each: the
/// mutation runs above stay short for speed, these cover the size limits.
#[test]
#[ignore = "fuzz harness: run on demand in release mode (docs/testing/fuzzing.md)"]
fn fuzz_large_inputs() {
    let owned = text_dict();
    let mut rng = Rng::new(0x0406_1A26);
    for _ in 0..300 {
        let mut big = vec![0u8];
        let target = rng.range(MAX_SCAN_BYTES - 64, MAX_INPUT_BYTES + 64);
        while big.len() < target {
            let t = rng.pick(&owned).map_or(&b"x"[..], Vec::as_slice);
            big.extend_from_slice(t);
        }
        darkclip_fuzzgen::arm();
        sensitive(&big);
        transforms(&big);
        tag_name(&big);
        // Inputs are about 1 MiB; a few copies are expected, unbounded growth is not.
        assert!(
            darkclip_fuzzgen::disarm() < 64 * MIB,
            "heap growth on a large input"
        );
    }
}

/// Writes the committed seed corpus (`fuzz/corpus/<target>/`). Run once by hand when the
/// generator changes; the files are small and synthetic.
#[test]
#[ignore = "writes fuzz/corpus; run by hand"]
fn write_seed_corpus() {
    // Without the vendor-prefix and key-armor fragments (the first 17): committed files must
    // not look like secrets to a secret scanner (GC-09).
    let owned: Vec<Vec<u8>> = text_dict().split_off(SECRET_SHAPED);
    for (name, seed) in [("sensitive", 1u64), ("transforms", 2), ("tag_name", 3)] {
        let mut rng = Rng::new(seed);
        let inputs: Vec<Vec<u8>> = (0..6).map(|_| gen_text(&mut rng, &owned, 512)).collect();
        darkclip_fuzzgen::write_corpus(name, &inputs).unwrap();
    }
}
