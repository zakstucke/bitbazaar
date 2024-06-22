use std::sync::{atomic::AtomicU64, Arc};

use arc_swap::ArcSwap;
use futures::Future;

use crate::prelude::*;

/// A data wrapper that automatically updates the data given out when deemed stale.
/// The data is set to refresh at a certain interval (triggered on access), or can be forcefully refreshed.
pub struct Refreshable<T, Fut: Future<Output = RResult<T, AnyErr>>, F: Fn() -> Fut> {
    // Don't want to hold lock when giving out data, so opposite to normal pattern:
    data: ArcSwap<T>,
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
            data: ArcSwap::new(Arc::new(data)),
            getter,
            last_updated_utc_ms: AtomicU64::new(utc_now_ms()),
            update_every_ms: update_every.as_millis() as u64,
        })
    }

    /// Update T from outside.
    pub fn set(&self, new_data: T) {
        self.last_updated_utc_ms
            .store(utc_now_ms(), std::sync::atomic::Ordering::Relaxed);
        self.data.store(Arc::new(new_data));
    }

    /// Force a refresh of the data.
    pub async fn refresh(&self) -> RResult<(), AnyErr> {
        let new_data = (self.getter)().await?;
        self.last_updated_utc_ms
            .store(utc_now_ms(), std::sync::atomic::Ordering::Relaxed);
        self.data.store(Arc::new(new_data));
        Ok(())
    }

    /// Get the underlying data for use.
    /// If the data is stale, it will be refreshed before returning.
    ///
    /// NOTE: the implementation of the guards means not too many should be alive at once, and keeping across await points should be discouraged.
    /// If you need long access to the underlying data, consider cloning it.
    pub async fn get(&self) -> RResult<arc_swap::Guard<Arc<T>>, AnyErr> {
        // Refresh if now stale:
        if utc_now_ms()
            - self
                .last_updated_utc_ms
                .load(std::sync::atomic::Ordering::Relaxed)
            > self.update_every_ms
        {
            self.refresh().await?;
        }
        Ok(self.data.load())
    }
}

fn utc_now_ms() -> u64 {
    chrono::Utc::now().timestamp_millis() as u64
}
