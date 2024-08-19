use crate::misc::Retry;

pub(crate) fn redis_retry_config() -> Retry<'static, redis::RedisError> {
    Retry::<redis::RedisError>::fibonacci(chrono::Duration::milliseconds(10))
        // Will cumulatively delay for up to about 5ish seconds.
        // SHOULDN'T BE LONGER, considering downstream user code may then handle the redis failure,
        // in e.g. web request, any longer would then be harmful.
        .until_total_delay(chrono::Duration::seconds(5))
            .on_retry(move |info| match info.last_error.kind() {
                // These should all be automatically retried:
                redis::ErrorKind::BusyLoadingError
                | redis::ErrorKind::TryAgain
                | redis::ErrorKind::MasterDown => {
                    tracing::warn!(
                        "Redis action failed with retryable error, retrying in {}. Last attempt no: '{}'.\nErr:\n{:?}.",
                        info.delay_till_next_attempt,
                        info.last_attempt_no,
                        info.last_error
                    );
                    None
                },
                // Everything else should just exit straight away, no point retrying internally.
                _ => Some(info.last_error),
            })
}
