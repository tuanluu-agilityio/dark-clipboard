//! Capture actor tuning (system-overview §2 rules 2–3, spike design consequences 2 and 4).

/// Timing of the capture actor. Defaults follow the Windows spike.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CaptureConfig {
    /// Quiet time after the last change signal of a burst before the clipboard is inspected.
    pub debounce_ms: u64,
    /// Extra attempts after a `Busy` clipboard (the first attempt is not a retry).
    pub open_retries: u32,
    /// Pause between attempts on a `Busy` clipboard.
    pub retry_delay_ms: u64,
}

impl Default for CaptureConfig {
    fn default() -> Self {
        Self {
            debounce_ms: 75,
            open_retries: 10,
            retry_delay_ms: 20,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_match_spike() {
        let c = CaptureConfig::default();
        assert_eq!(c.debounce_ms, 75);
        assert_eq!(c.open_retries, 10);
        assert_eq!(c.retry_delay_ms, 20);
    }
}
