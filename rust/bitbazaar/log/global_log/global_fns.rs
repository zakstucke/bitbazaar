use parking_lot::{MappedMutexGuard, MutexGuard};

use super::{out::GLOBAL_LOG, GlobalLog};
use crate::prelude::*;

/// Record an exception to the currently active span.
/// Matches oltp spec so it shows up correctly as an exception in observers
/// <https://opentelemetry.io/docs/specs/semconv/exceptions/exceptions-spans/>
///
/// Arguments:
/// - `message`: Information about the exception e.g. `Internal Error leading to 500 http response`.
/// - `stacktrace`: All of the location information for the exception, (maybe also the exception itself if e.g. from `Report<T>`).
pub fn record_exception(message: impl Into<String>, stacktrace: impl Into<String>) {
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
pub fn meter(
    name: impl Into<std::borrow::Cow<'static, str>>,
) -> Result<opentelemetry::metrics::Meter, AnyErr> {
    get_global()?.meter(name)
}

#[cfg(any(feature = "opentelemetry-grpc", feature = "opentelemetry-http"))]
/// Connect this program's span to the trace that is represented by the provided HTTP headers.
/// E.g. connect an axum handler's trace/span to the nginx trace/span.
pub fn set_span_parent_from_http_headers(
    span: &tracing::Span,
    headers: &http::HeaderMap,
) -> Result<(), AnyErr> {
    get_global()?.set_span_parent_from_http_headers(span, headers)
}

#[cfg(any(feature = "opentelemetry-grpc", feature = "opentelemetry-http"))]
/// Set the response headers from the current span context. So downstream services can continue the current trace.
pub fn set_response_headers_from_ctx<B>(response: &mut http::Response<B>) -> Result<(), AnyErr> {
    get_global()?.set_response_headers_from_ctx(response)
}

/// Force through logs, traces and metrics, useful in e.g. testing.
///
/// Note there doesn't seem to be an underlying interface to force through metrics.
pub fn flush() -> Result<(), AnyErr> {
    get_global()?.flush()
}

/// Shutdown the logger, traces and metrics, should be called when the program is about to exit.
pub fn shutdown() -> Result<(), AnyErr> {
    get_global()?.shutdown()
}

fn get_global<'a>() -> Result<MappedMutexGuard<'a, GlobalLog>, AnyErr> {
    if GLOBAL_LOG.lock().is_none() {
        return Err(anyerr!("GlobalLog not registered!"));
    }
    Ok(MutexGuard::map(GLOBAL_LOG.lock(), |x| x.as_mut().unwrap()))
}
