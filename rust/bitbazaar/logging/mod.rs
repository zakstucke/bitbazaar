mod clap_log_level_args;
mod macros;
mod setup_logger;

pub use clap_log_level_args::ClapLogLevelArgs;
pub use setup_logger::{setup_logger, LogTarget, LogTargetVariant};

#[cfg(test)]
mod tests {
    use std::{
        collections::{HashMap, HashSet},
        sync::Arc,
    };

    use colored::Colorize;
    use log::{Level, LevelFilter};
    use parking_lot::Mutex;
    use rstest::*;
    use tempfile::tempdir;

    use super::*;

    // Can't use global apply() because want to run lots of different configurations:
    fn get_logger(targets: Vec<LogTarget>) -> Box<dyn log::Log> {
        let (_, logger) = setup_logger(targets).unwrap().into_log();
        logger
    }

    /// Get a logger that simulates the stdout one, that also returns a fn to get all the logs written since the logger was created.
    fn get_stdout_logger(
        level_filter: log::LevelFilter,
        include_ts_till: Option<log::LevelFilter>,
        include_location_till: Option<log::LevelFilter>,
    ) -> (impl Fn() -> Vec<String>, Box<dyn log::Log>) {
        let log_store = Arc::new(Mutex::new(vec![]));
        let cloned = log_store.clone();
        let logger = get_logger(vec![LogTarget {
            msg_prefix: None,
            level_filter,
            include_ts_till,
            include_location_till,
            variant: LogTargetVariant::Custom {
                include_color: true,
                output: fern::Output::call(move |record| {
                    cloned.lock().push(format!("{}", record.args()))
                }),
            },
        }]);
        let get_logs = move || log_store.lock().clone();
        (get_logs, logger)
    }

    /// Can't use the normal macros as they need a global logger, which we can't enable as inside tests:
    fn log(logger: &dyn log::Log, level: Level, msg: &str) {
        logger.log(
            &log::Record::builder()
                .args(format_args!("{}", msg))
                .level(level)
                .target("bitbazaar::logging::tests")
                .module_path_static(Some("bitbazaar::logging::tests"))
                .file_static(Some("logging.rs"))
                .line(Some(123))
                .build(),
        );
    }

    fn log_all(logger: &dyn log::Log) {
        log(&logger, Level::Debug, "DEBUG LOG");
        log(&logger, Level::Info, "INFO LOG");
        log(&logger, Level::Warn, "WARN LOG");
        log(&logger, Level::Error, "ERROR LOG");
    }

    #[rstest]
    fn test_log_formatting_basic() {
        let (get_logs, logger) = get_stdout_logger(LevelFilter::Debug, None, None);
        log_all(&logger);
        assert_eq!(
            get_logs(),
            vec![
                format!("{}{}", "debug: ".cyan().bold(), "DEBUG LOG"),
                format!("{}{}", "info: ".bold(), "INFO LOG"),
                format!("{}{}", "warn: ".yellow().bold(), "WARN LOG"),
                format!("{}{}", "error: ".red().bold(), "ERROR LOG")
            ]
        );
    }

    #[rstest]
    fn test_log_formatting_extras() {
        // Should include timestamp for debug and info, location for debug, ingo and warn:
        let (get_logs, logger) = get_stdout_logger(
            LevelFilter::Debug,
            Some(LevelFilter::Info),
            Some(LevelFilter::Warn),
        );
        log_all(&logger);
        let logs = get_logs();
        assert_eq!(logs.len(), 4);

        let ts_and_loc_re = regex::Regex::new(r"\[\d{2}:\d{2}:\d{2}\]\[logging.rs:123\]").unwrap();

        // Debug should have both:
        assert!(ts_and_loc_re.is_match(&logs[0]), "{}", logs[0]);
        assert!(logs[0].ends_with("DEBUG LOG"), "{}", logs[0]);
        // Info should have both:
        assert!(ts_and_loc_re.is_match(&logs[1]), "{}", logs[1]);
        assert!(logs[1].ends_with("INFO LOG"), "{}", logs[1]);
        // Should only include loc for warn:
        assert_eq!(
            logs[2],
            format!(
                "{}{}",
                "[logging.rs:123] warn: ".yellow().bold(),
                "WARN LOG"
            )
        );
        // Shouldn't include anything for err:
        assert_eq!(
            logs[3],
            format!("{}{}", "error: ".red().bold(), "ERROR LOG")
        );
    }

    #[rstest]
    #[case(LevelFilter::Debug, vec!["DEBUG", "INFO", "WARN", "ERROR"])]
    #[case(LevelFilter::Info, vec!["INFO", "WARN", "ERROR"])]
    #[case(LevelFilter::Warn, vec!["WARN", "ERROR"])]
    #[case(LevelFilter::Error, vec!["ERROR"])]
    #[case(LevelFilter::Off, vec![])]
    fn test_log_filtering(#[case] level_filter: LevelFilter, #[case] expected_found: Vec<&str>) {
        let (get_logs, logger) = get_stdout_logger(level_filter, None, None);
        log_all(&logger);

        let logs = get_logs();
        let mut remaining = HashSet::<&str>::from_iter(expected_found.iter().cloned());
        for log in logs {
            let mut found = false;
            for matcher in remaining.clone().iter() {
                if log.contains(matcher) {
                    remaining.remove(matcher);
                    found = true;
                    break;
                }
            }
            assert!(found, "Unexpected log: {}", log);
        }
        assert_eq!(remaining.len(), 0);
        logger.flush();
    }

    #[rstest]
    fn test_log_to_file() {
        let temp_dir = tempdir().unwrap();
        let logger = get_logger(vec![LogTarget {
            msg_prefix: None,
            level_filter: LevelFilter::Debug,
            include_ts_till: None,
            include_location_till: None,
            variant: LogTargetVariant::File {
                dir: temp_dir.path().to_path_buf(),
                file_prefix: "foo_".to_string(),
            },
        }]);
        log_all(&logger);

        let files: HashMap<String, String> = temp_dir
            .path()
            .read_dir()
            .unwrap()
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

        // Check name matches "foo_%Y-%m-%d.log" with regex:
        let re = regex::Regex::new(r"^foo_\d{4}-\d{2}-\d{2}.log$").unwrap();
        assert!(re.is_match(name));

        let logs = contents.lines().collect::<Vec<_>>();
        assert_eq!(logs.len(), 4);
        assert!(logs[0].ends_with("DEBUG LOG"), "{}", logs[0]);
        assert!(logs[1].ends_with("INFO LOG"), "{}", logs[1]);
        assert!(logs[2].ends_with("WARN LOG"), "{}", logs[2]);
        assert!(logs[3].ends_with("ERROR LOG"), "{}", logs[3]);
    }
}
