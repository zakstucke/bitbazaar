use std::time::Duration;

use futures::Future;
use itertools::Either;

/// Will attempt execute the fn's returned future according to the entered spec.
///
/// # Arguments
/// * `retry_delays` - The delays between each retry, this also specifies how many retries to attempt.
/// * `last_delay_repeat_times` - The number of times the last delay should be repeated, providing access to pseudo-infinite retries.
/// * `fallible` - The function that will be executed.
/// * `on_retry` - The function that will be executed each time the fallible function fails and a new one is pending. If clear won't succeed, return Some(E) to indicate should raise with this error and not continue retrying.
pub async fn retry_backoff<R, E, Fut: Future<Output = Result<R, E>>>(
    retry_delays: &[Duration],
    last_delay_repeat_times: Option<usize>,
    fallible: impl Fn() -> Fut,
    on_retry: impl Fn(RetryBackoffInfo<E>) -> Option<E>,
) -> Result<R, E> {
    let total_attempts = retry_delays.len() + 1 + last_delay_repeat_times.unwrap_or(0);
    for (attempt_index, delay) in std::iter::once(&Duration::from_secs(0))
        .chain(retry_delays.iter())
        .chain(
            if let Some(last_delay_repeat_times) = last_delay_repeat_times {
                Either::Left(
                    std::iter::repeat(retry_delays.last().unwrap()).take(last_delay_repeat_times),
                )
            } else {
                Either::Right(std::iter::empty())
            },
        )
        .enumerate()
    {
        if delay.as_nanos() > 0 {
            tokio::time::sleep(*delay).await;
        }
        match fallible().await {
            Ok(r) => return Ok(r),
            Err(e) => {
                if attempt_index + 1 == total_attempts {
                    return Err(e);
                }
                // Call the on_retry function, if it returns Some(E) then return that error, exiting early.
                if let Some(e) = on_retry(RetryBackoffInfo {
                    last_error: e,
                    last_attempt_no: attempt_index + 1,
                    delay_till_next_attempt: *delay,
                }) {
                    return Err(e);
                }
            }
        }
    }
    unreachable!()
}

/// Information about the last retry attempt.
pub struct RetryBackoffInfo<E> {
    /// The error that caused the last attempt to fail.
    pub last_error: E,
    /// The number of the last attempt. E.g. first attempt failing would be 1.
    pub last_attempt_no: usize,
    /// The delay until the next attempt.
    pub delay_till_next_attempt: Duration,
}
