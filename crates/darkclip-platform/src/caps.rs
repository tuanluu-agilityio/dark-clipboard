//! Capability report (ADR-0004). `Unsupported` is a normal value: the UI renders it and
//! features hide or degrade.

use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Support {
    Native,
    Polling { interval_ms: u32 },
    Unsupported { reason: &'static str },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SourceSupport {
    Owner,
    Frontmost,
    None,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ShortcutSupport {
    Native,
    Portal,
    UserBound,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct Capabilities {
    pub monitor: Support,
    pub read_without_focus: Support,
    pub write: Support,
    /// Can the adapter see OS/app privacy markers before content?
    pub concealed_markers: Support,
    pub source_app: SourceSupport,
    pub global_shortcut: ShortcutSupport,
    pub auto_paste: Support,
    pub tray: Support,
    pub autostart: Support,
    /// "win32", "appkit", "x11-xfixes", "ext-data-control-v1", "fake", ...
    pub backend: &'static str,
}
