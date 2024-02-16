use std::borrow::Cow;

use parking_lot::{MappedMutexGuard, MutexGuard};

use super::{out::GLOBAL_LOG, GlobalLog};
use crate::prelude::*;

#[cfg(feature = "opentelemetry")]
/// Returns a new [Meter] with the provided name and default configuration.
///
/// A [Meter] should be scoped at most to a single application or crate. The
/// name needs to be unique so it does not collide with other names used by
/// an application, nor other applications.
///
/// If the name is empty, then an implementation defined default name will
/// be used instead.
pub fn meter(name: impl Into<Cow<'static, str>>) -> Result<opentelemetry::metrics::Meter, AnyErr> {
    get_global()?.meter(name)
}

#[cfg(feature = "opentelemetry")]
/// Connect this program's span to the trace that is represented by the provided HTTP headers.
/// E.g. connect an axum handler's trace/span to the nginx trace/span.
pub fn set_span_parent_from_http_headers(
    span: &tracing::Span,
    headers: &http::HeaderMap,
) -> Result<(), AnyErr> {
    get_global()?.set_span_parent_from_http_headers(span, headers)
}

#[cfg(feature = "opentelemetry")]
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
