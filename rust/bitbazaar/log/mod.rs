#[cfg(test)]
mod diff_file_log;
mod global_log;
mod macros;
#[cfg(any(feature = "opentelemetry-grpc", feature = "opentelemetry-http"))]
mod ot_tracing_bridge;

#[cfg(all(
    feature = "system",
    any(feature = "opentelemetry-grpc", feature = "opentelemetry-http")
))]
mod system_and_process_metrics;
pub use global_log::{global_fns::*, GlobalLog, GlobalLogBuilder};
#[cfg(all(
    feature = "system",
    any(feature = "opentelemetry-grpc", feature = "opentelemetry-http")
))]
pub use system_and_process_metrics::*;

#[cfg(any(feature = "opentelemetry-grpc", feature = "opentelemetry-http"))]
/// Opentelemetry types that might be needed downstream.
/// The aim is to avoid the user having to depend on opentelemetry crates directly.
pub mod otlp {
    pub use opentelemetry::{Key, KeyValue, StringValue, Value};

    /// Otlp metric types.
    pub mod metrics {
        pub use opentelemetry::metrics::{
            Counter, Histogram, ObservableCounter, ObservableGauge, ObservableUpDownCounter,
            SyncCounter, SyncHistogram, SyncUpDownCounter, Unit, UpDownCounter,
        };
    }
}

#[cfg(test)]
mod tests {
    use std::{
        collections::{HashMap, HashSet},
        sync::atomic::AtomicU32,
    };

    use once_cell::sync::Lazy;
    use parking_lot::Mutex;
    use rstest::*;
    use tempfile::tempdir;
    use tracing::{debug, error, info, warn, Level};

    use super::*;
    use crate::errors::prelude::*;

    fn log_all() {
        debug!("DLOG");
        info!("ILOG");
        warn!("WLOG");
        error!("ELOG");
    }

    fn into_vec(logs: &Mutex<Vec<String>>) -> Vec<String> {
        logs.lock().clone()
    }

    #[rstest]
    fn test_log_formatting_basic(
        // All combinations of:
        #[values(true, false)] include_timestamp: bool,
        #[values(true, false)] include_loc: bool,
    ) -> RResult<(), AnyErr> {
        static LOGS: Lazy<Mutex<Vec<String>>> = Lazy::new(Mutex::default);
        {
            // Fn repeat usage so static needs clearing each time:
            LOGS.lock().clear();
        }

        let log = GlobalLog::builder()
            .custom(true, include_loc, false, include_timestamp, |log| {
                LOGS.lock()
                    .push(String::from_utf8_lossy(log).trim().to_string());
            })
            .level_from(Level::DEBUG)?
            .build()?;
        log.with_tmp_global(log_all)?;

        let chk_log = |lvl: Level, in_log: &str, out_log: &str| -> RResult<(), AnyErr> {
            // Lvl should always be included:
            assert!(
                out_log.contains(&lvl.to_string().to_uppercase()),
                "{}",
                out_log
            );
            if include_loc {
                assert!(out_log.contains("mod.rs"), "{}", out_log);
            }
            if include_timestamp {
                // Confirm matches regex HH:MM:SS.mmm:
                assert!(regex::Regex::new(r"\d{2}:\d{2}:\d{2}.\d{3}")
                    .change_context(AnyErr)?
                    .is_match(out_log));
            }
            // Should include the actual log:
            assert!(
                out_log.contains(in_log),
                "Expected to contain: '{}', out: '{}'",
                in_log,
                out_log
            );

            Ok(())
        };

        let out = into_vec(&LOGS);
        assert_eq!(out.len(), 4, "{:?}", out);
        chk_log(Level::DEBUG, "DLOG", &out[0])?;
        chk_log(Level::INFO, "ILOG", &out[1])?;
        chk_log(Level::WARN, "WLOG", &out[2])?;
        chk_log(Level::ERROR, "ELOG", &out[3])?;

        Ok(())
    }

    #[cfg(feature = "log-filter")]
    #[rstest]
    // No matchers on either targets, so picked up by both targets:
    #[case::both(None, vec!["with_matcher DEBUG LOG1", "no_matcher DEBUG LOG1", "with_matcher DEBUG LOG2", "no_matcher DEBUG LOG2"])]
    // Matcher matches on first target, so no matcher target should ignore that log, i.e. one each:
    #[case::one_each(Some(regex::Regex::new(
        if cfg!(windows) {r"log\\mod.rs"} else {r"log/mod.rs"}
    ).unwrap()), vec!["with_matcher DEBUG LOG1", "no_matcher DEBUG LOG2"])]
    // Matcher failed, so both should be picked up by the one with no matcher:
    #[case::no_match(Some(regex::Regex::new(r"kdkfjdf").unwrap()), vec!["no_matcher DEBUG LOG1", "no_matcher DEBUG LOG2"])]
    fn test_log_matchers(
        #[case] loc_matcher: Option<regex::Regex>,
        #[case] expected_logs: Vec<&str>,
    ) -> RResult<(), AnyErr> {
        static LOGS: Lazy<Mutex<Vec<String>>> = Lazy::new(Mutex::default);
        {
            // Fn repeat usage so static needs clearing each time:
            LOGS.lock().clear();
        }

        // Add the first custom logger with a matcher:
        let mut builder = GlobalLog::builder()
            .custom(false, false, false, false, |log| {
                LOGS.lock().push(format!(
                    "with_matcher {}",
                    String::from_utf8_lossy(log).trim()
                ));
            })
            .level_from(Level::DEBUG)?;

        if let Some(loc_matcher) = loc_matcher {
            builder = builder.loc_matcher(loc_matcher)?;
        }

        // Add the second with no matcher and build:
        let log = builder
            .custom(false, false, false, false, |log| {
                LOGS.lock().push(format!(
                    "no_matcher {}",
                    String::from_utf8_lossy(log).trim()
                ));
            })
            .level_from(Level::DEBUG)?
            .build()?;

        log.with_tmp_global(|| {
            debug!("LOG1");
            diff_file_log::diff_file_log("LOG2");
        })?;

        assert_eq!(into_vec(&LOGS), expected_logs);

        Ok(())
    }

    #[rstest]
    #[case(Level::DEBUG, vec!["DLOG", "ILOG", "WLOG", "ELOG"])]
    #[case(Level::INFO, vec!["ILOG", "WLOG", "ELOG"])]
    #[case(Level::WARN, vec!["WLOG", "ELOG"])]
    #[case(Level::ERROR, vec!["ELOG"])]
    fn test_log_filtering(
        #[case] level_from: Level,
        #[case] expected_found: Vec<&str>,
    ) -> RResult<(), AnyErr> {
        static LOGS: Lazy<Mutex<Vec<String>>> = Lazy::new(Mutex::default);
        {
            // Fn repeat usage so static needs clearing each time:
            LOGS.lock().clear();
        }

        let log = GlobalLog::builder()
            .custom(false, false, false, false, |log| {
                LOGS.lock()
                    .push(String::from_utf8_lossy(log).trim().to_string());
            })
            .level_from(level_from)?
            .build()?;

        log.with_tmp_global(log_all)?;

        let out = into_vec(&LOGS);
        assert_eq!(out.len(), expected_found.len());
        let mut remaining = HashSet::<&str>::from_iter(expected_found.iter().cloned());
        for log in out.iter() {
            let mut found = false;
            for matcher in remaining.clone().iter() {
                if log.contains(matcher) {
                    remaining.remove(matcher);
                    found = true;
                    break;
                }
            }
            assert!(
                found,
                "Unexpected log: {}. Level from: {:?}, all LOGS: {:?}",
                log, level_from, out
            );
        }
        assert_eq!(remaining.len(), 0);

        Ok(())
    }

    /// - Confirm record_exception() and is recorded as an exception event on active span.
    /// - Confirm panic() is auto recorded as an exception event on active span.
    /// - Confirm both are recognised internally as exception events and use a custom formatter to give nice error messages.
    #[rstest]
    fn test_exception_recording() -> RResult<(), AnyErr> {
        static LOGS: Lazy<Mutex<Vec<String>>> = Lazy::new(Mutex::default);
        {
            // Fn repeat usage so static needs clearing each time:
            LOGS.lock().clear();
        }

        let log = GlobalLog::builder()
            .custom(false, false, false, false, |log| {
                LOGS.lock()
                    .push(String::from_utf8_lossy(log).trim().to_string());
            })
            .build()?;

        // Programmatically keeping line in check, atomic needed due to catch_unwind:
        let line_preceding_panic = AtomicU32::new(0);
        log.with_tmp_global(|| {
            // Manual record:
            record_exception("test_exc", "test_stack\nfoodle\nwoodle");

            // Panics should be auto recorded:
            let _ = std::panic::catch_unwind(|| {
                line_preceding_panic.store(line!(), std::sync::atomic::Ordering::Relaxed);
                panic!("test_panic");
            });
        })?;

        let out = into_vec(&LOGS);
        let exp_panic_loc = [
            "bitbazaar".to_string(),
            "log".to_string(),
            format!(
                "mod.rs:{}:17",
                line_preceding_panic.load(std::sync::atomic::Ordering::Relaxed) + 1
            ),
        ]
        .join(if cfg!(windows) { "\\" } else { "/" });

        assert_eq!(out.len(), 2, "{:?}", out);
        assert!(out[0].contains("test_stack"), "{:?}", out[0]);
        assert!(out[0].contains("Err: test_exc"), "{:?}", out[0]);
        assert!(out[1].contains(&exp_panic_loc), "{:?}", out[1]);
        assert!(out[1].contains("Panic: test_panic"), "{:?}", out[1]);

        Ok(())
    }

    #[rstest]
    fn test_log_to_file() -> RResult<(), AnyErr> {
        let temp_dir = tempdir().change_context(AnyErr)?;

        let log = GlobalLog::builder()
            .file("foo.log", temp_dir.path())
            .level_from(Level::DEBUG)?
            .build()?;

        log.with_tmp_global(log_all)?;

        // Sleep for 50ms to make sure everything's been flushed to the file: (happens in separate thread)
        std::thread::sleep(std::time::Duration::from_millis(50));

        let files: HashMap<String, String> = temp_dir
            .path()
            .read_dir()
            .change_context(AnyErr)?
            .map(|entry| {
                let entry = entry.unwrap();
                let path = entry.path();
                let contents = std::fs::read_to_string(&path).unwrap();
                (
                    path.file_name().unwrap().to_str().unwrap().to_string(),
                    contents,
                )
            })
            .collect();

        // Should only be one file:
        assert_eq!(files.len(), 1);
        let name = files.keys().next().unwrap();
        let contents = files.get(name).unwrap();

        // Check name matches "foo.log%Y-%m-%d" with regex:
        let re = regex::Regex::new(r"^foo.log.\d{4}-\d{2}-\d{2}$").change_context(AnyErr)?;
        assert!(re.is_match(name), "{}", name);

        let out = contents.lines().collect::<Vec<_>>();
        assert_eq!(out.len(), 4, "{}", contents);
        assert!(out[0].contains("DLOG"), "{}", out[0]);
        assert!(out[1].contains("ILOG"), "{}", out[1]);
        assert!(out[2].contains("WLOG"), "{}", out[2]);
        assert!(out[3].contains("ELOG"), "{}", out[3]);

        Ok(())
    }

    #[cfg(feature = "opentelemetry-grpc")]
    #[rstest]
    #[tokio::test(flavor = "multi_thread")]
    async fn test_otlp_grpc() -> RResult<(), AnyErr> {
        _inner_test_opentelemetry(GlobalLog::builder().otlp_grpc(4317, "rust-test", "0.1.0")).await
    }

    #[cfg(feature = "opentelemetry-http")]
    #[rstest]
    #[tokio::test(flavor = "multi_thread")]
    async fn test_otlp_http() -> RResult<(), AnyErr> {
        _inner_test_opentelemetry(GlobalLog::builder().otlp_http(
            "http://localhost:4318",
            "rust-test",
            "0.1.0",
        ))
        .await
    }

    async fn _inner_test_opentelemetry(builder: GlobalLogBuilder) -> RResult<(), AnyErr> {
        use std::path::PathBuf;

        use crate::misc::in_ci;

        // Collector won't be running ci:
        if in_ci() {
            return Ok(());
        }

        let logpath = PathBuf::from("../logs/otlp_telemetry_out.log");
        let mut cur_str_len = 0;
        if logpath.exists() {
            cur_str_len = std::fs::read_to_string(&logpath)
                .change_context(AnyErr)?
                .len();
        }

        let log = builder.level_from(Level::DEBUG)?.build()?;

        log.with_tmp_global(|| {
            debug!("BEFORE");
            example_spanned_fn();
            warn!("AFTER");

            // Use a metric:
            let meter = log.meter("my_meter").unwrap();
            let counter = meter.u64_counter("my_counter").init();
            counter.add(1, &[]);
        })?;

        // Make sure everything's been sent:
        log.flush()?;

        // Wait for a second, as that's how often the collector writes to the debug file:
        tokio::time::sleep(tokio::time::Duration::from_millis(1000)).await;

        // Logs should now exist in the collector, which is configured to write them to ./logs/otlp.log for testing:
        // Read logpath from filestart:
        let full = std::fs::read_to_string(&logpath).change_context(AnyErr)?;
        let contents = &full[cur_str_len..];

        let mut metrics: Vec<GenMetric> = vec![];
        let mut spans: Vec<GenSpan> = vec![];
        let mut logs: Vec<GenLog> = vec![];
        for line in contents.lines() {
            let value: serde_json::Value = serde_json::from_str(line)
                .change_context(AnyErr)
                .attach_printable_lazy(|| {
                    format!("Couldn't decode line as json. Line: '{}'", line)
                })?;

            if line.contains("resourceMetrics") {
                for resource in value
                    .as_object()
                    .unwrap()
                    .get("resourceMetrics")
                    .unwrap()
                    .as_array()
                    .unwrap()
                {
                    for scope in resource
                        .as_object()
                        .unwrap()
                        .get("scopeMetrics")
                        .unwrap()
                        .as_array()
                        .unwrap()
                    {
                        for metric in scope
                            .as_object()
                            .unwrap()
                            .get("metrics")
                            .unwrap()
                            .as_array()
                            .unwrap()
                        {
                            metrics.push(GenMetric {
                                name: metric
                                    .as_object()
                                    .unwrap()
                                    .get("name")
                                    .unwrap()
                                    .as_str()
                                    .unwrap()
                                    .into(),
                            });
                        }
                    }
                }
            } else if line.contains("resourceSpans") {
                for resource in value
                    .as_object()
                    .unwrap()
                    .get("resourceSpans")
                    .unwrap()
                    .as_array()
                    .unwrap()
                {
                    for scope in resource
                        .as_object()
                        .unwrap()
                        .get("scopeSpans")
                        .unwrap()
                        .as_array()
                        .unwrap()
                    {
                        for span in scope
                            .as_object()
                            .unwrap()
                            .get("spans")
                            .unwrap()
                            .as_array()
                            .unwrap()
                        {
                            spans.push(GenSpan {
                                sid: span.get("spanId").unwrap().as_str().unwrap().into(),
                            });
                        }
                    }
                }
            } else if line.contains("resourceLogs") {
                for resource in value
                    .as_object()
                    .unwrap()
                    .get("resourceLogs")
                    .unwrap()
                    .as_array()
                    .unwrap()
                {
                    for scope in resource
                        .as_object()
                        .unwrap()
                        .get("scopeLogs")
                        .unwrap()
                        .as_array()
                        .unwrap()
                    {
                        for log in scope
                            .as_object()
                            .unwrap()
                            .get("logRecords")
                            .unwrap()
                            .as_array()
                            .unwrap()
                        {
                            let log = log.as_object().unwrap();
                            logs.push(GenLog {
                                sid: log.get("spanId").unwrap().as_str().unwrap().into(),
                                body: otlp_value_to_string(log.get("body").unwrap()),
                                attrs: log
                                    .get("attributes")
                                    .unwrap()
                                    .as_array()
                                    .unwrap()
                                    .iter()
                                    .map(|attr| {
                                        let attr = attr.as_object().unwrap();
                                        (
                                            attr.get("key").unwrap().as_str().unwrap().into(),
                                            otlp_value_to_string(attr.get("value").unwrap()),
                                        )
                                    })
                                    .collect(),
                            });
                        }
                    }
                }
            } else {
                return Err(anyerr!("Unexpected line: {}", line));
            }
        }

        assert_eq!(spans.len(), 1);
        assert_eq!(logs.len(), 3);

        // Span should be assigned to nested log only, logs should be in order
        assert_eq!(logs[0].body, "BEFORE");
        assert_eq!(logs[0].sid, "");
        assert_eq!(logs[1].body, "NESTED");
        assert_eq!(logs[1].sid, spans[0].sid);
        assert_eq!(logs[2].body, "AFTER");
        assert_eq!(logs[2].sid, "");

        // Metadata should be correctly attached:
        assert_eq!(
            logs[2].attrs.get("code.namespace").unwrap(),
            "bitbazaar::log::tests"
        );

        // Metric should show up:
        assert_eq!(metrics[0].name, "my_counter");

        Ok(())
    }

    #[tracing::instrument]
    fn example_spanned_fn() {
        error!("NESTED");
    }

    struct GenMetric {
        name: String,
    }

    struct GenSpan {
        sid: String,
    }

    struct GenLog {
        sid: String,
        body: String,
        attrs: HashMap<String, String>,
    }

    fn otlp_value_to_string(value: &serde_json::Value) -> String {
        let val = value.as_object().unwrap();
        if val.contains_key("stringValue") {
            val.get("stringValue").unwrap().as_str().unwrap().into()
        } else if val.contains_key("intValue") {
            val.get("intValue").unwrap().as_str().unwrap().into()
        } else {
            panic!("Unknown value: {:?}", val)
        }
    }
}
