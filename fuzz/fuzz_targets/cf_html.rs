//! cargo-fuzz target `cf_html` (Task 406): the same entry point as the stable harness.
#![no_main]

libfuzzer_sys::fuzz_target!(|data: &[u8]| darkclip_windows::fuzz_entry::cf_html(data));
