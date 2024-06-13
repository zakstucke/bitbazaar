use std::{
    sync::atomic::AtomicU64,
    time::{SystemTime, UNIX_EPOCH},
};

/// A periodic updater. Run a callback at a specified time interval. Synchronous. Requires polling.
/// Useful in long running loops to do something at a specified time interval. [`PeriodicUpdater::maybe_update`] should be called during each loop.
pub struct PeriodicUpdater<A, F: Fn(std::time::Duration, A)> {
    last_timestamp_ms: AtomicU64,
    update_every: std::time::Duration,
    on_progress: F,
    _a: std::marker::PhantomData<A>,
}

impl<A, F: Fn(std::time::Duration, A)> PeriodicUpdater<A, F> {
    /// Create a new PeriodicUpdater.
    /// Make sure to call [`PeriodicUpdater::maybe_update`] frequently.
    ///
    /// Arguments:
    /// - `update_every`: The time interval at which to run the callback.
    /// - `on_progress`: The callback to run. Is passed the exact time since the last call, and any other params externally configured.
    pub fn new(update_every: std::time::Duration, on_progress: F) -> Self {
        Self {
            on_progress,
            last_timestamp_ms: AtomicU64::new(0),
            update_every,
            _a: std::marker::PhantomData,
        }
    }

    /// Call this function frequently to check if the callback should be run.
    pub fn maybe_update(&self, ext_params: A) {
        let epoch_ms = get_epoch_ms();
        let elapsed = std::time::Duration::from_millis(
            epoch_ms
                - self
                    .last_timestamp_ms
                    .load(std::sync::atomic::Ordering::Relaxed),
        );
        if elapsed >= self.update_every {
            (self.on_progress)(elapsed, ext_params);
            self.last_timestamp_ms
                .store(epoch_ms, std::sync::atomic::Ordering::Relaxed);
        }
    }
}

fn get_epoch_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64
}
