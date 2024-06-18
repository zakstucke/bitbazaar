use std::path::PathBuf;

use tracing::Level;

use super::GlobalLog;
use crate::prelude::*;

#[derive(Clone)]
/// Shared that can be set for all output types
pub struct SharedOpts {
    pub level_from: Level,

    // Keeping when feature disabled to make a bit more concise in usage:
    #[cfg(feature = "log-filter")]
    pub loc_matcher: Option<regex::Regex>,
    #[cfg(not(feature = "log-filter"))]
    #[allow(dead_code)]
    pub loc_matcher: Option<bool>,
}

impl Default for SharedOpts {
    fn default() -> Self {
        Self {
            level_from: Level::INFO,
            loc_matcher: None,
        }
    }
}

pub struct StdoutConf {
    /// When enabled, logs will be formatted more verbosely, but neater on the eyes.
    pub pretty: bool,
    /// Include the log location (file and line) in each log, defaults to false
    pub include_loc: bool,
    pub shared: SharedOpts,
}

pub struct FileConf {
    /// The prefix for the filenames, e.g. "graphs.log" which will come out as "graphs.log.2021-01-21,
    pub file_prefix: String,
    /// The directory to hold the log files, e.g. "./logs/", will create if missing.
    pub dir: PathBuf,
    pub shared: SharedOpts,
}

#[derive(Clone)]
pub struct CustomConf {
    /// When enabled, logs will be formatted more verbosely, but neater on the eyes.
    pub pretty: bool,
    /// Include the log location (file and line) in each log, defaults to false
    pub include_loc: bool,
    /// Include the timestamp in each log, defaults to false
    pub include_ts: bool,
    /// The fn to handle writing, passed the raw byte string.
    /// If needing a string, can do:
    ///
    /// `let log = String::from_utf8_lossy(log);`
    pub write: fn(&[u8]),
    /// Whether to include the color codes in the output, e.g. for writing to a file I'd turn off:
    pub include_color: bool,
    pub shared: SharedOpts,
}

#[cfg(any(feature = "opentelemetry-grpc", feature = "opentelemetry-http"))]
pub struct OtlpConf {
    #[cfg(feature = "opentelemetry-grpc")]
    /// The localhost port the open telemetry collector is running on and accepting grpc connections:
    pub grpc_port: Option<u16>,
    #[cfg(feature = "opentelemetry-http")]
    /// The url string to connect via http to, e.g. "/otlp" or "localhost/otlp":
    pub http_endpoint: Option<String>,
    /// The name of the service:
    pub service_name: String,
    /// The active version/deployment of the service:
    pub service_version: String,
    pub shared: SharedOpts,
}

/// The global log builder. See the [`GlobalLog`] struct for more information.
#[derive(Default)]
pub struct GlobalLogBuilder {
    pub(crate) outputs: Vec<Output>,
}

impl GlobalLogBuilder {
    /// Build the global log from the configured builder.
    pub fn build(self) -> RResult<GlobalLog, AnyErr> {
        super::setup::builder_into_global_log(self)
    }

    /// Write to stdout:
    ///
    /// Arguments:
    /// - `pretty`: When enabled, logs are formatted more verbosely, but easier on the eyes.
    /// - `include_loc`: When enabled, log contains write location (file and line).
    pub fn stdout(mut self, pretty: bool, include_loc: bool) -> Self {
        self.outputs.push(Output::Stdout(StdoutConf {
            pretty,
            include_loc,
            shared: SharedOpts::default(),
        }));
        self
    }

    /// Write to a file:
    ///
    /// Arguments:
    /// - `file_prefix`: The prefix for the filenames, e.g. "graphs.log" which will come out as "graphs.log.2021-01-21,
    /// - `dir`: The directory to hold the log files, e.g. "./logs/", will create if missing.
    pub fn file(mut self, file_prefix: impl Into<String>, dir: impl Into<PathBuf>) -> Self {
        self.outputs.push(Output::File(FileConf {
            file_prefix: file_prefix.into(),
            dir: dir.into(),
            shared: SharedOpts::default(),
        }));
        self
    }

    /// Write to a custom writer.
    ///
    /// Arguments:
    /// - `pretty`: When enabled, logs are formatted more verbosely, but easier on the eyes.
    /// - `include_loc`: When enabled, log contains write location (file and line).
    /// - `include_color`: When enabled, log contains colors.
    /// - `include_ts`: When enabled, log contains timestamp.
    /// - `writer`: The fn to handle writing, passed the raw byte string.
    ///
    /// If needing a string in the writer, can do:
    ///
    /// `let log = String::from_utf8_lossy(log);`
    pub fn custom(
        mut self,
        pretty: bool,
        include_loc: bool,
        include_color: bool,
        include_ts: bool,
        writer: fn(&[u8]),
    ) -> Self {
        self.outputs.push(Output::Custom(CustomConf {
            pretty,
            include_loc,
            include_color,
            include_ts,
            write: writer,
            shared: SharedOpts::default(),
        }));
        self
    }

    #[cfg(feature = "opentelemetry-grpc")]
    /// Write to an open telemetry provider via grpc. This works with the tokio runtime.
    ///
    /// Arguments:
    /// - `port`: The localhost port the open telemetry collector is running on and accepting grpc connections:
    /// - `service_name`: The name of the service:
    /// - `service_version`: The active version/deployment of the service:
    pub fn otlp_grpc(
        mut self,
        port: u16,
        service_name: impl Into<String>,
        service_version: impl Into<String>,
    ) -> Self {
        self.outputs.push(Output::Otlp(OtlpConf {
            grpc_port: Some(port),
            #[cfg(feature = "opentelemetry-http")]
            http_endpoint: None,
            service_name: service_name.into(),
            service_version: service_version.into(),
            shared: SharedOpts::default(),
        }));
        self
    }

    #[cfg(feature = "opentelemetry-http")]
    /// Write to an open telemetry provider via http. This works with wasm!
    ///
    /// Arguments:
    /// - `endpoint`: The url string to connect via http to, e.g. "/otlp" or "http://localhost/otlp":
    /// - `service_name`: The name of the service:
    /// - `service_version`: The active version/deployment of the service:
    pub fn otlp_http(
        mut self,
        endpoint: impl Into<String>,
        service_name: impl Into<String>,
        service_version: impl Into<String>,
    ) -> Self {
        self.outputs.push(Output::Otlp(OtlpConf {
            #[cfg(feature = "opentelemetry-grpc")]
            grpc_port: None,
            http_endpoint: Some(endpoint.into()),
            service_name: service_name.into(),
            service_version: service_version.into(),
            shared: SharedOpts::default(),
        }));
        self
    }

    /// Set the minimum level to log for.
    ///
    /// NOTE: Applies to the last set output type only.
    pub fn level_from(mut self, level: Level) -> RResult<Self, AnyErr> {
        let shared = self.get_active_shared()?;
        shared.level_from = level;
        Ok(self)
    }

    #[cfg(feature = "log-filter")]
    /// A regex that must be satisfied for a log to be accepted by this target.
    /// E.g. if regex is 'logging::tests' then only locations containing this will be logged by this target.
    /// Note that when None, will match all locations other than those matched by other layers with a loc_matcher.
    ///
    /// NOTE: Applies to the last set output type only.
    pub fn loc_matcher(mut self, loc_matcher: regex::Regex) -> RResult<Self, AnyErr> {
        let shared = self.get_active_shared()?;
        shared.loc_matcher = Some(loc_matcher);
        Ok(self)
    }

    fn get_active_shared(&mut self) -> RResult<&mut SharedOpts, AnyErr> {
        if let Some(output) = self.outputs.last_mut() {
            Ok(match output {
                Output::Stdout(conf) => &mut conf.shared,
                Output::File(conf) => &mut conf.shared,
                Output::Custom(conf) => &mut conf.shared,
                #[cfg(any(feature = "opentelemetry-grpc", feature = "opentelemetry-http"))]
                Output::Otlp(conf) => &mut conf.shared,
            })
        } else {
            Err(anyerr!(
                "No output set yet to apply this value to. Set an output first."
            ))
        }
    }
}

pub enum Output {
    Stdout(StdoutConf),
    File(FileConf),
    Custom(CustomConf),
    #[cfg(any(feature = "opentelemetry-grpc", feature = "opentelemetry-http"))]
    Otlp(OtlpConf),
}

impl Output {
    #[allow(dead_code)]
    pub fn shared_opts(&self) -> &SharedOpts {
        match self {
            Output::Stdout(conf) => &conf.shared,
            Output::File(conf) => &conf.shared,
            Output::Custom(conf) => &conf.shared,
            #[cfg(any(feature = "opentelemetry-grpc", feature = "opentelemetry-http"))]
            Output::Otlp(conf) => &conf.shared,
        }
    }
}
