use tracing::Level;

/// A simple clap argument group for controlling the log level for cli usage.
#[derive(Debug, clap::Args)]
pub struct ClapLogLevelArgs {
    /// Enable verbose logging.
    #[arg(
        short,
        long,
        global = true,
        group = "verbosity",
        help_heading = "Log levels"
    )]
    pub verbose: bool,
    /// Print diagnostics, but nothing else.
    #[arg(
        short,
        long,
        global = true,
        group = "verbosity",
        help_heading = "Log levels"
    )]
    /// Disable all logging (but still exit with status code "1" upon detecting diagnostics).
    #[arg(
        short,
        long,
        global = true,
        group = "verbosity",
        help_heading = "Log levels"
    )]
    pub silent: bool,
}

impl ClapLogLevelArgs {
    /// Convert the clap log level argument group into a log level that can be passed to `create_subscriber`. Will be None when Silent
    pub fn level(&self) -> Option<Level> {
        if self.silent {
            None
        } else if self.verbose {
            Some(Level::TRACE)
        } else {
            Some(Level::INFO)
        }
    }
}
