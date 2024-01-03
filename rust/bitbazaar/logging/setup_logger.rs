use std::path::PathBuf;

use colored::Colorize;
use log::Level;

use crate::{err, errors::TracedErr};

/// A target for logging, e.g. stdout, a file, or a custom writer.
///
/// Example stdout logger:
/// ```
/// use bitbazaar::logging::{LogTarget, setup_logger};
/// use log::LevelFilter;
///
/// let logger = setup_logger(vec![LogTarget {
///     level_filter: LevelFilter::Info, // Only log info and above
///     ..Default::default()
/// }]).unwrap();
/// logger.apply().unwrap(); // Register it as the global logger, this can only be done once
/// ```
///
/// Example file logger:
/// ```
/// use bitbazaar::logging::{LogTarget, LogTargetVariant, setup_logger};
/// use log::LevelFilter;
/// use std::path::PathBuf;
///
/// let logger = setup_logger(vec![LogTarget {
///     level_filter: LevelFilter::Info, // Only log info and above
///     variant: LogTargetVariant::File {
///         file_prefix: "my_program_".into(),
///         dir: PathBuf::from("./logs/"),
///     },
///     ..Default::default()
/// }]).unwrap();
/// logger.apply().unwrap(); // Register it as the global logger, this can only be done once
/// ```
pub struct LogTarget {
    /// The prefix to add, e.g. the name of the command (e.g. "etch")
    pub msg_prefix: Option<String>,
    /// The level to log at and above, e.g. `log::LevelFilter::Info`
    pub level_filter: log::LevelFilter,
    /// The target to log to, e.g. `LogTargetVariant::Stdout {}`
    pub variant: LogTargetVariant,
    /// Include the timestamp in each log, e.g. if Some(log::LevelFilter::Info) then only debug and info will have timestamps.
    pub include_ts_till: Option<log::LevelFilter>,
    /// Include the write location of the log in each log, e.g. if Some(log::LevelFilter::Info) then only debug and info will have locations.
    pub include_loc_till: Option<log::LevelFilter>,
    /// A regex that must be satisfied for a log to be accepted by this target.
    /// E.g. if regex is 'logging::tests' then only locations containing this will be logged by this target.
    /// Note that when None, will match all locations other than those matched by other targets with a loc_matcher.
    pub loc_matcher: Option<regex::Regex>,
}

impl Default for LogTarget {
    fn default() -> Self {
        Self {
            msg_prefix: None,
            level_filter: log::LevelFilter::Info,
            variant: LogTargetVariant::Stdout {},
            include_ts_till: None,
            include_loc_till: None,
            loc_matcher: None,
        }
    }
}

impl LogTarget {
    fn consume(
        self,
    ) -> (
        Option<String>,
        log::LevelFilter,
        Option<log::LevelFilter>,
        Option<log::LevelFilter>,
        LogTargetVariant,
        Option<regex::Regex>,
    ) {
        (
            self.msg_prefix,
            self.level_filter,
            self.include_ts_till,
            self.include_loc_till,
            self.variant,
            self.loc_matcher,
        )
    }
}

/// Specify where logs should be written to for a given logger.
pub enum LogTargetVariant {
    /// Write to stdout:
    Stdout {},
    /// Write to files.
    /// Where the string is the path to the log file excluding the end,
    /// e.g. "logs/my-program_" or "logs/".
    /// ${date}.log will be appended to the end of this path, a new file will be created every day:
    File {
        /// The prefix for the filenames, e.g. "graphs_",
        file_prefix: String,
        /// The directory to hold the log files, e.g. `./logs/`, will create if missing.
        dir: PathBuf,
    },
    /// Write to a custom output.
    Custom {
        /// The custom writer function:
        output: fern::Output,
        /// Whether to include the color codes in the output, e.g. for writing to a file I'd turn off:
        include_color: bool,
    },
}

/// Simple interface to setup a logger and output to a given target.
/// Returns the logger, must run `logger.apply()?` To actually enable it as the global logger, this can only be done once.
///
/// See the [`LogTarget`] struct for examples.
pub fn setup_logger(targets: Vec<LogTarget>) -> Result<fern::Dispatch, TracedErr> {
    let mut logger = fern::Dispatch::new();

    let all_loc_matchers = targets
        .iter()
        .filter_map(|target| target.loc_matcher.clone())
        .collect::<Vec<_>>();

    for target in targets {
        let (msg_prefix, level_filter, include_ts_till, include_loc_till, variant, loc_matcher) =
            target.consume();

        logger = match variant {
            LogTargetVariant::Stdout {} => logger.chain(
                create_logger(
                    msg_prefix,
                    level_filter,
                    include_ts_till,
                    include_loc_till,
                    true,
                    loc_matcher,
                    &all_loc_matchers,
                )?
                .chain(std::io::stdout()),
            ),
            LogTargetVariant::File { file_prefix, dir } => {
                // Throw if dir is an existing file:
                if dir.is_file() {
                    return Err(err!(
                        "Log directory is an existing file: {}",
                        dir.to_string_lossy()
                    ));
                }

                // Create the dir if missing:
                if !dir.exists() {
                    std::fs::create_dir_all(&dir)?;
                }

                logger.chain(
                    create_logger(
                        msg_prefix,
                        level_filter,
                        include_ts_till,
                        include_loc_till,
                        false,
                        loc_matcher,
                        &all_loc_matchers,
                    )?
                    // E.g. if prefix is "foo_" and dir is "logs", filename will be e.g. "logs/foo_2019-10-23.log":
                    .chain(fern::DateBased::new(dir.join(file_prefix), "%Y-%m-%d.log")),
                )
            }
            LogTargetVariant::Custom {
                output,
                include_color,
            } => logger.chain(
                create_logger(
                    msg_prefix,
                    level_filter,
                    include_ts_till,
                    include_loc_till,
                    include_color,
                    loc_matcher,
                    &all_loc_matchers,
                )?
                .chain(output),
            ),
        };
    }

    Ok(logger)
}

fn create_logger(
    msg_prefix: Option<String>,
    level_filter: log::LevelFilter,
    include_ts_till: Option<log::LevelFilter>,
    include_loc_till: Option<log::LevelFilter>,
    include_color: bool,
    loc_matcher: Option<regex::Regex>,
    all_loc_matchers: &[regex::Regex],
) -> Result<fern::Dispatch, TracedErr> {
    // Finalise msg_prefix, if not empty/None adding a space after it:
    let msg_prefix = if let Some(msg_prefix) = msg_prefix {
        if !msg_prefix.is_empty() && !msg_prefix.ends_with(' ') {
            format!("{} ", msg_prefix)
        } else {
            msg_prefix
        }
    } else {
        "".into()
    };

    let mut dispatcher = fern::Dispatch::new()
        .format(move |out, message, record| {
            let level = record.level();

            let mut prefix = "".to_string();

            if let Some(include_ts_till) = include_ts_till {
                if level >= include_ts_till {
                    prefix = format!(
                        "{}[{}]",
                        prefix,
                        // Just need the time, no need for date, if stdout clearly not needed, if file the date is in the log file name:
                        chrono::Local::now().format("%H:%M:%S")
                    );
                }
            }

            if let Some(include_loc_till) = include_loc_till {
                if level >= include_loc_till {
                    prefix = format!("{}[{}:{}]", prefix, record.target(), {
                        if let Some(line) = record.line() {
                            line.to_string()
                        } else {
                            "unknown".into()
                        }
                    },);
                }
            }

            // Add a space if not empty:
            if !prefix.is_empty() {
                prefix = format!("{} ", prefix);
            }

            let (prefix_color, lvl_name) = match level {
                Level::Error => (Some(colored::Color::Red), "error"),
                Level::Warn => (Some(colored::Color::Yellow), "warn"),
                Level::Info => (None, "info"),
                Level::Debug => (Some(colored::Color::Cyan), "debug"),
                Level::Trace => (Some(colored::Color::Cyan), "trace"),
            };

            prefix = format!("{}{}{}: ", prefix, &msg_prefix, lvl_name);

            let final_prefix = if include_color {
                if let Some(prefix_color) = prefix_color {
                    prefix.color(prefix_color).bold().to_string()
                } else {
                    prefix.bold().to_string()
                }
            } else {
                prefix
            };

            out.finish(format_args!("{}{}", final_prefix, message));
        })
        .level(level_filter);

    // Skip log if there's a custom location matcher present that doesn't match the file string:
    if let Some(loc_matcher) = loc_matcher {
        dispatcher =
            dispatcher.filter(move |metadata| -> bool { loc_matcher.is_match(metadata.target()) });
    } else if !all_loc_matchers.is_empty() {
        // If there isn't a custom location matcher, don't include if its being picked up by other targets with a loc_matcher:
        let all_loc_matchers = all_loc_matchers.to_vec();
        dispatcher = dispatcher.filter(move |metadata| -> bool {
            !all_loc_matchers
                .iter()
                .any(|matcher| matcher.is_match(metadata.target()))
        });
    }

    Ok(dispatcher)
}
