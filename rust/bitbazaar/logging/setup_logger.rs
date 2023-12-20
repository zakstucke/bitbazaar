use std::path::PathBuf;

use colored::Colorize;
use log::Level;

use crate::{err, errors::TracedErr};

/// A target for logging, e.g. stdout, a file, or a custom writer.
pub struct LogTarget {
    /// The prefix to add, e.g. the name of the command (e.g. "scaf")
    pub msg_prefix: Option<String>,
    /// The level to log at and above, e.g. `log::LevelFilter::Info`
    pub level_filter: log::LevelFilter,
    /// The target to log to, e.g. `LogTargetVariant::Stdout {}`
    pub variant: LogTargetVariant,
    /// Include the timestamp in each log, e.g. if Some(log::LevelFilter::Info) then only debug and info will have timestamps.
    pub include_ts_till: Option<log::LevelFilter>,
    /// Include the write location of the log in each log, e.g. if Some(log::LevelFilter::Info) then only debug and info will have locations.
    pub include_location_till: Option<log::LevelFilter>,
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
    ) {
        (
            self.msg_prefix,
            self.level_filter,
            self.include_ts_till,
            self.include_location_till,
            self.variant,
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
pub fn setup_logger(targets: Vec<LogTarget>) -> Result<fern::Dispatch, TracedErr> {
    let mut logger = fern::Dispatch::new();

    for target in targets {
        let (msg_prefix, level_filter, include_ts_till, include_location_till, variant) =
            target.consume();
        logger = match variant {
            LogTargetVariant::Stdout {} => logger.chain(
                create_logger(
                    msg_prefix,
                    level_filter,
                    include_ts_till,
                    include_location_till,
                    true,
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
                        include_location_till,
                        false,
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
                    include_location_till,
                    include_color,
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
    include_location_till: Option<log::LevelFilter>,
    include_color: bool,
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

    Ok(fern::Dispatch::new()
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

            if let Some(include_location_till) = include_location_till {
                if level >= include_location_till {
                    prefix = format!("{}[{}:{}]", prefix, record.file().unwrap_or("unknown"), {
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

            out.finish(format_args!(
                "{}{}",
                final_prefix,
                offset_str(final_prefix.len() + 2, message.to_string().as_str())
            ));
        })
        .level(level_filter)
        // TODO not sure on this level_for thing.
        .level_for("globset", log::LevelFilter::Warn))
}

fn offset_str(offset: usize, value: &str) -> String {
    // For every line but the first, add the offset as whitespace:
    let mut result = String::new();
    for (i, line) in value.lines().enumerate() {
        if i > 0 && !line.is_empty() {
            result.push_str(&" ".repeat(offset));
        }
        result.push_str(line);
        result.push('\n');
    }
    result.trim_end().to_string()
}
