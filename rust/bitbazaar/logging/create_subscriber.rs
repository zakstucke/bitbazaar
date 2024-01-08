use std::{path::PathBuf, str::FromStr};

use once_cell::sync::Lazy;
use parking_lot::Mutex;
use tracing::{Dispatch, Level, Metadata, Subscriber};
use tracing_appender::non_blocking::WorkerGuard;
use tracing_subscriber::{
    filter::FilterFn, fmt::MakeWriter, prelude::*, registry::LookupSpan, Layer,
};

use crate::{err, errors::TracedErr};

/// Specify which logs should be matched by this layer.
///
/// See the [`create_subscriber`] fn for examples.
#[derive(Debug, Clone)]
pub enum SubLayerFilter {
    /// Only include logs at or above the given level.
    Above(Level),
    /// Include a specific set of levels.
    Only(Vec<Level>),
}

/// A custom writer for logging, to something other than the provided variants.
///
/// See the [`create_subscriber`] fn for examples.
#[derive(Clone)]
pub struct SubCustomWriter {
    /// The fn to handle writing, passed the raw byte string.
    /// If needing a string, can do:
    ///
    /// `let log = String::from_utf8_lossy(log);`
    pub write: fn(&[u8]),
}

impl std::io::Write for SubCustomWriter {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        let buf_len = buf.len();
        (self.write)(buf);
        Ok(buf_len)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl<'writer> tracing_subscriber::fmt::MakeWriter<'writer> for SubCustomWriter {
    type Writer = SubCustomWriter;

    fn make_writer(&self) -> Self::Writer {
        self.clone()
    }
}

/// A target for a specific variation of logging, e.g. stdout, a file, or a custom writer.
///
/// See the [`create_subscriber`] fn for examples.
pub struct SubLayer {
    /// The filter to apply to this layer, e.g. `SubLayerFilter::Above(Level::INFO)`
    pub filter: SubLayerFilter,
    /// The target to log to, e.g. `SubLayerVariant::Stdout {}`
    pub variant: SubLayerVariant,
    /// When enabled, logs will be formatted more verbosely, but neater on the eyes, good for e.g. stdout.
    pub pretty: bool,
    /// Whether to include the active log level in each log, defaults to true
    pub include_lvl: bool,
    /// Include the timestamp in each log, defaults to false
    pub include_timestamp: bool,
    /// Include the log location (file and line) in each log, defaults to false
    pub include_loc: bool,
    /// A regex that must be satisfied for a log to be accepted by this target.
    /// E.g. if regex is 'logging::tests' then only locations containing this will be logged by this target.
    /// Note that when None, will match all locations other than those matched by other layers with a loc_matcher.
    pub loc_matcher: Option<regex::Regex>,
}

impl Default for SubLayer {
    fn default() -> Self {
        Self {
            pretty: false,
            filter: SubLayerFilter::Above(Level::INFO),
            variant: SubLayerVariant::Stdout {},
            include_lvl: true,
            include_timestamp: false,
            include_loc: false,
            loc_matcher: None,
        }
    }
}

impl SubLayer {
    fn consume(
        self,
    ) -> (
        SubLayerFilter,
        bool,
        bool,
        bool,
        bool,
        SubLayerVariant,
        Option<regex::Regex>,
    ) {
        (
            self.filter,
            self.pretty,
            self.include_lvl,
            self.include_timestamp,
            self.include_loc,
            self.variant,
            self.loc_matcher,
        )
    }
}

/// Specify where logs should be written to for a given sub.
///
/// See the [`create_subscriber`] fn for examples.
pub enum SubLayerVariant {
    /// Write to stdout:
    Stdout {},
    /// Write to files:
    File {
        /// The prefix for the filenames, e.g. "graphs.log" which will come out as "graphs.log.2021-01-21,
        file_prefix: String,
        /// The directory to hold the log files, e.g. "./logs/", will create if missing.
        dir: PathBuf,
    },
    /// Write with a custom writer.
    Custom {
        /// The `SubCustomWriter` struct which is passed the custom writer fn:
        writer: SubCustomWriter,
        /// Whether to include the color codes in the output, e.g. for writing to a file I'd turn off:
        include_color: bool,
    },
    #[cfg(feature = "opentelemetry")]
    /// Write to an open telemetry provider. This works with the tokio runtime.
    /// Note all config like pretty, include_lvl, include_loc, etc. are ignored as logs aren't formatted internally.
    OpenTelemetry {
        /// The endpoint to send the logs to, e.g. `https://localhost:4317` is the jaeger default.
        endpoint: String,
        /// Additional headers to send to the provider
        headers: Vec<(String, String)>,
    },
}

// When registering globally, hoist the guards out into here, to allow the CreatedSubscriber to go out of scope but keep the guards permanently.
static GLOBAL_GUARDS: Lazy<Mutex<Option<Vec<WorkerGuard>>>> = Lazy::new(Mutex::default);

pub struct CreatedSubscriber {
    pub dispatch: Dispatch,
    /// Need to store these guards, when they go out of scope the logging may stop.
    /// When made global these are hoisted into a static lazy var.
    guards: Vec<WorkerGuard>,
}

impl CreatedSubscriber {
    /// Register the subscriber as the global subscriber, can only be done once during the lifetime of the program.
    pub fn into_global(self) {
        // Keep hold of the guards:
        GLOBAL_GUARDS.lock().replace(self.guards);
        self.dispatch.init();
    }
}

/// Simple interface to setup a sub and output to a given target.
/// Returns the sub, must run `sub.apply()?` To actually enable it as the global sub, this can only be done once.
///
/// Example sub with an stdout logger:
/// ```
/// use bitbazaar::logging::{SubLayer, create_subscriber, SubLayerFilter};
/// use tracing_subscriber::prelude::*;
/// use tracing::Level;
///
/// let sub = create_subscriber(vec![SubLayer {
///     filter: SubLayerFilter::Above(Level::INFO), // Only log info and above
///     ..Default::default()
/// }]).unwrap();
/// sub.into_global(); // Register it as the global sub, this can only be done once
/// ```
///
/// Example sub with a file logger:
/// ```
/// use bitbazaar::logging::{SubLayer, SubLayerVariant, create_subscriber, SubLayerFilter};
/// use tracing_subscriber::prelude::*;
/// use tracing::Level;
/// use std::path::PathBuf;
///
/// let sub = create_subscriber(vec![SubLayer {
///     filter: SubLayerFilter::Above(Level::INFO), // Only log info and above
///     variant: SubLayerVariant::File {
///         file_prefix: "my_program_".into(),
///         dir: PathBuf::from("./logs/"),
///     },
///     ..Default::default()
/// }]).unwrap();
/// sub.into_global(); // Register it as the global sub, this can only be done once
/// ```
///
/// Example sub with a custom writer:
/// ```
/// use bitbazaar::logging::{SubCustomWriter, create_subscriber, SubLayer, SubLayerVariant, SubLayerFilter};
/// use tracing_subscriber::prelude::*;
/// use tracing::Level;
///
/// let sub = create_subscriber(vec![SubLayer {
///    filter: SubLayerFilter::Above(Level::INFO), // Only log info and above
///    variant: SubLayerVariant::Custom {
///        writer: SubCustomWriter {
///            write: |log| {
///                println!("My custom log: {}", String::from_utf8_lossy(log));
///            },
///        },
///        include_color: false,
///    },
///    ..Default::default()
/// }]).unwrap();
/// sub.into_global(); // Register it as the global sub, this can only be done once
/// ```
///
/// Example sub with open telemetry:
/// E.g. jaeger running locally on localhost:4317 (`docker pull jaegertracing/all-in-one:latest && docker run -d --name jaeger -e COLLECTOR_OTLP_ENABLED=true -p 16686:16686 -p 4317:4317 -p 4318:4318 jaegertracing/all-in-one:latest`)
/// ```no_run
/// use bitbazaar::logging::{SubCustomWriter, create_subscriber, SubLayer, SubLayerVariant, SubLayerFilter};
/// use tracing_subscriber::prelude::*;
/// use tracing::Level;
///
/// let sub = create_subscriber(vec![SubLayer {
///    filter: SubLayerFilter::Above(Level::INFO), // Only log info and above
///    variant: SubLayerVariant::OpenTelemetry {
///        endpoint: "http://localhost:4317".into(),
///        headers: vec![], // Additional headers to send to the provider
///    },
///    ..Default::default()
/// }]).unwrap();
/// sub.into_global(); // Register it as the global sub, this can only be done once
/// ```
pub fn create_subscriber(layers: Vec<SubLayer>) -> Result<CreatedSubscriber, TracedErr> {
    let all_loc_matchers = layers
        .iter()
        .filter_map(|target| target.loc_matcher.clone())
        .collect::<Vec<_>>();

    let mut out_layers = Vec::with_capacity(layers.len());
    let mut guards = vec![];
    for target in layers {
        let (filter, pretty, include_lvl, include_timestamp, include_loc, variant, loc_matcher) =
            target.consume();

        let new_layer = match variant {
            SubLayerVariant::Stdout {} => {
                let (writer, _guard) = tracing_appender::non_blocking(std::io::stdout());
                guards.push(_guard);
                create_fmt_layer(
                    pretty,
                    include_lvl,
                    include_timestamp,
                    include_loc,
                    true,
                    writer,
                )?
            }
            SubLayerVariant::File { file_prefix, dir } => {
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

                // Rotate the file daily:
                let file_appender = tracing_appender::rolling::daily(dir, file_prefix);
                let (writer, _guard) = tracing_appender::non_blocking(file_appender);
                guards.push(_guard);

                create_fmt_layer(
                    pretty,
                    include_lvl,
                    include_timestamp,
                    include_loc,
                    false,
                    writer,
                )?
            }
            SubLayerVariant::Custom {
                writer,
                include_color,
            } => create_fmt_layer(
                pretty,
                include_lvl,
                include_timestamp,
                include_loc,
                include_color,
                writer,
            )?,
            #[cfg(feature = "opentelemetry")]
            SubLayerVariant::OpenTelemetry { endpoint, headers } => {
                use opentelemetry_otlp::WithExportConfig;

                let mut header_map = tonic::metadata::MetadataMap::new();
                for (key, value) in headers {
                    header_map.insert(
                        tonic::metadata::MetadataKey::from_str(&key)?,
                        value.parse()?,
                    );
                }

                // Don't need formatting or anything for the opentelemetry layer, all handled externally.
                let tracer = opentelemetry_otlp::new_pipeline()
                    .tracing()
                    .with_exporter(
                        opentelemetry_otlp::new_exporter()
                            .tonic()
                            .with_endpoint(endpoint)
                            .with_metadata(header_map),
                    )
                    .install_batch(opentelemetry_sdk::runtime::Tokio)?;
                let layer = tracing_opentelemetry::layer().with_tracer(tracer);
                layer.boxed()
            }
        };

        // Now add the filtering for the layer:
        let new_layer =
            new_layer.with_filter(filter_layer(filter, loc_matcher, &all_loc_matchers)?);

        out_layers.push(new_layer);
    }

    // Combine the layers into the final subscriber:
    let subscriber = tracing_subscriber::registry().with(out_layers);
    Ok(CreatedSubscriber {
        dispatch: subscriber.into(),
        guards,
    })
}

fn filter_layer(
    filter: SubLayerFilter,
    loc_matcher: Option<regex::Regex>,
    all_loc_matchers: &[regex::Regex],
) -> Result<FilterFn<impl Fn(&Metadata<'_>) -> bool>, TracedErr> {
    // Needs to be a vec to pass through to the filter fn:
    let all_loc_matchers = all_loc_matchers.to_vec();

    Ok(FilterFn::new(move |metadata| {
        let lvl = metadata.level();

        // Handle the lvl first as this much quicker than the loc matcher:
        match &filter {
            SubLayerFilter::Above(base_lvl) => {
                if lvl > base_lvl {
                    return false;
                }
            }
            SubLayerFilter::Only(specific_lvls) => {
                if !specific_lvls.contains(lvl) {
                    return false;
                }
            }
        }

        // Check loc matching:
        if let Some(file_info) = metadata.file() {
            // Skip log if there's a custom location matcher present that doesn't match the file string:
            if let Some(loc_matcher) = &loc_matcher {
                if !loc_matcher.is_match(file_info) {
                    return false;
                }
            } else if !all_loc_matchers.is_empty() {
                // If there isn't a custom location matcher, don't include if its being picked up by other layers with a loc_matcher:
                if all_loc_matchers
                    .iter()
                    .any(|matcher| matcher.is_match(file_info))
                {
                    return false;
                }
            }
        }

        true
    }))
}

fn create_fmt_layer<S, W>(
    pretty: bool,
    include_lvl: bool,
    include_timestamp: bool,
    include_loc: bool,
    include_color: bool,
    writer: W,
) -> Result<Box<dyn Layer<S> + Send + Sync + 'static>, TracedErr>
where
    S: Subscriber,
    for<'a> S: LookupSpan<'a>, // Each layer has a different type, so have to box for return
    W: for<'writer> MakeWriter<'writer> + Send + Sync + 'static, // Allows all writers to be passed in before boxing
{
    let base_layer = tracing_subscriber::fmt::layer()
        .with_level(include_lvl)
        .with_target(false)
        .with_file(include_loc)
        .with_line_number(include_loc)
        .with_ansi(include_color)
        .with_writer(writer);

    // Annoying have to duplicate, but pretty/compact & time both change the type and prevents adding the filter at the end before boxing:
    if include_timestamp {
        // Create the custom timer, given either stdout or a file rotated daily, no need for date in the log,
        // also no need for any more than ms precision,
        // also make it a UTC time:
        let timer =
            time::format_description::parse("[hour]:[minute]:[second].[subsecond digits:3]")?;
        let time_offset = time::UtcOffset::current_local_offset().unwrap_or(time::UtcOffset::UTC);
        let timer = tracing_subscriber::fmt::time::OffsetTime::new(time_offset, timer);

        if pretty {
            Ok(base_layer.pretty().with_timer(timer).boxed())
        } else {
            Ok(base_layer.compact().with_timer(timer).boxed())
        }
    } else if pretty {
        Ok(base_layer.pretty().without_time().boxed())
    } else {
        Ok(base_layer.compact().without_time().boxed())
    }
}
