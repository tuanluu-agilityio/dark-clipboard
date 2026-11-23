//! cargo-fuzz target `uri_list` (Task 406): the same entry point as the stable harness.
#![no_main]

libfuzzer_sys::fuzz_target!(|data: &[u8]| darkclip_windows::fuzz_entry::uri_list_block(data));
