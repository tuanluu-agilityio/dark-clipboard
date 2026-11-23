//! cargo-fuzz target `png_to_dibs` (Task 406): the same entry point as the stable harness.
#![no_main]

libfuzzer_sys::fuzz_target!(|data: &[u8]| darkclip_windows::fuzz_entry::png_dibs(data));
