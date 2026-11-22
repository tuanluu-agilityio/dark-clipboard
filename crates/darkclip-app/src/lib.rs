//! Application layer: services, capture actor, retention and permission guard.
//!
//! Must not depend on: `tauri`, OS crates (adapters are injected).

#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used, clippy::panic))]

#[cfg(test)]
mod tests {
    #[test]
    fn crate_builds() {}
}
