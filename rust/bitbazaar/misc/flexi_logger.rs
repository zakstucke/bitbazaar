use std::{fmt::Debug, future::Future, sync::Arc};

use parking_lot::Mutex;

/// An interface that can be used to track arbitrary logging "flows".
pub trait FlexiLog: std::fmt::Debug + Send + Sync {
    /// The actual object used to apply logs.
    type Writer: FlexiLogWriter;

    /// Can be used to synchronously batch together multiple changes.
    fn batch(&self, cb: impl FnOnce(&mut Self::Writer) + Send + 'static);

    /// Get the current phase. Just always return e,g, Pending if not supported.
    /// Async as it's a getter, can't spawn off async tasks like the setters if underlying is async.
    fn phase(&self) -> impl Future<Output = FlexiLogPhase> + Send;

    /// Set the current progress.
    /// Just return 0.0 if not supported.
    /// Async as it's a getter, can't spawn off async tasks like the setters if underlying is async.
    fn progress(&self) -> impl Future<Output = f64> + Send;

    /// Set the current progress. Depending on the underlying imp this could e.g. log a message, or write a progress bar etc.
    fn set_progress(&self, progress: f64) {
        self.batch(move |w| w.set_progress(progress))
    }

    /// Set the current phase. The underlying imp might not do anything for phases.
    fn set_phase(&self, phase: FlexiLogPhase) {
        self.batch(move |w| w.set_phase(phase))
    }

    /// Underlying log adder with more options.
    /// If the actual output doesn't support force_replace_prior, just ignore it and add like normal. (Same with lvl.)
    fn log_with_opts(&self, lvl: tracing::Level, msg: String, force_replace_prior: bool) {
        self.batch(move |w| w.log_with_opts(lvl, msg, force_replace_prior))
    }

    /// Log a debug message.
    fn log_debug(&self, msg: impl Into<String>) {
        let msg = msg.into();
        self.batch(move |w| w.log_debug(msg))
    }

    /// Log an info message.
    fn log_info(&self, msg: impl Into<String>) {
        let msg = msg.into();
        self.batch(move |w| w.log_info(msg))
    }

    /// Log a warning message.
    fn log_warn(&self, msg: impl Into<String>) {
        let msg = msg.into();
        self.batch(move |w| w.log_warn(msg))
    }

    /// Log an error message.
    fn log_error(&self, msg: impl Into<String>) {
        let msg = msg.into();
        self.batch(move |w| w.log_error(msg))
    }

    /// Set the state of the flow to failed.
    fn set_failed(&self) {
        self.batch(move |w| w.set_failed())
    }

    /// Set the state of the flow to completed.
    fn set_completed(&self) {
        self.batch(move |w| w.set_completed())
    }

    /// Set the state of the flow to running.
    fn set_running(&self) {
        self.batch(move |w| w.set_running())
    }

    /// Set the state of the flow to failed pending retry.
    fn set_failed_pending_retry(&self, scheduled_for: chrono::DateTime<chrono::Utc>) {
        self.batch(move |w| w.set_failed_pending_retry(scheduled_for))
    }
}

/// The actual object used to apply logs.
/// This is useful when updates are async, making synchronous batching easy.
pub trait FlexiLogWriter: std::fmt::Debug {
    /// Get the current phase. Just always return e,g, Pending if not supported.
    fn phase(&self) -> FlexiLogPhase;

    /// Set the current progress.
    /// Just return 0.0 if not supported.
    fn progress(&self) -> f64;

    /// Set the current progress.
    /// Depending on the underlying imp this could be a noop or e.g. log a message, or write a progress bar etc.
    fn set_progress(&self, progress: f64);

    /// Set the current phase. The underlying imp might not do anything for phases.
    fn set_phase(&self, phase: FlexiLogPhase);

    /// Underlying log adder with more options.
    /// If the actual output doesn't support force_replace_prior, just ignore it and add like normal. (Same with lvl.)
    fn log_with_opts(&self, lvl: tracing::Level, msg: String, force_replace_prior: bool);

    /// Log a debug message.
    fn log_debug(&self, msg: impl Into<String>) {
        self.log_with_opts(tracing::Level::DEBUG, msg.into(), false)
    }

    /// Log an info message.
    fn log_info(&self, msg: impl Into<String>) {
        self.log_with_opts(tracing::Level::INFO, msg.into(), false)
    }

    /// Log a warning message.
    fn log_warn(&self, msg: impl Into<String>) {
        self.log_with_opts(tracing::Level::WARN, msg.into(), false)
    }

    /// Log an error message.
    fn log_error(&self, msg: impl Into<String>) {
        self.log_with_opts(tracing::Level::ERROR, msg.into(), false)
    }

    /// Set the state of the flow to failed.
    fn set_failed(&self) {
        self.set_phase(FlexiLogPhase::Failed {
            started_at: self.phase().started_at(),
            finished_at: chrono::Utc::now(),
        })
    }

    /// Set the state of the flow to completed.
    fn set_completed(&self) {
        self.set_phase(FlexiLogPhase::Completed {
            started_at: self.phase().started_at(),
            finished_at: chrono::Utc::now(),
        })
    }

    /// Set the state of the flow to running.
    fn set_running(&self) {
        self.set_phase(FlexiLogPhase::Running {
            // Note not maintaining previous's phases started at,
            // started at should be the start of the flow's processing, forgetting about previous failed attempts:
            started_at: chrono::Utc::now(),
        })
    }

    /// Set the state of the flow to failed pending retry.
    fn set_failed_pending_retry(&self, scheduled_for: chrono::DateTime<chrono::Utc>) {
        let cur_phase = self.phase();
        self.set_phase(FlexiLogPhase::FailedPendingRetry {
            started_at: cur_phase.started_at(),
            num_tries_done: match cur_phase {
                FlexiLogPhase::FailedPendingRetry { num_tries_done, .. } => num_tries_done + 1,
                _ => 1,
            },
            finished_at: chrono::Utc::now(),
            scheduled_for,
        })
    }
}

#[cfg(feature = "redis")]
/// Lots of loggers are backed by redis.
/// A higher-order trait to produce a logger once redis is available.
pub trait FlexiLogFromRedis: std::fmt::Debug {
    /// The type of the flexi logger.
    type FlexiLogger: FlexiLog;

    /// Create a new flexi logger from a redis connection.
    fn into_flexi_log(self, redis: &crate::redis::Redis) -> Self::FlexiLogger;
}

/// Useful for phase tracking, depending on the logger.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
pub enum FlexiLogPhase {
    /// The flow is pending, not started yet.
    Pending,
    /// The flow is running.
    Running {
        /// When the flow actually started running.
        started_at: chrono::DateTime<chrono::Utc>,
    },
    /// The flow is completed.
    Completed {
        /// When the flow actually started running.
        started_at: chrono::DateTime<chrono::Utc>,
        /// When the flow succeeded.
        finished_at: chrono::DateTime<chrono::Utc>,
    },
    /// The flow failed.
    Failed {
        /// When the flow actually started running.
        started_at: chrono::DateTime<chrono::Utc>,
        /// When the flow failed.
        finished_at: chrono::DateTime<chrono::Utc>,
    },
    /// The flow failed, but is pending a retry.
    FailedPendingRetry {
        /// When the flow actually started running.
        started_at: chrono::DateTime<chrono::Utc>,
        /// When the flow failed.
        finished_at: chrono::DateTime<chrono::Utc>,
        /// E.g. if the run failed, this will be 1.
        num_tries_done: usize,
        /// When the flow is scheduled to re-run at.
        scheduled_for: chrono::DateTime<chrono::Utc>,
    },
}

impl FlexiLogPhase {
    /// True when completed or failed, so safe to delete. (failed pending retry isn't safe)
    pub fn is_finished(&self) -> bool {
        match self {
            FlexiLogPhase::Completed { .. } | FlexiLogPhase::Failed { .. } => true,
            FlexiLogPhase::FailedPendingRetry { .. }
            | FlexiLogPhase::Pending
            | FlexiLogPhase::Running { .. } => false,
        }
    }

    /// Get the elapsed time of the flow, will be None if not started yet.
    pub fn elapsed(&self) -> Option<chrono::Duration> {
        match self {
            FlexiLogPhase::Pending => None,
            FlexiLogPhase::Running { started_at } => Some(chrono::Utc::now() - *started_at),
            FlexiLogPhase::Completed {
                started_at,
                finished_at,
                ..
            }
            | FlexiLogPhase::Failed {
                started_at,
                finished_at,
                ..
            }
            // Still makes sense for this, effectively saying the flow took this long, but will reset when it tries again.
            | FlexiLogPhase::FailedPendingRetry {
                started_at,
                finished_at,
                ..
            } => Some(*finished_at - *started_at),
        }
    }

    /// Useful internally for transitioning phase.
    fn started_at(&self) -> chrono::DateTime<chrono::Utc> {
        match self {
            FlexiLogPhase::Pending => chrono::Utc::now(),
            FlexiLogPhase::Running { started_at } => *started_at,
            FlexiLogPhase::Completed { started_at, .. }
            | FlexiLogPhase::Failed { started_at, .. } => *started_at,
            FlexiLogPhase::FailedPendingRetry { started_at, .. } => *started_at,
        }
    }
}

impl FlexiLogWriter for () {
    fn phase(&self) -> FlexiLogPhase {
        FlexiLogPhase::Pending
    }

    fn progress(&self) -> f64 {
        0.0
    }

    fn set_progress(&self, _progress: f64) {}

    fn set_phase(&self, _phase: FlexiLogPhase) {}

    fn log_with_opts(&self, _lvl: tracing::Level, _msg: String, _force_replace_prior: bool) {}
}

impl FlexiLog for () {
    type Writer = ();

    fn batch(&self, _cb: impl FnOnce(&mut Self::Writer)) {}

    async fn phase(&self) -> FlexiLogPhase {
        FlexiLogPhase::Pending
    }

    async fn progress(&self) -> f64 {
        0.0
    }
}

#[cfg(feature = "redis")]
impl FlexiLogFromRedis for () {
    type FlexiLogger = ();

    fn into_flexi_log(self, _: &crate::redis::Redis) -> Self::FlexiLogger {}
}

impl<W: FlexiLogWriter, T: FlexiLog<Writer = W>> FlexiLog for Option<T> {
    type Writer = W;

    fn batch(&self, cb: impl FnOnce(&mut Self::Writer) + Send + 'static) {
        if let Some(inner) = self {
            inner.batch(cb);
        }
    }

    async fn phase(&self) -> FlexiLogPhase {
        if let Some(inner) = self {
            inner.phase().await
        } else {
            FlexiLogPhase::Pending
        }
    }

    async fn progress(&self) -> f64 {
        if let Some(inner) = self {
            inner.progress().await
        } else {
            0.0
        }
    }
}

/// Flexi logging bridge to wrap arbitrary flexi logging implementers, doesn't require any generics.
pub struct FlexiLogBridge {
    phase: Mutex<FlexiLogPhase>,
    progress: Mutex<f64>,
    on_batch: Box<dyn Fn(&mut FlexiLogBridgeWriter) + Send + Sync>,
}

impl FlexiLogBridge {
    /// Create a new flexi log bridge.
    pub async fn new(flexilog: impl FlexiLog + 'static) -> Self {
        let flexilog = Arc::new(flexilog);
        Self {
            phase: Mutex::new(flexilog.phase().await),
            progress: Mutex::new(flexilog.progress().await),
            on_batch: Box::new(move |bridge| {
                let mut updates = bridge.inner.lock();
                let phase = updates.phase.clone();
                let logs = std::mem::take(&mut updates.logs);
                let progress = updates.progress;
                let flexilog = flexilog.clone();
                flexilog.batch(move |inner| {
                    if phase != inner.phase() {
                        inner.set_phase(phase);
                    }
                    if let Some(progress) = progress {
                        inner.set_progress(progress);
                    }
                    for (lvl, msg, force_replace_prior) in logs {
                        inner.log_with_opts(lvl, msg, force_replace_prior);
                    }
                });
            }),
        }
    }
}

impl Debug for FlexiLogBridge {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FlexiLogBridge").finish()
    }
}

impl FlexiLog for FlexiLogBridge {
    type Writer = FlexiLogBridgeWriter;

    fn batch(&self, cb: impl FnOnce(&mut Self::Writer)) {
        let mut writer = FlexiLogBridgeWriter {
            inner: Mutex::new(FlexiLogBridgeWriterInner {
                phase: self.phase.lock().clone(),
                progress: None,
                logs: Vec::new(),
            }),
        };
        cb(&mut writer);
        *self.phase.lock() = writer.phase();
        *self.progress.lock() = writer.progress();
        (self.on_batch)(&mut writer);
    }

    async fn phase(&self) -> FlexiLogPhase {
        self.phase.lock().clone()
    }

    async fn progress(&self) -> f64 {
        *self.progress.lock()
    }
}

impl<'a, F: FlexiLog> FlexiLog for &'a F {
    type Writer = F::Writer;

    fn batch(&self, cb: impl FnOnce(&mut Self::Writer) + Send + 'static) {
        (*self).batch(cb)
    }

    async fn phase(&self) -> FlexiLogPhase {
        (*self).phase().await
    }

    async fn progress(&self) -> f64 {
        (*self).progress().await
    }
}

/// Flexi logging bridge writer, doesn't require any generics.
pub struct FlexiLogBridgeWriter {
    inner: Mutex<FlexiLogBridgeWriterInner>,
}

impl Debug for FlexiLogBridgeWriter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FlexiLogBridgeWriter").finish()
    }
}

impl FlexiLogWriter for FlexiLogBridgeWriter {
    fn phase(&self) -> FlexiLogPhase {
        self.inner.lock().phase.clone()
    }

    fn progress(&self) -> f64 {
        self.inner.lock().progress.unwrap_or(0.0)
    }

    fn set_progress(&self, progress: f64) {
        self.inner.lock().progress = Some(progress);
    }

    fn set_phase(&self, phase: FlexiLogPhase) {
        self.inner.lock().phase = phase;
    }

    fn log_with_opts(&self, lvl: tracing::Level, msg: String, force_replace_prior: bool) {
        self.inner.lock().logs.push((lvl, msg, force_replace_prior));
    }
}

struct FlexiLogBridgeWriterInner {
    phase: FlexiLogPhase,
    // Options to not update if not changed:
    progress: Option<f64>,
    logs: Vec<(tracing::Level, String, bool)>,
}

#[cfg(test)]
mod tests {
    use std::{ops::Deref, sync::OnceLock};

    use super::*;

    #[tokio::test]
    async fn test_flexi_bridge() {
        #[derive(Debug, Clone)]
        struct LogBackend {
            inner: Arc<Mutex<LogBackendInner>>,
        }

        #[derive(Debug)]
        struct LogBackendInner {
            logs: Mutex<Vec<(tracing::Level, String, bool)>>,
            phase: Mutex<FlexiLogPhase>,
            progress: Mutex<f64>,
        }

        impl FlexiLogWriter for LogBackendInner {
            fn phase(&self) -> FlexiLogPhase {
                self.phase.lock().clone()
            }

            fn progress(&self) -> f64 {
                *self.progress.lock()
            }

            fn set_progress(&self, progress: f64) {
                *self.progress.lock() = progress;
            }

            fn set_phase(&self, phase: FlexiLogPhase) {
                *self.phase.lock() = phase;
            }

            fn log_with_opts(&self, lvl: tracing::Level, msg: String, force_replace_prior: bool) {
                self.logs.lock().push((lvl, msg, force_replace_prior));
            }
        }

        impl FlexiLog for LogBackend {
            type Writer = LogBackendInner;

            fn batch(&self, cb: impl FnOnce(&mut Self::Writer)) {
                let mut inner = self.inner.lock();
                cb(&mut inner);
            }

            async fn phase(&self) -> FlexiLogPhase {
                self.inner.lock().phase.lock().clone()
            }

            async fn progress(&self) -> f64 {
                *self.inner.lock().progress.lock()
            }
        }

        let backend = LogBackend {
            inner: Arc::new(Mutex::new(LogBackendInner {
                logs: Mutex::new(Vec::new()),
                phase: Mutex::new(FlexiLogPhase::Pending),
                progress: Mutex::new(0.0),
            })),
        };

        let bridge = FlexiLogBridge::new(backend.clone()).await;

        let running_started_at = Arc::new(OnceLock::new());
        {
            let running_started_at = running_started_at.clone();
            bridge.batch(move |w| {
                let started_at = chrono::Utc::now();
                let _ = running_started_at.set(started_at);

                w.set_progress(0.5);
                w.set_phase(FlexiLogPhase::Running { started_at });
                w.log_info("Hello world");
                w.log_warn("foo");
            });
        }

        let inner = backend.inner.lock();
        assert_eq!(inner.logs.lock().len(), 2);
        assert_eq!(*inner.progress.lock().deref(), 0.5);
        assert_eq!(inner.progress(), 0.5);
        assert_eq!(
            inner.logs.lock()[0],
            (tracing::Level::INFO, "Hello world".to_string(), false)
        );
        assert_eq!(
            inner.logs.lock()[1],
            (tracing::Level::WARN, "foo".to_string(), false)
        );
        assert_eq!(
            *inner.phase.lock().deref(),
            FlexiLogPhase::Running {
                started_at: *running_started_at.get().unwrap()
            }
        );
    }
}
