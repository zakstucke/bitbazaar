mod macros;
mod recorder;

/// Format a duration in a human readable format.
pub fn format_duration(duration: std::time::Duration) -> String {
    let time_unit;
    let time_value;

    let elapsed_s = duration.as_secs() as u128;
    if elapsed_s >= 1 {
        time_unit = "s";
        time_value = elapsed_s;
    } else {
        let elapsed_ms = duration.as_millis();
        if elapsed_ms >= 1 {
            time_unit = "ms";
            time_value = elapsed_ms;
        } else {
            let elapsed_us = duration.as_micros();
            if elapsed_us >= 1 {
                time_unit = "μs";
                time_value = elapsed_us;
            } else {
                let elapsed_ns = duration.as_nanos();
                time_unit = "ns";
                time_value = elapsed_ns;
            }
        }
    }

    format!("{}{}", time_value, time_unit)
}

pub use recorder::{TimeRecorder, GLOBAL_TIME_RECORDER};

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use rstest::*;

    use super::*;
    use crate::timeit;

    #[rstest]
    #[case(Duration::from_millis(1), "1ms")]
    #[case(Duration::from_micros(1), "1μs")]
    #[case(Duration::from_nanos(1), "1ns")]
    #[case(Duration::from_millis(999), "999ms")]
    #[case(Duration::from_millis(1000), "1s")]
    fn test_format_duration(#[case] input: Duration, #[case] expected: String) {
        assert_eq!(expected, format_duration(input));
    }

    #[rstest]
    fn test_recorder() {
        let recorder = TimeRecorder::new();

        recorder.timeit("test", || {
            std::thread::sleep(Duration::from_millis(1));
        });

        let elapsed = recorder.total_elapsed().unwrap();
        assert!(elapsed.as_millis() >= 1);
        assert!(elapsed.as_millis() < 3);

        let formatted = recorder.format_verbose().unwrap();
        assert!(formatted.contains("test"));
    }

    #[rstest]
    fn test_global() {
        timeit!("test", {
            std::thread::sleep(Duration::from_millis(1));
        });
        let elapsed = GLOBAL_TIME_RECORDER.total_elapsed().unwrap();
        assert!(elapsed.as_millis() >= 1);
        assert!(elapsed.as_millis() < 2);
    }
}
