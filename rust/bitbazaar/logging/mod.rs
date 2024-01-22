mod clap_log_level_args;
mod create_subscriber;
#[cfg(test)]
mod diff_file_log;
mod macros;

pub use clap_log_level_args::ClapLogLevelArgs;
pub use create_subscriber::{
    create_subscriber, SubCustomWriter, SubLayer, SubLayerFilter, SubLayerVariant,
};

#[cfg(test)]
mod tests {
    use std::collections::{HashMap, HashSet};

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
    #[serial_test::serial] // Uses static, so parameterized versions can't run in parallel.
    fn test_log_formatting_basic(
        // All combinations of:
        #[values(true, false)] include_lvl: bool,
        #[values(true, false)] include_timestamp: bool,
        #[values(true, false)] include_loc: bool,
    ) -> Result<(), AnyErr> {
        static LOGS: Lazy<Mutex<Vec<String>>> = Lazy::new(Mutex::default);
        {
            // Fn repeat usage so static needs clearing each time:
            LOGS.lock().clear();
        }

        let sub = create_subscriber(vec![SubLayer {
            variant: SubLayerVariant::Custom {
                include_color: false,
                writer: SubCustomWriter {
                    write: |log| {
                        LOGS.lock()
                            .push(String::from_utf8_lossy(log).trim().to_string());
                    },
                },
            },
            include_lvl,
            include_timestamp,
            include_loc,
            filter: SubLayerFilter::Above(Level::DEBUG),
            ..Default::default()
        }])?;

        tracing::dispatcher::with_default(&sub.dispatch, || {
            log_all();
        });

        let chk_log = |lvl: Level, in_log: &str, out_log: &str| -> Result<(), AnyErr> {
            if include_lvl {
                assert!(
                    out_log.contains(&lvl.to_string().to_uppercase()),
                    "{}",
                    out_log
                );
            }
            if include_loc {
                assert!(out_log.contains("mod.rs"), "{}", out_log);
            }
            if include_timestamp {
                // Confirm matches regex HH:MM:SS.mmm:
                assert!(regex::Regex::new(r"\d{2}:\d{2}:\d{2}.\d{3}")
                    .change_context(AnyErr)?
                    .is_match(out_log));
            }
            // Should end with the actual log:
            assert!(out_log.ends_with(in_log), "{}", out_log);

            Ok(())
        };

        let out = into_vec(&LOGS);
        assert_eq!(out.len(), 4);
        chk_log(Level::DEBUG, "DLOG", &out[0])?;
        chk_log(Level::INFO, "ILOG", &out[1])?;
        chk_log(Level::WARN, "WLOG", &out[2])?;
        chk_log(Level::ERROR, "ELOG", &out[3])?;

        Ok(())
    }

    #[rstest]
    fn test_log_pretty() -> Result<(), AnyErr> {
        static LOGS: Lazy<Mutex<Vec<String>>> = Lazy::new(Mutex::default);

        let sub = create_subscriber(vec![SubLayer {
            variant: SubLayerVariant::Custom {
                include_color: false,
                writer: SubCustomWriter {
                    write: |log| {
                        LOGS.lock()
                            .push(String::from_utf8_lossy(log).trim().to_string());
                    },
                },
            },
            include_lvl: true,
            include_timestamp: false,
            include_loc: true,
            filter: SubLayerFilter::Above(Level::DEBUG),
            pretty: true,
            ..Default::default()
        }])?;

        tracing::dispatcher::with_default(&sub.dispatch, || {
            debug!("DLOG");
        });

        assert_eq!(
            into_vec(&LOGS),
            vec!["DEBUG  DLOG\n    at bitbazaar/logging/mod.rs:127"]
        );

        Ok(())
    }

    #[rstest]
    fn test_log_color() -> Result<(), AnyErr> {
        static LOGS: Lazy<Mutex<Vec<String>>> = Lazy::new(Mutex::default);

        let sub = create_subscriber(vec![SubLayer {
            variant: SubLayerVariant::Custom {
                include_color: true,
                writer: SubCustomWriter {
                    write: |log| {
                        LOGS.lock()
                            .push(String::from_utf8_lossy(log).trim().to_string());
                    },
                },
            },
            filter: SubLayerFilter::Above(Level::DEBUG),
            ..Default::default()
        }])?;

        tracing::dispatcher::with_default(&sub.dispatch, || {
            info!("ILOG");
        });

        assert_eq!(
            into_vec(&LOGS),
            // Leaving color coding to tracing crate now, just copied the output so know about any changes:
            vec![format!("\u{1b}[32m INFO\u{1b}[0m ILOG")]
        );

        Ok(())
    }

    #[rstest]
    // No matchers on either targets, so picked up by both targets:
    #[case(None, vec!["with_matcher DEBUG LOG1", "no_matcher DEBUG LOG1", "with_matcher DEBUG LOG2", "no_matcher DEBUG LOG2"])]
    // Matcher matches on first target, so no matcher target should ignore that log, i.e. one each:
    #[case(Some(regex::Regex::new(
        r"logging/mod.rs"
    ).unwrap()), vec!["with_matcher DEBUG LOG1", "no_matcher DEBUG LOG2"])]
    // Matcher failed, so both should be picked up by the one with no matcher:
    #[case(Some(regex::Regex::new(r"kdkfjdf").unwrap()), vec!["no_matcher DEBUG LOG1", "no_matcher DEBUG LOG2"])]
    #[serial_test::serial] // Uses static, so parameterized versions can't run in parallel.
    fn test_log_matchers(
        #[case] loc_matcher: Option<regex::Regex>,
        #[case] expected_logs: Vec<&str>,
    ) -> Result<(), AnyErr> {
        static LOGS: Lazy<Mutex<Vec<String>>> = Lazy::new(Mutex::default);
        {
            // Fn repeat usage so static needs clearing each time:
            LOGS.lock().clear();
        }

        let sub = create_subscriber(vec![
            SubLayer {
                variant: SubLayerVariant::Custom {
                    include_color: false,
                    writer: SubCustomWriter {
                        write: |log| {
                            LOGS.lock().push(format!(
                                "with_matcher {}",
                                String::from_utf8_lossy(log).trim()
                            ));
                        },
                    },
                },
                loc_matcher: loc_matcher.clone(),
                filter: SubLayerFilter::Above(Level::DEBUG),
                ..Default::default()
            },
            SubLayer {
                variant: SubLayerVariant::Custom {
                    include_color: false,
                    writer: SubCustomWriter {
                        write: |log| {
                            LOGS.lock().push(format!(
                                "no_matcher {}",
                                String::from_utf8_lossy(log).trim()
                            ));
                        },
                    },
                },
                filter: SubLayerFilter::Above(Level::DEBUG),
                ..Default::default()
            },
        ])?;

        tracing::dispatcher::with_default(&sub.dispatch, || {
            debug!("LOG1");
            diff_file_log::diff_file_log("LOG2");
        });

        assert_eq!(into_vec(&LOGS), expected_logs);

        Ok(())
    }

    #[rstest]
    #[case(SubLayerFilter::Above(Level::DEBUG), vec!["DLOG", "ILOG", "WLOG", "ELOG"])]
    #[case(SubLayerFilter::Above(Level::INFO), vec!["ILOG", "WLOG", "ELOG"])]
    #[case(SubLayerFilter::Above(Level::WARN), vec!["WLOG", "ELOG"])]
    #[case(SubLayerFilter::Above(Level::ERROR), vec!["ELOG"])]
    #[case(SubLayerFilter::Only(vec![Level::ERROR, Level::DEBUG]), vec!["DLOG", "ELOG"])]
    #[case(SubLayerFilter::Only(vec![Level::INFO, Level::WARN]), vec!["ILOG", "WLOG"])]
    #[serial_test::serial] // Uses static, so parameterized versions can't run in parallel.
    fn test_log_filtering(
        #[case] filter: SubLayerFilter,
        #[case] expected_found: Vec<&str>,
    ) -> Result<(), AnyErr> {
        static LOGS: Lazy<Mutex<Vec<String>>> = Lazy::new(Mutex::default);
        {
            // Fn repeat usage so static needs clearing each time:
            LOGS.lock().clear();
        }

        let sub = create_subscriber(vec![SubLayer {
            variant: SubLayerVariant::Custom {
                include_color: false,
                writer: SubCustomWriter {
                    write: |log| {
                        LOGS.lock()
                            .push(String::from_utf8_lossy(log).trim().to_string());
                    },
                },
            },
            filter: filter.clone(),
            ..Default::default()
        }])?;

        tracing::dispatcher::with_default(&sub.dispatch, || {
            log_all();
        });

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
                "Unexpected log: {}. Filter: {:?}, all LOGS: {:?}",
                log, filter, out
            );
        }
        assert_eq!(remaining.len(), 0);

        Ok(())
    }

    #[rstest]
    fn test_log_to_file() -> Result<(), AnyErr> {
        let temp_dir = tempdir().change_context(AnyErr)?;
        let sub = create_subscriber(vec![SubLayer {
            filter: SubLayerFilter::Above(Level::DEBUG),
            variant: SubLayerVariant::File {
                dir: temp_dir.path().to_path_buf(),
                file_prefix: "foo.log".to_string(),
            },
            ..Default::default()
        }])?;

        tracing::dispatcher::with_default(&sub.dispatch, || {
            log_all();
        });

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
        assert_eq!(out.len(), 4);
        assert!(out[0].ends_with("DLOG"), "{}", out[0]);
        assert!(out[1].ends_with("ILOG"), "{}", out[1]);
        assert!(out[2].ends_with("WLOG"), "{}", out[2]);
        assert!(out[3].ends_with("ELOG"), "{}", out[3]);

        Ok(())
    }

    #[cfg(feature = "opentelemetry")]
    #[rstest]
    #[tokio::test]
    async fn test_opentelemetry() -> Result<(), AnyErr> {
        // Not actually going to implement a fake collector on the other side, just check nothing errors:

        let sub = create_subscriber(vec![SubLayer {
            filter: SubLayerFilter::Above(Level::DEBUG),
            variant: SubLayerVariant::OpenTelemetry {
                endpoint: "http://localhost:4317".to_string(),
                headers: vec![],
            },
            ..Default::default()
        }])?;

        tracing::dispatcher::with_default(&sub.dispatch, || {
            log_all();
        });

        Ok(())
    }
}
