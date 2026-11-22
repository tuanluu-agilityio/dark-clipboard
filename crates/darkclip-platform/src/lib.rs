//! Platform traits, capability types and a `FakeClipboard` for tests.
//!
//! Must not depend on: OS crates (only traits, capability types, a fake for tests).

#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used, clippy::panic))]

#[cfg(test)]
mod tests {
    #[test]
    fn crate_builds() {}
}
