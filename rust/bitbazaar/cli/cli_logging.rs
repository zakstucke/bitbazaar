use colored::Colorize;
use log::Level;

use crate::errors::TracedErr;

#[derive(Debug, Default, PartialOrd, Ord, PartialEq, Eq, Copy, Clone)]
pub enum LogLevel {
    /// No output ([`log::LevelFilter::Off`]).
    Silent,
    /// All user-facing output ([`log::LevelFilter::Info`]).
    #[default]
    Default,
    /// All user-facing output ([`log::LevelFilter::Debug`]).
    Verbose,
}

impl LogLevel {
    #[allow(clippy::trivially_copy_pass_by_ref)]
    pub const fn level_filter(&self) -> log::LevelFilter {
        match self {
            LogLevel::Default => log::LevelFilter::Info,
            LogLevel::Verbose => log::LevelFilter::Debug,
            LogLevel::Silent => log::LevelFilter::Off,
        }
    }
}

/// Sets up a simple stdout logger tailored to cli usage.
pub fn setup_cli_logging(prefix: &str, level: &LogLevel) -> Result<(), TracedErr> {
    fern::Dispatch::new()
        .format(|out, message, record| match record.level() {
            Level::Error => {
                let final_prefix = format!("{} error", prefix);

                out.finish(format_args!(
                    "{}{} {}",
                    final_prefix.red().bold(),
                    ":".bold(),
                    offset_str(final_prefix.len() + 2, message.to_string().as_str())
                ));
            }
            Level::Warn => {
                let final_prefix = format!("{} warn", prefix);

                out.finish(format_args!(
                    "{}{} {}",
                    final_prefix.yellow().bold(),
                    ":".bold(),
                    offset_str(final_prefix.len() + 2, message.to_string().as_str())
                ));
            }
            Level::Info => {
                let final_prefix = format!("{} info", prefix);

                out.finish(format_args!(
                    "{}{} {}",
                    final_prefix.bold(),
                    ":".bold(),
                    offset_str(final_prefix.len() + 2, message.to_string().as_str())
                ));
            }
            Level::Debug | Level::Trace => {
                let level = record.level().to_string().to_lowercase();
                let final_prefix = format!("{}{}", prefix, level);

                out.finish(format_args!(
                    "{}{} {} ({})",
                    final_prefix.cyan().bold(),
                    ":".bold(),
                    offset_str(final_prefix.len() + 2, message.to_string().as_str()),
                    record.target()
                ));
            }
        })
        .level(level.level_filter())
        .level_for("globset", log::LevelFilter::Warn)
        .chain(std::io::stderr())
        .apply()?;
    Ok(())
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
    result
}

#[cfg(test)]
mod tests {
    use crate::logging::log_level::LogLevel;

    #[test]
    fn ordering() {
        assert!(LogLevel::Default > LogLevel::Silent);
        assert!(LogLevel::Default >= LogLevel::Default);
        assert!(LogLevel::Verbose > LogLevel::Default);
        assert!(LogLevel::Verbose > LogLevel::Silent);
    }
}
