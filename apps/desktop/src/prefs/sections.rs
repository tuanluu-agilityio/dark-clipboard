//! Which section of the settings window shows each user-facing setting (Task 705). The table is
//! the single list of "every existing setting stays, exactly once": `tests/settings_sections.rs`
//! reads this file and checks it against the `// section:N id` marker beside each row in
//! `ui/settings.slint`. Presentation only: no value, default or save rule lives here.

/// `(section index, setting id)`: one entry per user-facing setting or action.
// Read by the source-scan test only, so a normal build does not use it.
#[allow(dead_code)]
pub const SETTING_SECTIONS: &[(u8, &str)] = &[
    // 0 General
    (0, "shortcut"),
    (0, "language"),
    (0, "start-at-login"),
    (0, "popup-position"),
    (0, "close-on-focus-loss"),
    (0, "enter-pastes"),
    (0, "show-welcome"),
    // 1 Capture and Privacy
    (1, "capture-mode"),
    (1, "always-protected"),
    (1, "sensitive-content"),
    (1, "excluded-apps"),
    (1, "kinds"),
    (1, "skip-summary"),
    // 2 History and Storage
    (2, "max-items"),
    (2, "max-age"),
    (2, "max-storage"),
    (2, "storage-stats"),
    (2, "cleanup"),
    (2, "clear-history"),
    // 3 Data and Transfer
    (3, "backup"),
    (3, "restore"),
    (3, "import"),
    (3, "data-folder"),
    // 4 Translate (Task 606: the fifth section, so index 4; About moved from 4 to 5)
    (4, "translate-enable"),
    (4, "translate-shortcut"),
    (4, "translate-direction"),
    (4, "translate-model"),
    (4, "translate-key"),
    (4, "translate-test"),
    (4, "translate-never-sent"),
    // 5 About (section 4 until Task 606)
    (5, "version"),
    (5, "auto-update"),
    (5, "check-updates"),
    (5, "capabilities"),
    (5, "licences"),
];
