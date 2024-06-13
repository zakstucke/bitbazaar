use futures::{Future, FutureExt};

/// An interface that can be used to track arbitrary logging "flows".
pub trait FlexiLog: std::fmt::Debug {
    /// Set the current progress. Depending on the underlying imp this could e.g. log a message, or update a progress bar etc.
    fn set_progress(&self, progress: f64) -> impl Future<Output = ()> + Send;

    /// Underlying log adder with more options.
    /// If the actual output doesn't support force_replace_prior, just ignore it and add like normal. (Same with lvl.)
    fn log_with_opts(
        &self,
        lvl: tracing::Level,
        msg: String,
        force_replace_prior: bool,
    ) -> impl Future<Output = ()> + Send;

    /// Log a debug message.
    fn log_debug(&self, msg: impl Into<String>) -> impl Future<Output = ()> + Send {
        let msg = msg.into();
        self.log_with_opts(tracing::Level::DEBUG, msg, false)
    }

    /// Log an info message.
    fn log_info(&self, msg: impl Into<String>) -> impl Future<Output = ()> + Send {
        let msg = msg.into();
        self.log_with_opts(tracing::Level::INFO, msg, false)
    }

    /// Log a warning message.
    fn log_warn(&self, msg: impl Into<String>) -> impl Future<Output = ()> + Send {
        let msg = msg.into();
        self.log_with_opts(tracing::Level::WARN, msg, false)
    }

    /// Log an error message.
    fn log_error(&self, msg: impl Into<String>) -> impl Future<Output = ()> + Send {
        let msg = msg.into();
        self.log_with_opts(tracing::Level::ERROR, msg, false)
    }
}

/// Means can be used when no logging wanted.
impl FlexiLog for () {
    async fn set_progress(&self, _progress: f64) {}

    async fn log_with_opts(&self, _lvl: tracing::Level, _msg: String, _force_replace_prior: bool) {}
}

impl<T: FlexiLog> FlexiLog for Option<T> {
    fn set_progress(&self, progress: f64) -> impl Future<Output = ()> + Send {
        if let Some(inner) = self {
            inner.set_progress(progress).left_future()
        } else {
            async {}.right_future()
        }
    }

    fn log_with_opts(
        &self,
        lvl: tracing::Level,
        msg: String,
        force_replace_prior: bool,
    ) -> impl Future<Output = ()> + Send {
        if let Some(inner) = self {
            inner
                .log_with_opts(lvl, msg, force_replace_prior)
                .left_future()
        } else {
            async {}.right_future()
        }
    }
}
