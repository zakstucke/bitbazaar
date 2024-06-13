use once_cell::sync::Lazy;
use parking_lot::Mutex;
use tracing::{Dispatch, Level};
use tracing_subscriber::prelude::*;

use crate::errors::prelude::*;

pub static GLOBAL_LOG: Lazy<Mutex<Option<GlobalLog>>> = Lazy::new(Mutex::default);

/// The global logger/tracer for stdout, file and full open telemetry. Works with the tracing crates (info!, debug!, warn!, error!) and span funcs and decorators.
///
/// [`GlobalLog::meter`] is also provided to create metrics, these aren't native to the tracing crate.
///
/// Unlike my other lang implementations, for file and stdout this uses a separate flow, they won't use metrics and only receive basic span information.
/// Simply because I did it first, plus the rust opentelemetry sdk is very new, so difficult to use and
/// the benefit of handling file and stdout through otlp is minimal.
///
/// Open telemetry support is opinionated: unencrypted/uncompressed output to a local grpc port, the intention is this is a otpl collector sidecar.
///
/// Examples:
///
/// Kitchen sink:
/// ```
/// use bitbazaar::logging::GlobalLog;
/// use tracing_subscriber::prelude::*;
/// use tracing::Level;
///
/// let temp_dir = tempfile::tempdir().unwrap();
/// let log = GlobalLog::builder()
///             .stdout(true, false)
///             .level_from(Level::DEBUG) // Debug and up for stdout, each defaults to INFO
///             .file("my_program.log", temp_dir)
///             .otlp(4317, "service-name", "0.1.0").level_from(Level::INFO)
///             .build().unwrap();
/// log.register_global()?; // Register it as the global sub, this can only be done once
/// ```
pub struct GlobalLog {
    /// Tracing dispatcher, needed to make the global logger.
    pub(crate) dispatch: Option<Dispatch>,

    // tracing_appender not included in wasm:
    #[cfg(not(target_arch = "wasm32"))]
    /// Need to store these guards, when they go out of scope the logging may stop.
    /// When made global these are hoisted into a static lazy var.
    pub(crate) _guards: Vec<tracing_appender::non_blocking::WorkerGuard>,

    #[cfg(any(feature = "opentelemetry-grpc", feature = "opentelemetry-http"))]
    pub(crate) otlp_providers: OtlpProviders,
}

#[cfg(any(feature = "opentelemetry-grpc", feature = "opentelemetry-http"))]
pub struct OtlpProviders {
    pub logger_provider: Option<opentelemetry_sdk::logs::LoggerProvider>,
    pub tracer_provider: Option<opentelemetry_sdk::trace::TracerProvider>,
    // Will always create one, dummy if not being initiated by user, to allow meter() to still work:
    pub meter_provider: opentelemetry_sdk::metrics::MeterProvider,
}

impl GlobalLog {
    /// Create a builder to configure the global logger.
    pub fn builder() -> super::builder::GlobalLogBuilder {
        super::builder::GlobalLogBuilder::default()
    }

    /// A managed wrapper on creation of the GlobalLog and registering it as the global logger.
    ///
    /// Sets up console logging only. Should only be used for quick logging, as an example and testing.
    pub fn setup_quick_stdout_global_logging(level_from: Level) -> RResult<(), AnyErr> {
        GlobalLog::builder()
            .stdout(true, false)
            .level_from(level_from)?
            .build()?
            .register_global()?;
        Ok(())
    }

    /// Register the logger as the global logger/tracer/metric manager, can only be done once during the lifetime of the program.
    ///
    /// If you need temporary globality, use the [`GlobalLog::with_tmp_global`] method.
    pub fn register_global(mut self) -> RResult<(), AnyErr> {
        if let Some(dispatch) = self.dispatch.take() {
            // Make it global:
            GLOBAL_LOG.lock().replace(self);
            dispatch.init();
            Ok(())
        } else {
            Err(anyerr!("Already registered!"))
        }
    }

    #[cfg(any(feature = "opentelemetry-grpc", feature = "opentelemetry-http"))]
    /// See [`super::global_fns::meter`]`
    pub fn meter(
        &self,
        name: impl Into<std::borrow::Cow<'static, str>>,
    ) -> RResult<opentelemetry::metrics::Meter, AnyErr> {
        use opentelemetry::metrics::MeterProvider;

        Ok(self.otlp_providers.meter_provider.meter(name))
    }

    #[cfg(any(feature = "opentelemetry-grpc", feature = "opentelemetry-http"))]
    /// See [`super::global_fns::set_span_parent_from_http_headers`]`
    pub fn set_span_parent_from_http_headers(
        &self,
        span: &tracing::Span,
        headers: &http::HeaderMap,
    ) -> RResult<(), AnyErr> {
        use tracing_opentelemetry::OpenTelemetrySpanExt;

        use crate::log::global_log::http_headers::HeaderExtractor;

        let ctx_extractor = HeaderExtractor(headers);
        let ctx = opentelemetry::global::get_text_map_propagator(|propagator| {
            propagator.extract(&ctx_extractor)
        });
        span.set_parent(ctx);
        Ok(())
    }

    #[cfg(any(feature = "opentelemetry-grpc", feature = "opentelemetry-http"))]
    /// See [`super::global_fns::set_response_headers_from_ctx`]`
    pub fn set_response_headers_from_ctx<B>(
        &self,
        response: &mut http::Response<B>,
    ) -> RResult<(), AnyErr> {
        use tracing_opentelemetry::OpenTelemetrySpanExt;

        use crate::log::global_log::http_headers::HeaderInjector;

        let ctx = tracing::Span::current().context();
        let mut injector = HeaderInjector(response.headers_mut());
        opentelemetry::global::get_text_map_propagator(|propagator| {
            propagator.inject_context(&ctx, &mut injector);
        });
        Ok(())
    }

    /// Temporarily make the logger global, for the duration of the given closure.
    ///
    /// If you want to make the logger global permanently, use the [`GlobalLog::register_global`] method.
    pub fn with_tmp_global<T>(&self, f: impl FnOnce() -> T) -> RResult<T, AnyErr> {
        if let Some(dispatch) = &self.dispatch.as_ref() {
            Ok(tracing::dispatcher::with_default(dispatch, f))
        } else {
            Err(anyerr!("GlobalLog missing internal dispatch object! Remember the dispatcher is taken during the register_global() method and cannot be reused."))
        }
    }

    /// See [`super::global_fns::flush`]`
    pub fn flush(&self) -> RResult<(), AnyErr> {
        #[cfg(any(feature = "opentelemetry-grpc", feature = "opentelemetry-http"))]
        {
            if let Some(prov) = &self.otlp_providers.logger_provider {
                prov.force_flush();
            }
            if let Some(prov) = &self.otlp_providers.tracer_provider {
                prov.force_flush();
            }
            self.otlp_providers
                .meter_provider
                .force_flush()
                .change_context(AnyErr)?;
        }
        Ok(())
    }

    /// See [`super::global_fns::shutdown`]`
    pub fn shutdown(&mut self) -> RResult<(), AnyErr> {
        #[cfg(any(feature = "opentelemetry-grpc", feature = "opentelemetry-http"))]
        {
            if let Some(prov) = &mut self.otlp_providers.logger_provider {
                prov.shutdown();
            }
            if let Some(prov) = &self.otlp_providers.tracer_provider {
                // Doesn't have a shutdown interface.
                prov.force_flush();
            }
            self.otlp_providers
                .meter_provider
                .shutdown()
                .change_context(AnyErr)?;
        }
        Ok(())
    }
}
