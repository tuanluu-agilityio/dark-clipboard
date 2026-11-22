//! Wrapper that keeps sensitive values out of `Debug` output (GC-06).

/// Holds a value whose `Debug` output is always `<redacted>`.
#[derive(Clone, PartialEq, Eq, Default)]
pub struct Redacted<T>(pub T);

impl<T> std::fmt::Debug for Redacted<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("<redacted>")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redacted_debug_hides_value() {
        let r = Redacted("TOPSECRET".to_string());
        assert_eq!(format!("{r:?}"), "<redacted>");
        assert_eq!(format!("{:?}", Some(Redacted(1))), "Some(<redacted>)");
    }
}
