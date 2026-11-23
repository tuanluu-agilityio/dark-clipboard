//! cargo-fuzz target `backup_manifest` (Task 406): the same entry point as the stable harness.
#![no_main]

libfuzzer_sys::fuzz_target!(|data: &[u8]| darkclip_storage::fuzz_entry::backup_manifest(data));
