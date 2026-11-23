//! cargo-fuzz target `dib_to_png` (Task 406): the same entry point as the stable harness.
#![no_main]

libfuzzer_sys::fuzz_target!(|data: &[u8]| darkclip_windows::fuzz_entry::dib(data));
