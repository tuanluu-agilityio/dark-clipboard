//! Domain models, policy, dedupe and sensitive-content detection.
//!
//! Must not depend on: any OS crate, `rusqlite`, `tauri`, any async runtime, the network.

#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used, clippy::panic))]

#[cfg(test)]
mod tests {
    #[test]
    fn crate_builds() {}
}
