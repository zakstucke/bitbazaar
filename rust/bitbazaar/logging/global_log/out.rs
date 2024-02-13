use std::borrow::Cow;

use once_cell::sync::Lazy;
use parking_lot::Mutex;
use tracing::{Dispatch, Level};
use tracing_appender::non_blocking::WorkerGuard;
use tracing_subscriber::prelude::*;

use crate::errors::prelude::*;

// When registering globally, hoist the guards out into here, to allow the CreatedSubscriber to go out of scope but keep the guards permanently.
static GLOBAL_GUARDS: Lazy<Mutex<Option<Vec<WorkerGuard>>>> = Lazy::new(Mutex::default);

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

    /// Need to store these guards, when they go out of scope the logging may stop.
    /// When made global these are hoisted into a static lazy var.
    pub(crate) guards: Vec<WorkerGuard>,

    #[cfg(feature = "opentelemetry")]
    pub(crate) otlp_providers: OtlpProviders,
}

#[cfg(feature = "opentelemetry")]
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
    pub fn setup_quick_stdout_global_logging(level_from: Level) -> Result<(), AnyErr> {
        GlobalLog::builder()
            .stdout(true, false)
            .level_from(level_from)?
            .build()?
            .register_global()?;
        Ok(())
    }

    /// Register the logger as the global logger/tracer/metric manager, can only be done once during the lifetime of the program.
    ///
    /// If you need temporary globality, use the [`GlobalLog::as_tmp_global`] method.
    pub fn register_global(&mut self) -> Result<(), AnyErr> {
        if let Some(dispatch) = self.dispatch.take() {
            // Keep hold of the guards:
            GLOBAL_GUARDS
                .lock()
                .replace(std::mem::take(&mut self.guards));
            dispatch.init();
            Ok(())
        } else {
            Err(anyerr!("Already registered!"))
        }
    }

    #[cfg(feature = "opentelemetry")]
    /// Returns a new [Meter] with the provided name and default configuration.
    ///
    /// A [Meter] should be scoped at most to a single application or crate. The
    /// name needs to be unique so it does not collide with other names used by
    /// an application, nor other applications.
    ///
    /// If the name is empty, then an implementation defined default name will
    /// be used instead.
    pub fn meter(&self, name: impl Into<Cow<'static, str>>) -> opentelemetry::metrics::Meter {
        use opentelemetry::metrics::MeterProvider;

        self.otlp_providers.meter_provider.meter(name)
    }

    /// Temporarily make the logger global, for the duration of the given closure.
    ///
    /// If you want to make the logger global permanently, use the [`GlobalLog::register_global`] method.
    pub fn with_tmp_global<T>(&self, f: impl FnOnce() -> T) -> Result<T, AnyErr> {
        if let Some(dispatch) = &self.dispatch.as_ref() {
            Ok(tracing::dispatcher::with_default(dispatch, f))
        } else {
            Err(anyerr!("GlobalLog missing internal dispatch object! Remember the dispatcher is taken during the register_global() method and cannot be reused."))
        }
    }

    /// Force through logs, traces and metrics, useful in e.g. testing.
    ///
    /// Note there doesn't seem to be an underlying interface to force through metrics.
    pub fn flush(&self) -> Result<(), AnyErr> {
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
        Ok(())
    }
}
