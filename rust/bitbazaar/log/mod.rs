#[cfg(test)]
mod diff_file_log;
mod global_log;
mod macros;
#[cfg(any(feature = "opentelemetry-grpc", feature = "opentelemetry-http"))]
mod ot_tracing_bridge;

// Can't run a collector on wasm:
#[cfg(all(not(target_arch = "wasm32"), feature = "collector"))]
mod standalone_collector;
#[cfg(all(not(target_arch = "wasm32"), feature = "collector"))]
pub use standalone_collector::*;

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
        sync::{atomic::AtomicU32, Arc, LazyLock},
    };

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
        static LOGS: LazyLock<Mutex<Vec<String>>> = LazyLock::new(Mutex::default);
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
        static LOGS: LazyLock<Mutex<Vec<String>>> = LazyLock::new(Mutex::default);
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
        static LOGS: LazyLock<Mutex<Vec<String>>> = LazyLock::new(Mutex::default);
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
        static LOGS: LazyLock<Mutex<Vec<String>>> = LazyLock::new(Mutex::default);
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
        let (_collector, port, records) = _build_collector(include_str!(
            "./test_assets/collector_config_example_grpc.yaml"
        ))
        .await
        .change_context(AnyErr)?;
        _inner_test_opentelemetry(
            GlobalLog::builder().otlp_grpc(port, "rust-test", "0.1.0"),
            &records,
        )
        .await
    }

    #[cfg(feature = "opentelemetry-http")]
    #[rstest]
    #[tokio::test(flavor = "multi_thread")]
    async fn test_otlp_http() -> RResult<(), AnyErr> {
        let (_collector, port, records) = _build_collector(include_str!(
            "./test_assets/collector_config_example_http.yaml"
        ))
        .await
        .change_context(AnyErr)?;
        _inner_test_opentelemetry(
            GlobalLog::builder().otlp_http(
                &format!("http://localhost:{}", port),
                "rust-test",
                "0.1.0",
            ),
            &records,
        )
        .await
    }

    async fn _build_collector(
        config: &str,
    ) -> RResult<(CollectorStandalone, u16, Arc<Mutex<Vec<String>>>), AnyErr> {
        use std::sync::Arc;

        let records = Arc::new(Mutex::new(vec![]));

        let port = portpicker::pick_unused_port().unwrap();
        let collector = CollectorStandalone::new(
            &config.replace("$PORT", &port.to_string()),
            {
                let records = records.clone();
                move |stdout| {
                    println!("{}", stdout);
                    records.lock().push(stdout);
                    async {}
                }
            },
            {
                let records = records.clone();
                move |stderr| {
                    println!("{}", stderr);
                    records.lock().push(stderr);
                    async {}
                }
            },
        )
        .await
        .change_context(AnyErr)?;

        Ok((collector, port, records))
    }

    async fn _inner_test_opentelemetry(
        builder: GlobalLogBuilder,
        records: &Mutex<Vec<String>>,
    ) -> RResult<(), AnyErr> {
        let log = builder.level_from(Level::DEBUG)?.build()?;

        #[tracing::instrument]
        fn example_spanned_fn() {
            error!("NESTED");
        }

        // Sleeping after each, to try and ensure the correct debug output:
        log.with_tmp_global(|| {
            // On windows this needs to be really long to get static record ordering for testing:
            let delay = if cfg!(windows) { 100 } else { 10 };

            debug!("BEFORE");
            std::thread::sleep(std::time::Duration::from_millis(delay));
            example_spanned_fn();
            std::thread::sleep(std::time::Duration::from_millis(delay));
            warn!("AFTER");

            // Use a metric:
            std::thread::sleep(std::time::Duration::from_millis(delay));
            let meter = log.meter("my_meter").unwrap();
            let counter = meter.u64_counter("my_counter").init();
            counter.add(1, &[]);
            std::thread::sleep(std::time::Duration::from_millis(delay));
        })?;

        // Make sure everything's been sent:
        log.flush()?;

        // Logs should now exist:
        let contents = records.lock();

        // Until below issue is resolved, debug logs come in an awful format, so very hacky parsing here to make sure things more or less work:
        // https://github.com/open-telemetry/opentelemetry-collector/issues/9149
        let breakpoints = [
            "LogsExporter",
            "ResourceLog",
            "TracesExporter",
            "MetricsExporter",
        ];
        let mut items = vec![];
        let mut active = HashMap::new();
        for line in contents.iter() {
            let trimmed = line.trim().replace("-> ", "").replace("\t", ":");
            if !active.is_empty() && breakpoints.iter().any(|b| trimmed.contains(b)) {
                items.push(std::mem::take(&mut active));
            }
            if trimmed.contains(":") {
                let mut parts = trimmed.splitn(2, ":");
                let key = parts.next().unwrap().trim();
                let value = parts.next().unwrap().trim();
                active.insert(key.to_string(), value.to_string());
            } else {
                active.insert(trimmed, "".to_string());
            }
        }
        items.push(active);
        // Getting rid of junk:
        items.retain(|item| {
            !(item.is_empty()
                || (item.len() == 1
                    && (item.contains_key("LogsExporter")
                        || item.contains_key("MetricsExporter")
                        || item.contains_key("TracesExporter"))))
        });

        // Expecting 5 items, each log and then the span declaration then the metric:
        assert_eq!(items.len(), 5, "{:#?}", items);

        println!("{:#?}", items);

        // First should be the BEFORE log:
        assert_eq!(items[0].get("Body").unwrap(), "Str(BEFORE)");
        assert_eq!(items[0].get("SeverityText").unwrap(), "DEBUG");
        assert_eq!(items[0].get("Span ID").unwrap(), "");
        // Metadata should be correctly attached:
        assert_eq!(
            items[0].get("code.namespace").unwrap(),
            "Str(bitbazaar::log::tests)"
        );

        // Second should be the NESTED log inside a span:
        assert_eq!(items[1].get("Body").unwrap(), "Str(NESTED)");
        assert_eq!(items[1].get("SeverityText").unwrap(), "ERROR");
        let sid = items[1].get("Span ID").unwrap();
        assert!(!sid.is_empty(), "{}", sid);

        // Third should be the AFTER log:
        assert_eq!(items[2].get("Body").unwrap(), "Str(AFTER)");
        assert_eq!(items[2].get("SeverityText").unwrap(), "WARN");
        assert_eq!(items[2].get("Span ID").unwrap(), "");

        // Fourth should be the span definition:
        // Should be the same span id as the log:
        assert_eq!(items[3].get("ID").unwrap(), sid);

        // Fifth should be the metric:
        assert_eq!(items[4].get("Name").unwrap(), "my_counter");

        Ok(())
    }
}
