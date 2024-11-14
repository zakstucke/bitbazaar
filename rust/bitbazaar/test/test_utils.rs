/// Asserts that the given [`chrono::TimeDelta`] is in the given range.
#[macro_export]
macro_rules! assert_td_in_range {
    ($td:expr, $range:expr) => {
        assert!(
            $td >= $range.start && $td <= $range.end,
            "Expected '{}' to be in range '{}' - '{}'.",
            $crate::chrono::chrono_format_td($td, true),
            $crate::chrono::chrono_format_td($range.start, true),
            $crate::chrono::chrono_format_td($range.end, true),
        );
    };
}

// Re-export:
pub use crate::assert_td_in_range;
