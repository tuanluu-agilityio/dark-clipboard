//! Storage: SQLite, FTS5, blobs and migrations.
//!
//! Must not depend on: OS crates, `tauri`.

#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used, clippy::panic))]

#[cfg(test)]
mod tests {
    #[test]
    fn crate_builds() {}
}
