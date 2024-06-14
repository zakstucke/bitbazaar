use tracing::{Dispatch, Level, Metadata, Subscriber};
use tracing_subscriber::{filter::FilterFn, layer::SubscriberExt, registry::LookupSpan, Layer};

use super::{builder::GlobalLogBuilder, GlobalLog};
use crate::{log::global_log::event_formatter::CustEventFormatter, prelude::*};

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

pub fn builder_into_global_log(builder: GlobalLogBuilder) -> RResult<GlobalLog, AnyErr> {
    #[cfg(windows)]
    // When on windows, this might be needed to fix colored output:
    let _ = colored::control::set_virtual_terminal(true);

    // Configure the program to automatically log panics as an error event on the current span:
    super::exceptions::auto_trace_panics();

    #[cfg(any(feature = "opentelemetry-grpc", feature = "opentelemetry-http"))]
    // If opentelemetry being used, error_stacks should have color turned off, this would break text in external viewers outside terminals:
    error_stack::Report::set_color_mode(error_stack::fmt::ColorMode::None);

    #[cfg(feature = "log-filter")]
    let all_loc_matchers = builder
        .outputs
        .iter()
        .filter_map(|output| output.shared_opts().loc_matcher.clone())
        .collect::<Vec<_>>();

    #[cfg(any(feature = "opentelemetry-grpc", feature = "opentelemetry-http"))]
    let mut otlp_providers = {
        use super::out::OtlpProviders;
        OtlpProviders {
            logger_provider: None,
            tracer_provider: None,
            meter_provider: opentelemetry_sdk::metrics::MeterProvider::default(),
        }
    };
    let mut out_layers = vec![];

    #[cfg(not(target_arch = "wasm32"))]
    let mut guards = vec![];

    for output in builder.outputs {
        macro_rules! add_layer {
            ($shared:expr, $layer:expr) => {
                // Now add the filtering for the layer:
                out_layers.push(
                    $layer
                        .with_filter(filter_layer(
                            $shared.level_from.clone(),
                            #[cfg(feature = "log-filter")]
                            $shared.loc_matcher.clone(),
                            #[cfg(feature = "log-filter")]
                            &all_loc_matchers,
                        )?)
                        .boxed(),
                );
            };
        }

        match output {
            super::builder::Output::Stdout(stdout) => {
                // When not web, normal std out, color and non-blocking:
                #[cfg(not(target_arch = "wasm32"))]
                {
                    let (writer, _guard) = tracing_appender::non_blocking(std::io::stdout());
                    guards.push(_guard);
                    add_layer!(
                        stdout.shared,
                        create_fmt_layer(stdout.pretty, false, stdout.include_loc, true, writer)?
                    );
                };

                // When web:
                // - custom console writer
                // - no color: current practice is js logging, which will be hooking into this console logger, so color would ruin otlp output
                // - non-blocking as doesn't seem to be supported on web (raises runtime):
                #[cfg(target_arch = "wasm32")]
                {
                    use tracing_subscriber_wasm::MakeConsoleWriter;

                    add_layer!(
                        stdout.shared,
                        create_fmt_layer(
                            stdout.pretty,
                            false,
                            stdout.include_loc,
                            false,
                            MakeConsoleWriter::default()
                        )?
                    );
                };
            }
            // File obvs can't be written in wasm, excluding to keep tracing_appender out of build etc.
            #[cfg(target_arch = "wasm32")]
            super::builder::Output::File(_) => {
                return Err(anyerr!("File logging not supported in wasm."));
            }
            #[cfg(not(target_arch = "wasm32"))]
            super::builder::Output::File(file) => {
                // Throw if dir is an existing file:
                if file.dir.is_file() {
                    return Err(anyerr!(
                        "Log directory is an existing file: {}",
                        file.dir.to_string_lossy()
                    ));
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
            #[cfg(any(feature = "opentelemetry-grpc", feature = "opentelemetry-http"))]
            super::builder::Output::Otlp(otlp) => {
                use opentelemetry::global::set_text_map_propagator;
                use opentelemetry_otlp::new_pipeline;
                use opentelemetry_sdk::{
                    logs as sdklogs,
                    propagation::{
                        BaggagePropagator, TextMapCompositePropagator, TraceContextPropagator,
                    },
                    resource, trace as sdktrace,
                };

                #[cfg(any(feature = "opentelemetry-grpc", feature = "opentelemetry-http"))]
                // Theoretically both features could be enabled, so create an array to be able to double initiate two layers (both grpc and http)
                // makes compiler happy and isn't hacky!
                let mut exporters: Vec<(
                    opentelemetry_otlp::LogExporterBuilder,
                    opentelemetry_otlp::SpanExporterBuilder,
                    opentelemetry_otlp::MetricsExporterBuilder,
                )> = vec![];

                #[cfg(feature = "opentelemetry-grpc")]
                #[allow(unused_variables)]
                if let Some(port) = otlp.grpc_port {
                    use opentelemetry_otlp::{new_exporter, WithExportConfig};

                    // Spinlock up to 10 seconds until the collector is listening, important not to lose startup logs, don't want to continue until we know the collector can receive logs.
                    let wait_start = std::time::Instant::now();
                    let mut found_collector = false;
                    let mut is_first = true;
                    while wait_start.elapsed() < std::time::Duration::from_secs(10) {
                        if crate::misc::is_tcp_port_listening("localhost", port)? {
                            found_collector = true;

                            // If printed the spinlocking message, confirm all good now:
                            if !is_first {
                                println!("Collector up after {:?}!", wait_start.elapsed());
                            }

                            break;
                        }

                        if is_first {
                            println!("Spinlocking for up to 10 seconds for OpenTelemetry collector availability on local port {}...", port);
                            is_first = false;
                        }

                        // Don't want this to delay startup otherwise, so very short waits:
                        std::thread::sleep(std::time::Duration::from_millis(5));
                    }
                    if !found_collector {
                        return Err(anyerr!("Can't connect to open telemetry collector on local port {}. Are you sure it's running?", port));
                    }

                    let endpoint = format!("grpc://localhost:{}", port);
                    let get_exporter = || new_exporter().tonic().with_endpoint(&endpoint);
                    exporters.push((
                        get_exporter().into(),
                        get_exporter().into(),
                        get_exporter().into(),
                    ));
                };

                #[cfg(feature = "opentelemetry-http")]
                if let Some(endpoint) = otlp.http_endpoint {
                    use opentelemetry_otlp::{new_exporter, WithExportConfig};

                    let get_exporter = || new_exporter().http().with_endpoint(&endpoint);
                    exporters.push((
                        get_exporter().into(),
                        get_exporter().into(),
                        get_exporter().into(),
                    ));
                };

                for (log_exporter, trace_exporter, metric_exporter) in exporters {
                    // Configure the global propagator to use between different services, without this step when you try and connect other services they'll strangely not work (this defaults to a no-op)
                    //
                    // Only enable to the 2 main standard propagators, the w3c trace context and baggage.
                    //
                    // https://opentelemetry.io/docs/concepts/sdk-configuration/general-sdk-configuration/#otel_propagators
                    set_text_map_propagator(TextMapCompositePropagator::new(vec![
                        Box::new(TraceContextPropagator::new()),
                        Box::new(BaggagePropagator::new()),
                    ]));

                    #[cfg(not(target_arch = "wasm32"))]
                    let svc_instance_id = hostname::get()
                        .change_context(AnyErr)?
                        .to_string_lossy()
                        .to_string();
                    #[cfg(target_arch = "wasm32")]
                    // TODO: (not much point atm as wasm http doesn't work): maybe something legitimate for web?
                    let svc_instance_id = "wasm".to_string();

                    let resource = resource::Resource::new(vec![
                        opentelemetry::KeyValue::new(
                            opentelemetry_semantic_conventions::resource::SERVICE_NAME,
                            otlp.service_name.clone(),
                        ),
                        opentelemetry::KeyValue::new(
                            opentelemetry_semantic_conventions::resource::SERVICE_VERSION,
                            otlp.service_version.clone(),
                        ),
                        opentelemetry::KeyValue::new(
                            opentelemetry_semantic_conventions::resource::SERVICE_INSTANCE_ID,
                            svc_instance_id,
                        ),
                    ]);

                    // Different layers are needed for the logger, tracer and meter:
                    let logger = new_pipeline()
                        .logging()
                        .with_log_config(sdklogs::Config::default().with_resource(resource.clone()))
                        .with_exporter(log_exporter)
                        .install_batch(opentelemetry_sdk::runtime::Tokio)
                        .change_context(AnyErr)?;
                    let logging_provider = logger
                        .provider()
                        .ok_or_else(|| anyerr!("No logger provider attached."))?;
                    let log_layer = crate::log::ot_tracing_bridge::OpenTelemetryTracingBridge::new(
                        &logging_provider,
                    );
                    otlp_providers.logger_provider = Some(logging_provider.clone());
                    add_layer!(otlp.shared, log_layer);

                    let tracer = new_pipeline()
                        .tracing()
                        .with_trace_config(
                            sdktrace::Config::default().with_resource(resource.clone()),
                        )
                        .with_exporter(trace_exporter)
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
                        .with_exporter(metric_exporter)
                        .build()
                        .change_context(AnyErr)?;
                    let metric_layer: tracing_opentelemetry::MetricsLayer<
                        tracing_subscriber::Registry,
                    > = tracing_opentelemetry::MetricsLayer::new(meter_provider.clone());
                    otlp_providers.meter_provider = meter_provider;
                    add_layer!(otlp.shared, metric_layer);
                }
            }
        };
    }

    // Combine the layers into the final subscriber:
    let subscriber = tracing_subscriber::registry().with(out_layers);
    let dispatch: Dispatch = subscriber.into();
    Ok(GlobalLog {
        dispatch: Some(dispatch),
        #[cfg(not(target_arch = "wasm32"))]
        _guards: guards,
        #[cfg(any(feature = "opentelemetry-grpc", feature = "opentelemetry-http"))]
        otlp_providers,
    })
}

fn filter_layer(
    level_from: Level,
    #[cfg(feature = "log-filter")] loc_matcher: Option<regex::Regex>,
    #[cfg(feature = "log-filter")] all_loc_matchers: &[regex::Regex],
) -> Result<FilterFn<impl Fn(&Metadata<'_>) -> bool>, AnyErr> {
    #[cfg(feature = "log-filter")]
    // Needs to be a vec to pass through to the filter fn:
    let all_loc_matchers = all_loc_matchers.to_vec();

    Ok(FilterFn::new(move |metadata| {
        // Handle the lvl first as this much quicker than the loc matcher:
        if level_from < *metadata.level() {
            return false;
        }

        #[cfg(feature = "log-filter")]
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
) -> RResult<Box<dyn Layer<S> + Send + Sync + 'static>, AnyErr>
where
    S: Subscriber + Send + Sync + 'static,
    for<'a> S: LookupSpan<'a>, // Each layer has a different type, so have to box for return
    W: for<'writer> tracing_subscriber::fmt::MakeWriter<'writer> + Send + Sync + 'static, // Allows all writers to be passed in before boxing
{
    /// README: This is all so complicated because tracing_subscriber layers all have distinct types depending on the args.
    /// We also override the event formatter with our own that defers to the original for everything except exception events,
    /// for exception events we try and keep like a usual stacktrace.
    ///
    /// The macros are all about keeping the code concise, despite the different types and repeated usage (due to lack of clone)

    macro_rules! base {
        ($layer_or_fmt:expr) => {
            $layer_or_fmt
                .with_level(true)
                .with_target(false)
                .with_file(include_loc)
                .with_line_number(include_loc)
                .with_ansi(include_color)
        };
    }

    macro_rules! base_layer {
        () => {
            base!(tracing_subscriber::fmt::layer()).with_writer(writer)
        };
    }

    macro_rules! base_format {
        () => {
            base!(tracing_subscriber::fmt::format())
        };
    }

    // Annoying have to duplicate, but pretty/compact & time both change the type and prevents adding the filter at the end before boxing:
    let layer = if include_timestamp {
        // Create the custom timer, given either stdout or a file rotated daily, no need for date in the log,
        // also no need for any more than ms precision,
        // also make it a UTC time:
        let timer =
            time::format_description::parse("[hour]:[minute]:[second].[subsecond digits:3]")
                .change_context(AnyErr)?;
        let time_offset = time::UtcOffset::current_local_offset().unwrap_or(time::UtcOffset::UTC);
        let timer = tracing_subscriber::fmt::time::OffsetTime::new(time_offset, timer);

        if pretty {
            base_layer!()
                .pretty()
                .with_timer(timer.clone())
                .event_format(CustEventFormatter::new(
                    base_format!().pretty().with_timer(timer),
                ))
                .boxed()
        } else {
            base_layer!()
                .compact()
                .with_timer(timer.clone())
                .event_format(CustEventFormatter::new(
                    base_format!().compact().with_timer(timer),
                ))
                .boxed()
        }
    } else if pretty {
        base_layer!()
            .pretty()
            .without_time()
            .event_format(CustEventFormatter::new(
                base_format!().pretty().without_time(),
            ))
            .boxed()
    } else {
        base_layer!()
            .compact()
            .without_time()
            .event_format(CustEventFormatter::new(
                base_format!().compact().without_time(),
            ))
            .boxed()
    };

    Ok(layer)
}
