//! Translate in place (Phase 6, Task 605): a second global shortcut copies the selection of
//! the window in front, translates it with the user's own Anthropic key and pastes the result
//! over it. `flow` is the pure controller behind the `FlowSys` seam; `host` does the real calls
//! on a worker thread. Windows only; off by default (D-093).

pub(crate) mod flow;
pub(crate) mod host;
