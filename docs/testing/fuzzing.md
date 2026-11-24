# Fuzzing (Task 406)

Date: 2026-11-24. Machine: Windows 11 Home Single Language 10.0.26300, x64, 22 logical cores, the owner's work machine (in use during the runs).
Toolchain: `rustc 1.100.0 (0ef1bc386 2026-11-09)`, stable `x86_64-pc-windows-msvc`.

## What ran, and what did not

- **libFuzzer did not run.** This machine has only the stable MSVC toolchain and no `cargo-fuzz`; `cargo fuzz` needs nightly. No result on this page comes from libFuzzer or from coverage-guided fuzzing.
- What ran is the package's **stable deterministic fallback**: `#[ignore]` tests that feed each target inputs from a seeded generator (SplitMix64) with structure-aware generators and mutations (`crates/darkclip-fuzzgen`). It has no coverage feedback; depth comes from the structure-aware generators (valid DIBs, PNGs of every colour type and depth with chunk CRCs recomputed after mutation, DROPFILES blocks, `text/uri-list`s, CF_HTML payloads, backup and export zips whose manifests match their entries), a token dictionary per target and stacked mutations (a pool of earlier mutated inputs). Non-ignored tests (`generators_reach_the_converters`, `generators_reach_the_deep_checks`) check that the generators get past the first rejection.
- The cargo-fuzz targets exist in `fuzz/` (13 targets, one per stable target, calling the same `fuzz_entry` function) for a later run on a machine with nightly: see [Manual libFuzzer run](#manual-libfuzzer-run). Here they were only type-checked on stable (`cd fuzz && cargo check --bins`: finished without errors, `libfuzzer-sys` 0.4); they were never built with the sanitizer or run.

For every input the runner checks: no panic (each call runs under `catch_unwind`), the target's own invariants (`assert!` in each `fuzz_entry` function: output caps, round trips, categories, verified sizes), the live heap growth of the call against a per-target budget (a tracking global allocator in the test binary), and a per-call time limit. A finding saves the input to `target/fuzz-findings/` and fails the test with the target, seed and iteration; the input is never printed.

## Targets

| Target | Code under test | Entry point | Heap budget per call | Time limit per call |
|---|---|---|---|---|
| `dib_to_png` | `CF_DIB`/`CF_DIBV5` → PNG (`darkclip-windows` `dib::dib_to_png`), PNG cap from the selector byte | `darkclip_windows::fuzz_entry::dib` | 16 MiB | 5 s |
| `png_to_dibs` | stored PNG → `CF_DIBV5` + `CF_DIB` (copy-back, `dib::png_to_dibs`), then both back to PNG | `darkclip_windows::fuzz_entry::png_dibs` | 3 × 256 MiB + 16 MiB (the decode caps, by design) | 60 s |
| `png_decode` | capture PNG decode with the core caps (`darkclip_app::image::decode_png`), downscale, thumbnail | `darkclip_app::fuzz_entry::png_decode` | 2 × 256 MiB + 32 MiB (by design) | 60 s |
| `dropfiles` | `CF_HDROP` parser (`text::parse_dropfiles`), then `text/uri-list` building, then (Windows) the list back to DROPFILES | `darkclip_windows::fuzz_entry::dropfiles` | 8 MiB | 5 s |
| `uri_list` | stored `text/uri-list` → DROPFILES for paste (`access::uri_list_to_dropfiles`, Windows only) | `darkclip_windows::fuzz_entry::uri_list_block` | 8 MiB | 5 s |
| `cf_html` | the `HTML Format` read path: capped UTF-8 copy (`text::utf8_capped`), also `utf16_capped` and `bytes_capped`, then the preview and HTML-to-text | `darkclip_windows::fuzz_entry::cf_html` | 8 MiB | 5 s |
| `sensitive` | all seven sensitive scanners (`darkclip_core::sensitive::scan`), masked preview | `darkclip_core::fuzz_entry::sensitive` | 4 MiB | 2 s |
| `transforms` | all 13 transforms of Task 404, round trips (URL, Base64, JSON minify/pretty) | `darkclip_core::fuzz_entry::transforms` | 16 MiB | 2 s |
| `tag_name` | tag normaliser (`Tag::validate_name`, idempotent) and `make_preview` | `darkclip_core::fuzz_entry::tag_name` | 4 MiB | 2 s |
| `archive_names` | archive entry-name allow-list and hex parsing (`archive::classify_entry`, `parse_hex32`) | `darkclip_storage::fuzz_entry::archive_names` | 1 MiB | 10 s |
| `backup_manifest` | `manifest.json` parsing (`backup::parse_manifest`) | `darkclip_storage::fuzz_entry::backup_manifest` | 8 MiB | 10 s |
| `backup_archive` | a `.dcbackup` as restore reads it: zip checks, manifest, manifest vs entries, every file read with size and SHA-256 verified, database extraction | `darkclip_storage::fuzz_entry::backup_archive` | 300 MiB (by design, see below) | 10 s |
| `export_archive` | a `.dcexport` as import reads it: zip checks, `export.json`, blob list, every blob read (some twice) | `darkclip_storage::fuzz_entry::export_archive` | 300 MiB (by design) | 10 s |

Notes:
- There is **no separate CF_HTML header parser** in the code: Windows stores the `HTML Format` payload whole, header included, and nothing parses `StartHTML`/`StartFragment`. The `cf_html` target fuzzes what does read the payload.
- There is **no TIFF decoder** in the code: `CF_TIFF` is only named (`darkclip_windows::formats`, for the format list) and its bytes are never decoded, so the TIFF input of the Phase 5 Review Focus 5 is not applicable and has no target. (Added by D-072, 2026-11-24.)
- The archive targets read the zip from memory through `CheckedZip::from_reader`, the same checks `CheckedZip::open` runs on a file (`open` now delegates to it). The SQLite checks on a restored database (integrity, schema, content) and the live swap are not fuzzed here: they run SQLite on the file, and are covered by the Task 405 crafted-database tests.
- The heap budgets of the image and archive targets equal what the design allows: a small PNG may claim an image up to the decode caps (64 MP, 256 MiB of RGBA, D-043), and the archive readers read a JSON entry (up to 64 MiB) or a blob (up to its declared size) into memory. A small input can therefore make these readers allocate up to those caps. This is bounded and accepted, not a finding; the peaks seen are in the tables below.
- Time limits catch hangs and super-linear behaviour; the slowest call of each run is recorded. The core test binary also runs `fuzz_large_inputs` (300 inputs of 256 KiB to 1 MiB built from the dictionary, through the three core targets, heap growth under 64 MiB).

## Commands

From the repository root, Git Bash. The environment overrides keep `debug-assertions` and `overflow-checks` on (integer overflow panics, as under cargo-fuzz) and use a fast optimised build in a separate target folder; `panic = "abort"` of the release profile does not apply to test binaries.

```bash
export CARGO_PROFILE_RELEASE_LTO=false CARGO_PROFILE_RELEASE_CODEGEN_UNITS=16 \
  CARGO_PROFILE_RELEASE_OPT_LEVEL=3 CARGO_PROFILE_RELEASE_DEBUG_ASSERTIONS=true \
  CARGO_PROFILE_RELEASE_OVERFLOW_CHECKS=true CARGO_PROFILE_RELEASE_STRIP=false \
  CARGO_TARGET_DIR=target/fuzz-harness
export DARKCLIP_FUZZ_ITERS=1000000      # per target (the default)
# optional: DARKCLIP_FUZZ_SECS=3600 (keep going at least this long), DARKCLIP_FUZZ_SEED=<n>
for c in darkclip-core darkclip-app darkclip-windows darkclip-storage; do
  cargo test --release -p $c --lib fuzz_entry::harness::fuzz_ -- --ignored --nocapture
done
```

Each target prints one line: `fuzz <target>: iterations=… seed=… elapsed=… slowest_call=… peak_heap=…B findings=0`.
To reproduce a crash that aborts the process (for example an allocation failure), rerun the same target and seed with `DARKCLIP_FUZZ_SAVE_LAST=1`: every input is written to `target/fuzz-findings/<target>-last.bin` before the call.
The seed corpora in `fuzz/corpus/<target>/` are written by the `write_seed_corpus` tests (`cargo test -p <crate> --lib write_seed_corpus -- --ignored`); they are small and synthetic. The corpus writers leave out every fragment aimed at the sensitive detectors (vendor prefixes, token and key armour, password keywords, card numbers, emails, phone numbers) and the digit runs; the fuzz runs still use them, in memory only (GC-09). The non-ignored test `committed_corpus_is_not_sensitive` (darkclip-core) runs the sensitive scanner over every committed seed file, with and without its selector byte, and checks that none contains a secret keyword or vendor prefix. (Before the Task 406 review, the text corpora still held password keywords and a card-like number; they were regenerated.) `fuzz/.gitattributes` keeps them byte-exact.

The quality gate never runs these: they are `#[ignore]`.

## Results

### Run 1: 1,000,000 iterations per target (default seeds)

Wall time: the four crates ran in parallel (four `cargo test` processes), each running its targets on parallel threads. Built before timing.

| Target | Iterations | Seed | Elapsed | Slowest call | Peak heap growth | Findings |
|---|---|---|---|---|---|---|
| `sensitive` | 1,000,000 | `0xc3248acb0a8cb50` | 100.4 s | 12.9 ms | 131,094 B | 0 |
| `transforms` | 1,000,000 | `0xba2a75db9268be27` | 273.9 s | 35.6 ms | 393,965 B | 0 |
| `tag_name` | 1,000,000 | `0x55598d3cf4bb1220` | 91.0 s | 17.5 ms | 49,916 B | 0 |
| `png_decode` | 1,000,000 | `0x619452d955c1a5c4` | 126.8 s | 21.5 ms | 16,802,936 B | 0 |
| `dib_to_png` | 1,000,000 | `0x55f607ecc6589f5b` | 67.8 s | 19.1 ms | 372,735 B | 0 |
| `png_to_dibs` | 1,000,000 | `0x7d1342fb0adc1dda` | 165.4 s | 102.0 ms | 143,554,748 B | 0 |
| `dropfiles` | 1,000,000 | `0xa1096ed491637fb4` | 26.2 s | 7.7 ms | 798,013 B | 0 |
| `uri_list` | 1,000,000 | `0xc779cff43a6fc7ed` | 18.6 s | 5.1 ms | 223,092 B | 0 |
| `cf_html` | 1,000,000 | `0xf050e90bdddd10d3` | 43.1 s | 11.7 ms | 472,631 B | 0 |
| `archive_names` | 1,000,000 | `0xd77464b554e4485` | 3.8 s | 2.6 ms | 1,024 B | 0 |
| `backup_manifest` | 1,000,000 | `0xa100e60780f6f9fa` | 232.1 s | 10.6 ms | 50,496 B | 0 |
| `backup_archive` | 1,000,000 | `0xc4d39f3cc1497751` | 344.4 s | 122.7 ms | 152,846 B | 0 |
| `export_archive` | 1,000,000 | `0x3656bbff5cae1b25` | 265.3 s | 25.6 ms | 150,880 B | 0 |

`fuzz_large_inputs` (core): passed. All four `cargo test` processes exited 0 (`test result: ok`).
The `png_to_dibs` peak (137 MiB) is within the 256 MiB decode cap. Inputs are at most 64 KiB, so it comes from a header (IHDR) that claims a larger image than the data holds: the output buffer is sized from the header before the data is read (bounded by the caps, see the notes above).

### Run 2: one hour per target (second base seed)

Same build and commands with `DARKCLIP_FUZZ_SECS=3600 DARKCLIP_FUZZ_SEED=0x2026`, all 13 targets in parallel (four `cargo test` processes), 02:50 to 03:51. The quality gate (`scripts/check.ps1`) and `cargo deny` ran on the same machine during the first minutes, and the machine was in use, so the slowest-call times include scheduling noise; none came near a time limit.

| Target | Iterations | Seed | Elapsed | Slowest call | Peak heap growth | Findings |
|---|---|---|---|---|---|---|
| `sensitive` | 30,493,771 | `0xc3248acb4ae3769` | 3600.0 s | 77.7 ms | 131,116 B | 0 |
| `transforms` | 8,228,076 | `0xba2a75db966e421e` | 3600.0 s | 254.4 ms | 398,709 B | 0 |
| `tag_name` | 32,865,276 | `0x55598d3cf0bdee19` | 3600.0 s | 94.3 ms | 52,860 B | 0 |
| `png_decode` | 24,477,156 | `0x619452d951c759fd` | 3600.0 s | 499.3 ms | 260,350,008 B | 0 |
| `dib_to_png` | 44,223,683 | `0x55f607ecc25e6362` | 3600.0 s | 110.3 ms | 391,480 B | 0 |
| `png_to_dibs` | 11,775,479 | `0x7d1342fb0edae1e3` | 3600.0 s | 286.6 ms | 148,819,635 B | 0 |
| `dropfiles` | 102,059,246 | `0xa1096ed49565838d` | 3600.0 s | 207.3 ms | 1,038,711 B | 0 |
| `uri_list` | 141,918,627 | `0xc779cff43e693bd4` | 3600.0 s | 441.8 ms | 262,244 B | 0 |
| `cf_html` | 71,493,694 | `0xf050e90bd9dbecea` | 3600.0 s | 136.2 ms | 566,526 B | 0 |
| `archive_names` | 832,984,613 | `0xd77464b5148b8bc` | 3600.0 s | 82.1 ms | 1,024 B | 0 |
| `backup_manifest` | 8,952,100 | `0xa100e60784f005c3` | 3600.0 s | 166.7 ms | 51,466 B | 0 |
| `backup_archive` | 5,790,767 | `0xc4d39f3cc54f8b68` | 3600.0 s | 332.9 ms | 152,482 B | 0 |
| `export_archive` | 6,781,423 | `0x3656bbff58a8e71c` | 3600.0 s | 350.2 ms | 153,984 B | 0 |

All four `cargo test` processes exited 0 (`test result: ok`, wall 3601 to 3602 s each). The `png_decode` peak (248 MiB) is a header claiming an image near the 256 MiB decode cap: bounded by the caps, as designed (D-043).

## Findings

### F1: PNG with a malformed palette panicked the `png` decoder (fixed)

- **Found by:** the stable harness, target `png_to_dibs`, during a 50,000-iteration smoke run (seed `0x7d1342fb0adc1dda`, iteration 21,287), before the runs above.
- **What:** `png` 0.18.1 panics (`range end index 3 out of range for slice of length 1`, `src/decoder/transform/palette.rs:74`) while expanding a `PLTE` chunk whose length is not a multiple of 3. A length outside 3 to 768 bytes is already refused by `png` itself (`ChunkLengthWrong`, `src/decoder/stream.rs`), so only the multiple-of-3 case panics; the pre-check below refuses both anyway. Both PNG decoders use the palette expansion (`Transformations::normalize_to_color8`): the capture decode (`darkclip_app::image::decode_png`) and the copy-back conversion (`darkclip_windows` `png_to_dibs`). The release profile uses `panic = "abort"`, so such a PNG on the clipboard, in history (copy-back), or in an imported export or a restored backup would have ended the process.
- **Minimised input:** a 1×1 8-bit indexed PNG whose `PLTE` chunk is 4 bytes long (`darkclip_fuzzgen::png::indexed_1x1(&[1; 4])`, 86 bytes). Kept as `fuzz/corpus/png_to_dibs/seed-06` and `fuzz/corpus/png_decode/seed-06`.
- **Regression tests (committed first, failing before the fix):** `dib::tests::png_with_malformed_palette_is_refused_not_a_panic` (darkclip-windows) and `image::tests::png_with_malformed_palette_is_corrupt_not_a_panic` (darkclip-app), each with palette lengths 4, 2 and 1 (these panicked before the fix) and 769 and 771 (already refused by `png`), plus a well-formed control.
- **Fix:** `darkclip_core::limits::png_palette_ok` walks the PNG chunk headers in one linear pass without allocating and returns `false` when a `PLTE` chunk is not 1 to 256 whole entries (the second half duplicates `png`'s own length check, so the fix does not depend on it); both decoders refuse such a file before the `png` crate sees it (`ImageError::Corrupt`, `DibError::Malformed`). Unit test `limits::tests::png_palette_lengths`. The other `png` user, `darkclip-storage`'s thumbnail check, reads the header only (`read_header_info`) and never expands a palette.
- **Privacy relevance:** the fix sits in the capture path (image decoding before dedupe and thumbnails) and the copy-back path. It only refuses more inputs; it reads no content earlier and logs nothing.
- **Upstream:** worth reporting to `image-rs/image-png` (not done from this task: no network use from agents for project data). Until a fixed release is adopted, the pre-check stays.

No other crash, panic, invariant failure, over-budget allocation or over-time call was found.

## Manual libFuzzer run

On a machine with a nightly toolchain (Linux or macOS is simplest; Windows MSVC also works with recent nightlies):

```bash
rustup toolchain install nightly
cargo install cargo-fuzz
cd fuzz
cargo +nightly fuzz list
# one target for an hour, starting from the committed seeds:
cargo +nightly fuzz run png_to_dibs corpus/png_to_dibs -- -max_total_time=3600 -rss_limit_mb=2048 -max_len=65536
```

`-rss_limit_mb=2048` leaves room for the by-design decode caps (up to about 800 MiB for `png_to_dibs`). The `fuzz/` crate is its own workspace (excluded from the product workspace), so it is never built by the quality gate or checked by `cargo deny`; it depends on `libfuzzer-sys` 0.4 and enables the `fuzzing` feature of the four crates, which exposes their `fuzz_entry` modules (never enabled by a workspace crate). `uri_list` does nothing on non-Windows targets (the converter is Windows-only code). The archive targets write a scratch database file in the system temp folder when they extract one. A crash lands in `fuzz/artifacts/<target>/` (ignored by git); minimise it with `cargo +nightly fuzz tmin <target> <file>`, then add a regression test next to the code and the minimised file to the corpus. Fuzzing is not part of CI (package: "Not doing").
