use std::sync::{atomic::AtomicU64, Arc};

use futures::Future;
use parking_lot::Mutex;

use crate::prelude::*;

/// A data wrapper that automatically updates the data given out when deemed stale.
/// The data is set to refresh at a certain interval (triggered on access), or can be forcefully refreshed.
pub struct Refreshable<T, Fut: Future<Output = RResult<T, AnyErr>>, F: Fn() -> Fut> {
    // Don't want to hold lock when giving out data, so opposite to normal pattern:
    data: Mutex<Arc<T>>,
    getter: F,
    last_updated_utc_ms: AtomicU64,
    update_every_ms: u64,
}

impl<T, Fut: Future<Output = RResult<T, AnyErr>>, F: Fn() -> Fut> Refreshable<T, Fut, F> {
    /// Creates a new refreshable data wrapper.
    ///
    /// Arguments:
    /// - `update_every`: The interval at which the data should be refreshed. Will only actually trigger on attempted access.
    /// - `getter`: A function that returns a future that resolves to the data.
    pub async fn new(update_every: std::time::Duration, getter: F) -> RResult<Self, AnyErr> {
        let data = getter().await?;
        Ok(Self {
            data: Mutex::new(Arc::new(data)),
            getter,
            last_updated_utc_ms: AtomicU64::new(utc_now_ms()),
            update_every_ms: update_every.as_millis() as u64,
        })
    }

    /// Force a refresh of the data.
    pub async fn force_refresh(&self) -> RResult<(), AnyErr> {
        let new_data = (self.getter)().await?;
        self.last_updated_utc_ms
            .store(utc_now_ms(), std::sync::atomic::Ordering::Relaxed);
        *self.data.lock() = Arc::new(new_data);
        Ok(())
    }

    /// Get the underlying data for use.
    /// If the data is stale, it will be refreshed before returning.
    pub async fn get(&self) -> RResult<Arc<T>, AnyErr> {
        let now = utc_now_ms();
        let last_updated = self
            .last_updated_utc_ms
            .load(std::sync::atomic::Ordering::Relaxed);

        // Refresh if now stale:
        let replacement_data = if now - last_updated > self.update_every_ms {
            let new_data = (self.getter)().await?;
            self.last_updated_utc_ms
                .store(now, std::sync::atomic::Ordering::Relaxed);
            Some(new_data)
        } else {
            None
        };

        // Temporarily lock to access:
        let mut data = self.data.lock();
        if let Some(new_data) = replacement_data {
            *data = Arc::new(new_data);
        }
        Ok(data.clone())
    }
}

fn utc_now_ms() -> u64 {
    chrono::Utc::now().timestamp_millis() as u64
}
