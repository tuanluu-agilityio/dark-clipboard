//! Settings window logic that does not need a window (Task 207): the editable draft with its
//! validation, the excluded-apps list with the confirmation for built-in entries, the locked
//! markers row, the read-only banner, the clear-history flow and the About texts. The Slint
//! wiring is `ui/settings_host.rs`. Nothing here reads or logs clipboard content.

// The settings window runs on Windows first; other platforms compile it but do not show it yet.
#![cfg_attr(not(windows), allow(dead_code))]

pub mod about;
pub mod model;
#[cfg(test)]
mod model_tests;
pub mod sys;
pub mod welcome;
