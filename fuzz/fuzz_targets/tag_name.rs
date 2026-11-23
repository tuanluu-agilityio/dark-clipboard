//! cargo-fuzz target `tag_name` (Task 406): the same entry point as the stable harness.
#![no_main]

libfuzzer_sys::fuzz_target!(|data: &[u8]| darkclip_core::fuzz_entry::tag_name(data));
