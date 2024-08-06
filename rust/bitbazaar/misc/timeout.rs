use std::future::Future;

use chrono::TimeDelta;
use futures::FutureExt;

use crate::misc::sleep_compat;

/// Run the future with the given timeout,
/// returning the result of the closure and cancelling the future if it takes too long.
///
/// Arguments:
/// - `timeout`: The maximum time to wait for the future to complete.
/// - `on_timeout`: A closure that will be called if the future times out, this value will return instead.
/// - `fut`: The future to run.
pub async fn with_timeout<R, E, Fut: Future<Output = Result<R, E>>>(
    timeout: TimeDelta,
    on_timeout: impl FnOnce() -> Result<R, E>,
    fut: Fut,
) -> Result<R, E> {
    futures::select! {
        result = {fut.fuse()} => result,
        _ = {sleep_compat(timeout).fuse()} => {
            on_timeout()
        }
    }
}
