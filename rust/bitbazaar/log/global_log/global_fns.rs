use parking_lot::{MappedMutexGuard, MutexGuard};

use super::{out::GLOBAL_LOG, GlobalLog};
use crate::prelude::*;

/// Record an exception to the currently active span, making sure the record location is added to the stacktrace.
/// Matches oltp spec so it shows up correctly as an exception in observers
/// <https://opentelemetry.io/docs/specs/semconv/exceptions/exceptions-spans/>
///
/// Arguments:
/// - `message`: Information about the exception e.g. `Internal Error leading to 500 http response`.
/// - `stacktrace`: All of the location information for the exception, (maybe also the exception itself if e.g. from `Report<T>`).
#[track_caller]
pub fn record_exception(message: impl Into<String>, stacktrace: impl Into<String>) {
    let caller = std::panic::Location::caller();
    record_exception_custom_caller(caller, message, stacktrace);
}

/// Same as [`record_exception`] except you pass a custom caller.
pub fn record_exception_custom_caller(
    caller: &std::panic::Location<'_>,
    message: impl Into<String>,
    stacktrace: impl Into<String>,
) {
    let mut stacktrace = stacktrace.into();
    stacktrace = if stacktrace.trim().is_empty() {
        format!("╰╴at {}", caller)
    } else {
        format!("{}\n╰╴at {}", stacktrace, caller)
    };
    super::exceptions::record_exception_inner(message, stacktrace, "Err");
}

#[cfg(any(feature = "opentelemetry-grpc", feature = "opentelemetry-http"))]
/// Returns a new [`opentelemetry::metrics::Meter`] with the provided name and default configuration.
///
/// A [opentelemetry::metrics::Meter] should be scoped at most to a single application or crate. The
/// name needs to be unique so it does not collide with other names used by
/// an application, nor other applications.
///
/// If the name is empty, then an implementation defined default name will
/// be used instead.
pub fn meter(name: impl Into<std::borrow::Cow<'static, str>>) -> opentelemetry::metrics::Meter {
    opentelemetry::global::meter(name)
}

#[cfg(any(feature = "opentelemetry-grpc", feature = "opentelemetry-http"))]
/// Returns the default [`opentelemetry::metrics::Meter`] for the app, labelled "default".
pub fn global_meter() -> &'static opentelemetry::metrics::Meter {
    use std::sync::LazyLock;

    static GLOBAL_METER: LazyLock<opentelemetry::metrics::Meter> =
        LazyLock::new(|| opentelemetry::global::meter("default"));

    &GLOBAL_METER
}

#[cfg(any(feature = "opentelemetry-grpc", feature = "opentelemetry-http"))]
/// Connect this program's span to the trace that is represented by the provided HTTP headers.
/// E.g. connect an axum handler's trace/span to the nginx trace/span.
pub fn set_span_parent_from_http_headers(
    span: &tracing::Span,
    headers: &http::HeaderMap,
) -> RResult<(), AnyErr> {
    get_global()?.set_span_parent_from_http_headers(span, headers)
}

#[cfg(any(feature = "opentelemetry-grpc", feature = "opentelemetry-http"))]
/// Set the response headers from the current span context. So downstream services can continue the current trace.
pub fn set_response_headers_from_ctx<B>(response: &mut http::Response<B>) -> RResult<(), AnyErr> {
    get_global()?.set_response_headers_from_ctx(response)
}

/// NOTE ALL LOGGING WILL CEASE AFTER A FLUSH!
/// Force through logs, traces and metrics, useful in e.g. testing.
///
/// Note there doesn't seem to be an underlying interface to force through metrics.
pub fn flush_and_consume() -> RResult<(), AnyErr> {
    let glob = GLOBAL_LOG.lock().take();
    if let Some(glob) = glob {
        glob.flush_and_consume()
    } else {
        Err(anyerr!("GlobalLog not registered or already consumed!"))
    }
}

/// Shutdown the logger, traces and metrics, should be called when the program is about to exit.
pub fn shutdown() -> RResult<(), AnyErr> {
    get_global()?.shutdown()
}

fn get_global<'a>() -> RResult<MappedMutexGuard<'a, GlobalLog>, AnyErr> {
    if GLOBAL_LOG.lock().is_none() {
        return Err(anyerr!("GlobalLog not registered or already consumed!"));
    }
    Ok(MutexGuard::map(GLOBAL_LOG.lock(), |x| x.as_mut().unwrap()))
}
