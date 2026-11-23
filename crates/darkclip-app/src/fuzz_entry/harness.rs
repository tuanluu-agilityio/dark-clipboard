//! Stable fuzz harness for the capture-side PNG decoder (Task 406). `#[ignore]`: run on
//! demand, in release mode, as documented in `docs/testing/fuzzing.md`. Every input is
//! synthetic.

use super::*;
use darkclip_fuzzgen::png::{gen_png, PNG_DICT};
use darkclip_fuzzgen::{fix_png_crcs, Fuzz, Rng, TrackingAlloc};
use std::time::Duration;

#[global_allocator]
static ALLOC: TrackingAlloc = TrackingAlloc;

const MIB: usize = 1024 * 1024;

#[test]
#[ignore = "fuzz harness: run on demand in release mode (docs/testing/fuzzing.md)"]
fn fuzz_png_decode() {
    // By design a small file may claim an image up to the caps: the decoder buffer and the
    // RGBA copy are each bounded by 256 MiB, plus the decoder's 8 MiB working limit.
    let budget = 2 * MAX_RGBA_BYTES as usize + 32 * MIB;
    Fuzz {
        name: "png_decode",
        max_len: 64 * 1024,
        dict: PNG_DICT,
        mem_budget: budget,
        time_limit: Duration::from_secs(60),
        fixup: Some(fix_png_crcs),
    }
    .run(gen_png, png_decode);
}

/// Writes the committed seed corpus (`fuzz/corpus/png_decode/`). Run by hand when the
/// generator changes; the files are small and synthetic.
#[test]
#[ignore = "writes fuzz/corpus; run by hand"]
fn write_seed_corpus() {
    let mut rng = Rng::new(200);
    let mut inputs: Vec<Vec<u8>> = (0..6).map(|_| gen_png(&mut rng)).collect();
    // The minimised Task 406 finding: a 4-byte PLTE chunk.
    inputs.push(darkclip_fuzzgen::png::indexed_1x1(&[1; 4]));
    darkclip_fuzzgen::write_corpus("png_decode", &inputs).unwrap();
}
