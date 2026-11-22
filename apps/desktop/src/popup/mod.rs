//! Popup logic that does not need a window (Task 204): highlight ranges, the keyboard map, the
//! list view-model and placement. The Slint wiring is `ui/popup_host.rs`.

// The popup runs on Windows first; other platforms compile it but do not show it yet.
#![cfg_attr(not(windows), allow(dead_code))]

pub mod highlight;
pub mod keys;
pub mod list;
pub mod place;
