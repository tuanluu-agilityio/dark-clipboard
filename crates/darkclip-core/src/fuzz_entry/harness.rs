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

/// How many leading entries of [`text_dict`] are aimed at the sensitive detectors: vendor
/// prefixes, token and key armour, password keywords, card numbers, emails and phone
/// numbers (through `"(+1) 202-555-0100"`). The fuzz runs use them; the committed corpus
/// never does (GC-09).
const SENSITIVE_SHAPED: usize = 30;

/// A structure-aware text input: a selector byte, then a mix of dictionary fragments,
/// random alphanumeric runs, separators and the odd random byte.
/// `digit_runs`: also emit runs of digits, spaces and dashes (card- and phone-like); off
/// for the committed corpus.
fn gen_text(rng: &mut Rng, dict: &[Vec<u8>], max: usize, digit_runs: bool) -> Vec<u8> {
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
            3 if digit_runs => {
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
    fuzz(name, &dict, max_len, mem).run(|rng| gen_text(rng, &owned, max_len, true), target);
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
    // Without the sensitive-shaped fragments and digit runs: committed files must not look
    // like secrets, card numbers or personal data (GC-09). `committed_corpus_is_not_sensitive`
    // checks the result.
    let owned: Vec<Vec<u8>> = text_dict().split_off(SENSITIVE_SHAPED);
    for (name, seed) in [("sensitive", 1u64), ("transforms", 2), ("tag_name", 3)] {
        let mut rng = Rng::new(seed);
        let inputs: Vec<Vec<u8>> = (0..6)
            .map(|_| gen_text(&mut rng, &owned, 512, false))
            .collect();
        darkclip_fuzzgen::write_corpus(name, &inputs).unwrap();
    }
}

/// Keywords and prefixes that must never appear in a committed seed file (GC-09), lower case
/// and assembled at runtime.
fn forbidden_fragments() -> Vec<String> {
    let mut v: Vec<String> = text_dict()
        .into_iter()
        .take(SENSITIVE_SHAPED)
        .map(|f| String::from_utf8_lossy(&f).trim().to_lowercase())
        .filter(|f| f.chars().count() >= 3)
        .collect();
    for k in [
        concat!("pass", "word"),
        concat!("pass", "wd"),
        concat!("pw", "d"),
        concat!("sec", "ret"),
        concat!("tok", "en"),
        concat!("mật ", "khẩu"),
        concat!("mat ", "khau"),
        concat!("priv", "ate key"),
    ] {
        v.push(k.to_string());
    }
    v
}

/// Every committed seed file (all targets): the sensitive scanner finds nothing in it, and
/// it holds no secret keyword or vendor prefix (GC-09). Not ignored.
#[test]
fn committed_corpus_is_not_sensitive() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fuzz/corpus");
    let forbidden = forbidden_fragments();
    let all = CATEGORIES.to_vec();
    let mut files = 0;
    for dir in std::fs::read_dir(&root).unwrap() {
        let dir = dir.unwrap().path();
        for f in std::fs::read_dir(&dir).unwrap() {
            let path = f.unwrap().path();
            let bytes = std::fs::read(&path).unwrap();
            let name = path.display().to_string();
            for text in [
                String::from_utf8_lossy(&bytes),
                String::from_utf8_lossy(bytes.get(1..).unwrap_or_default()),
            ] {
                assert!(
                    scan(&text, &all).categories.is_empty(),
                    "scanner hit in {name}"
                );
                let lower = text.to_lowercase();
                for k in &forbidden {
                    assert!(!lower.contains(k.as_str()), "forbidden fragment in {name}");
                }
            }
            files += 1;
        }
    }
    assert!(files >= 13 * 6, "corpus files found: {files}");
}
