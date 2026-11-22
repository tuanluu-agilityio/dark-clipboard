//! History window logic that does not need a window (Task 211): multi-select and delete
//! confirmation, the keyboard map and window placement. The filters, the list, the detail pane
//! and the row text are the popup's pure view-models (`crate::popup`), reused as they are. The
//! Slint wiring is `ui/history_host.rs`.

// The history window runs on Windows first; other platforms compile it but do not show it yet.
#![cfg_attr(not(windows), allow(dead_code))]

pub mod keys;
pub mod place;
pub mod select;
