//! Data management in Settings > History (Task 411): the cleanup confirmation flow with the
//! counts it shows, input checks and the byte formatting. Pure logic, tested everywhere; the
//! Slint wiring is `ui/settings_host.rs`. Nothing here reads or logs clipboard content.

use darkclip_core::i18n::Msg;

/// Days accepted for "delete clips older than".
pub const DAYS_MIN: u32 = 1;
pub const DAYS_MAX: u32 = 3_650;

/// A cleanup that asks first and then deletes (clear history has its own `ClearFlow`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CleanupAction {
    OlderThan(u32),
    Orphans,
    Compact,
}

/// How many clips "clear history" would delete for each choice of the checkbox.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ClearCounts {
    /// Pinned and favorite clips kept.
    pub keeping: u64,
    /// Everything.
    pub everything: u64,
}

impl ClearCounts {
    pub fn for_choice(&self, keep_pinned_and_favorites: bool) -> u64 {
        let _ = keep_pinned_and_favorites;
        0
    }
}

/// The confirmation of one cleanup: nothing is deleted without an open dialog.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct CleanupFlow {
    open: Option<(CleanupAction, u64)>,
}

impl CleanupFlow {
    /// Opens the dialog; `shown` is the count (or, for compaction, the database size in bytes)
    /// the dialog names.
    pub fn ask(&mut self, action: CleanupAction, shown: u64) {
        let _ = (action, shown);
    }

    pub fn cancel(&mut self) {
        self.open = None;
    }

    /// The action when the dialog was open; closes it. `None` otherwise.
    pub fn confirm(&mut self) -> Option<CleanupAction> {
        None
    }

    pub fn is_open(&self) -> bool {
        self.open.is_some()
    }

    /// The question and the number it shows.
    pub fn prompt(&self) -> Option<(Msg, u64)> {
        None
    }
}

/// "3650" -> 3650; empty, zero, signs, spaces inside, too large and non-digits -> `None`.
pub fn parse_days(text: &str) -> Option<u32> {
    let _ = text;
    None
}

/// 1024-based, one decimal from KB up: `0 B`, `512 B`, `1.5 KB`, `2.0 MB`.
pub fn format_bytes(n: u64) -> String {
    let _ = n;
    String::new()
}

/// Free bytes from `df -Pk <path>` output (the last line's "Available" column, in KiB).
pub fn parse_df_available(output: &str) -> Option<u64> {
    let _ = output;
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn confirmation_counts_follow_the_checkbox() {
        let c = ClearCounts {
            keeping: 7,
            everything: 12,
        };
        assert_eq!(c.for_choice(true), 7);
        assert_eq!(c.for_choice(false), 12);
    }

    #[test]
    fn a_cleanup_needs_the_dialog_and_shows_its_count() {
        let mut f = CleanupFlow::default();
        assert_eq!(f.confirm(), None, "nothing is deleted without the dialog");
        f.ask(CleanupAction::OlderThan(30), 42);
        assert!(f.is_open());
        assert_eq!(f.prompt(), Some((Msg::SettingsOlderConfirm, 42)));
        f.cancel();
        assert!(!f.is_open());
        assert_eq!(f.confirm(), None);
        f.ask(CleanupAction::Orphans, 3);
        assert_eq!(f.prompt(), Some((Msg::SettingsOrphansConfirm, 3)));
        assert_eq!(f.confirm(), Some(CleanupAction::Orphans));
        // Closed after the confirmation: a second one does nothing.
        assert_eq!(f.confirm(), None);
        assert_eq!(f.prompt(), None);
        f.ask(CleanupAction::Compact, 1_048_576);
        assert_eq!(f.prompt(), Some((Msg::SettingsCompactConfirm, 1_048_576)));
        assert_eq!(f.confirm(), Some(CleanupAction::Compact));
        // Asking again replaces the previous question.
        f.ask(CleanupAction::Orphans, 1);
        f.ask(CleanupAction::OlderThan(7), 9);
        assert_eq!(f.confirm(), Some(CleanupAction::OlderThan(7)));
    }

    #[test]
    fn days_accept_only_whole_numbers_in_range() {
        assert_eq!(parse_days("1"), Some(1));
        assert_eq!(parse_days(" 30 "), Some(30));
        assert_eq!(parse_days("3650"), Some(3650));
        for bad in [
            "",
            " ",
            "0",
            "3651",
            "-5",
            "+5",
            "1.5",
            "1 0",
            "abc",
            "99999999999",
        ] {
            assert_eq!(parse_days(bad), None, "{bad:?}");
        }
    }

    #[test]
    fn bytes_are_formatted_for_people() {
        assert_eq!(format_bytes(0), "0 B");
        assert_eq!(format_bytes(512), "512 B");
        assert_eq!(format_bytes(1024), "1.0 KB");
        assert_eq!(format_bytes(1536), "1.5 KB");
        assert_eq!(format_bytes(2 * 1024 * 1024), "2.0 MB");
        assert_eq!(format_bytes(5 * 1024 * 1024 * 1024), "5.0 GB");
        assert_eq!(format_bytes(u64::MAX).ends_with("GB"), true);
    }

    #[test]
    fn df_output_gives_the_available_space() {
        let out = "Filesystem 1024-blocks Used Available Capacity Mounted on\n\
                   /dev/disk1s1 1000 400 600 40% /\n";
        assert_eq!(parse_df_available(out), Some(600 * 1024));
        assert_eq!(parse_df_available(""), None);
        assert_eq!(parse_df_available("Filesystem only\n"), None);
        assert_eq!(parse_df_available("a b c notanumber d\n"), None);
    }
}
