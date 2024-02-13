use tracing::{Dispatch, Level, Metadata, Subscriber};
use tracing_subscriber::{
    filter::FilterFn, fmt::MakeWriter, prelude::*, registry::LookupSpan, Layer,
};

use super::{builder::GlobalLogBuilder, GlobalLog};
use crate::prelude::*;

/// Need the write trait for our write function.
impl std::io::Write for super::builder::CustomConf {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        let buf_len = buf.len();
        (self.write)(buf);
        Ok(buf_len)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

// Need to be able to convert into a tracing writer:
impl<'writer> tracing_subscriber::fmt::MakeWriter<'writer> for super::builder::CustomConf {
    type Writer = super::builder::CustomConf;

    fn make_writer(&self) -> Self::Writer {
        self.clone()
    }
}

pub fn builder_into_global_log(builder: GlobalLogBuilder) -> Result<GlobalLog, AnyErr> {
    let all_loc_matchers = builder
        .outputs
        .iter()
        .filter_map(|output| output.shared_opts().loc_matcher.clone())
        .collect::<Vec<_>>();

    #[cfg(feature = "opentelemetry")]
    use super::out::OtlpProviders;
    #[cfg(feature = "opentelemetry")]
    let mut otlp_providers = OtlpProviders {
        logger_provider: None,
        tracer_provider: None,
        meter_provider: opentelemetry_sdk::metrics::MeterProvider::default(),
    };
    let mut out_layers = vec![];
    let mut guards = vec![];
    for output in builder.outputs {
        macro_rules! add_layer {
            ($shared:expr, $layer:expr) => {
                // Now add the filtering for the layer:
                out_layers.push(
                    $layer
                        .with_filter(filter_layer(
                            $shared.level_from.clone(),
                            $shared.loc_matcher.clone(),
                            &all_loc_matchers,
                        )?)
                        .boxed(),
                );
            };
        }

        match output {
            super::builder::Output::Stdout(stdout) => {
                let (writer, _guard) = tracing_appender::non_blocking(std::io::stdout());
                guards.push(_guard);
                add_layer!(
                    stdout.shared,
                    create_fmt_layer(stdout.pretty, false, stdout.include_loc, true, writer,)?
                );
            }
            super::builder::Output::File(file) => {
                // Throw if dir is an existing file:
                if file.dir.is_file() {
                    bail!(report!(AnyErr).attach_printable(format!(
                        "Log directory is an existing file: {}",
                        file.dir.to_string_lossy()
                    )));
                }

                // Create the dir if missing:
                if !file.dir.exists() {
                    std::fs::create_dir_all(&file.dir).change_context(AnyErr)?;
                }

                // Rotate the file daily:
                let file_appender = tracing_appender::rolling::daily(file.dir, file.file_prefix);
                let (writer, _guard) = tracing_appender::non_blocking(file_appender);
                guards.push(_guard);

                add_layer!(
                    file.shared,
                    create_fmt_layer(false, true, true, false, writer,)?
                );
            }
            super::builder::Output::Custom(custom) => {
                let shared = custom.shared.clone();
                add_layer!(
                    shared,
                    create_fmt_layer(
                        custom.pretty,
                        custom.include_ts,
                        custom.include_loc,
                        custom.include_color,
                        custom,
                    )?
                );
            }
            #[cfg(feature = "opentelemetry")]
            super::builder::Output::Otlp(otlp) => {
                use opentelemetry_otlp::{new_exporter, new_pipeline, WithExportConfig};
                use opentelemetry_sdk::{logs as sdklogs, resource, trace as sdktrace};

                if !crate::misc::is_tcp_port_listening("localhost", otlp.port)? {
                    return Err(anyerr!("Can't connect to open telemetry collector on local port {}. Are you sure it's running?", otlp.port));
                }

                let endpoint = format!("grpc://localhost:{}", otlp.port);
                let get_exporter = || new_exporter().tonic().with_endpoint(&endpoint);

                let resource = resource::Resource::new(vec![
                    opentelemetry::KeyValue::new(
                        opentelemetry_semantic_conventions::resource::SERVICE_NAME,
                        otlp.service_name,
                    ),
                    opentelemetry::KeyValue::new(
                        opentelemetry_semantic_conventions::resource::SERVICE_VERSION,
                        otlp.service_version,
                    ),
                    opentelemetry::KeyValue::new(
                        opentelemetry_semantic_conventions::resource::SERVICE_INSTANCE_ID,
                        hostname::get()
                            .change_context(AnyErr)?
                            .to_string_lossy()
                            .to_string(),
                    ),
                ]);

                // Different layers are needed for the logger, tracer and meter:
                let logger = new_pipeline()
                    .logging()
                    .with_log_config(sdklogs::Config::default().with_resource(resource.clone()))
                    .with_exporter(get_exporter())
                    .install_batch(opentelemetry_sdk::runtime::Tokio)
                    .change_context(AnyErr)?;
                let logging_provider = logger
                    .provider()
                    .ok_or_else(|| anyerr!("No log provider attached."))?;
                let log_layer = crate::logging::ot_tracing_bridge::OpenTelemetryTracingBridge::new(
                    &logging_provider,
                );
                otlp_providers.logger_provider = Some(logging_provider);
                add_layer!(otlp.shared, log_layer);

                let tracer = new_pipeline()
                    .tracing()
                    .with_trace_config(sdktrace::Config::default().with_resource(resource.clone()))
                    .with_exporter(get_exporter())
                    .install_batch(opentelemetry_sdk::runtime::Tokio)
                    .change_context(AnyErr)?;
                let tracing_provider = tracer
                    .provider()
                    .ok_or_else(|| anyerr!("No tracing provider attached."))?;
                let trace_layer = tracing_opentelemetry::layer().with_tracer(tracer);
                otlp_providers.tracer_provider = Some(tracing_provider);
                add_layer!(otlp.shared, trace_layer);

                let meter_provider = new_pipeline()
                    .metrics(opentelemetry_sdk::runtime::Tokio)
                    .with_resource(resource.clone())
                    .with_exporter(get_exporter())
                    .build()
                    .change_context(AnyErr)?;
                let metric_layer: tracing_opentelemetry::MetricsLayer<
                    tracing_subscriber::Registry,
                > = tracing_opentelemetry::MetricsLayer::new(meter_provider.clone());
                otlp_providers.meter_provider = meter_provider;
                add_layer!(otlp.shared, metric_layer);
            }
        };
    }

    // Combine the layers into the final subscriber:
    let subscriber = tracing_subscriber::registry().with(out_layers);
    let dispatch: Dispatch = subscriber.into();
    Ok(GlobalLog {
        dispatch: Some(dispatch),
        guards,
        #[cfg(feature = "opentelemetry")]
        otlp_providers,
    })
}

fn filter_layer(
    level_from: Level,
    loc_matcher: Option<regex::Regex>,
    all_loc_matchers: &[regex::Regex],
) -> Result<FilterFn<impl Fn(&Metadata<'_>) -> bool>, AnyErr> {
    // Needs to be a vec to pass through to the filter fn:
    let all_loc_matchers = all_loc_matchers.to_vec();

    Ok(FilterFn::new(move |metadata| {
        // Handle the lvl first as this much quicker than the loc matcher:
        if level_from < *metadata.level() {
            return false;
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
    include_timestamp: bool,
    include_loc: bool,
    include_color: bool,
    writer: W,
) -> Result<Box<dyn Layer<S> + Send + Sync + 'static>, AnyErr>
where
    S: Subscriber,
    for<'a> S: LookupSpan<'a>, // Each layer has a different type, so have to box for return
    W: for<'writer> MakeWriter<'writer> + Send + Sync + 'static, // Allows all writers to be passed in before boxing
{
    let base_layer = tracing_subscriber::fmt::layer()
        .with_level(true)
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
            time::format_description::parse("[hour]:[minute]:[second].[subsecond digits:3]")
                .change_context(AnyErr)?;
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