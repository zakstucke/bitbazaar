use std::sync::{atomic::AtomicU64, Arc, OnceLock};

use arc_swap::ArcSwap;
use chrono::TimeDelta;
use futures::{
    future::{BoxFuture, LocalBoxFuture},
    Future, FutureExt,
};

pub use arc_swap::Guard as RefreshableGuard;

use crate::{
    prelude::*,
    redis::{Redis, RedisBatchFire, RedisBatchReturningOps, RedisConnLike},
};

// TODO for both, maybe some lock which is held during a refresh, to prevent copies of this all calling refresh next to each other if called whilst a refresh is going on.
// TODO some testing for this too.

macro_rules! impl_refreshable_write_synced {
    ($name:ident, $box_method:ident, $box_future:ident, [$($cb_reqs:tt)*], [$($fut_reqs:tt)*]) => {
        /// A data wrapper that automatically updates the data given out when deemed stale.
        /// The data is set to refresh at a certain interval (triggered on access), or can be forcefully refreshed.
        pub struct $name<T: Clone> {
            redis: Redis,
            redis_namespace: String,
            redis_key: String,
            redis_mutate_key: String,
            mutate_id: AtomicU64,
            // Don't want to hold lock when giving out data, so opposite to normal pattern:
            data: OnceLock<ArcSwap<T>>,
            // To prevent stopping Send/Sync working for this struct, these need to be both:
            getter: Arc<Box<dyn Fn() -> $box_future<'static, RResult<T, AnyErr>> $($cb_reqs)*>>,
            setter: Arc<Box<dyn Fn(T) -> $box_future<'static, RResult<(), AnyErr>> $($cb_reqs)*>>,
            on_mutate: Option<Arc<Box<dyn Fn(&mut T) -> RResult<(), AnyErr> + 'static $($cb_reqs)*>>>,
            last_updated_utc_ms: AtomicU64,
            force_refresh_every_ms: u64,
        }

        impl<T: Clone> $name<T> {
            /// Creates a new refreshable data wrapper.
            /// This will only call the getter on first access, not on instanstiation.
            ///
            /// Arguments:
            /// - `redis`: The redis wrapper itself needed for dlocking.
            /// - `redis_namespace`: The namespace to use for the redis key when locking during setting.
            /// - `redis_key`: The key to use for the redis key when locking during setting.
            /// - `force_refresh_every`: The interval for forceful data should be refreshed. For when something other than a `Refreshable` container updates, but still good as a backup.
            /// - `getter`: A function that returns a future that resolves to the data.
            /// - `setter`: A function that updates the source with new data.
            pub fn new<
                FutGet: Future<Output = RResult<T, AnyErr>> $($fut_reqs)* + 'static,
                FutSet: Future<Output = RResult<(), AnyErr>> $($fut_reqs)* + 'static,
            >(
                redis: &Redis,
                redis_namespace: impl Into<String>,
                redis_key: impl Into<String>,
                force_refresh_every: TimeDelta,
                getter: impl Fn() -> FutGet + 'static $($cb_reqs)*,
                setter: impl Fn(T) -> FutSet + 'static $($cb_reqs)*,
            ) -> RResult<Self, AnyErr> {
                let redis_key = redis_key.into();
                Ok(Self {
                    redis: redis.clone(),
                    redis_namespace: redis_namespace.into(),
                    redis_mutate_key: format!("{}_mutater", redis_key),
                    redis_key,
                    mutate_id: AtomicU64::new(0),
                    data: OnceLock::new(),
                    getter: Arc::new(Box::new(move || getter().$box_method())),
                    setter: Arc::new(Box::new(move |data| setter(data).$box_method())),
                    on_mutate: None,
                    last_updated_utc_ms: AtomicU64::new(utc_now_ms()),
                    force_refresh_every_ms: force_refresh_every.num_milliseconds() as u64,
                })
            }

            /// Do something whenever a mutation happens, useful to hook in other mutations while a mut self is available.
            pub fn on_mutate(
                mut self,
                on_mutate: impl Fn(&mut T) -> RResult<(), AnyErr> + 'static $($cb_reqs)*,
            ) -> Self {
                self.on_mutate = Some(Arc::new(Box::new(on_mutate)));
                self
            }

            /// Update stored data:
            async fn set_data(&self, new_data: T) -> RResult<(), AnyErr> {
                self.last_updated_utc_ms
                    .store(utc_now_ms(), std::sync::atomic::Ordering::Relaxed);
                self.data().await?.store(Arc::new(new_data));
                Ok(())
            }

            /// Internal of `sync`, doesn't set the new data so can be used in mutator.
            async fn sync_no_set(&self, conn: &impl RedisConnLike) -> RResult<Option<T>, AnyErr> {
                let mutate_id_changed = {
                    if let Some(current_mutate_id) = conn
                        .batch()
                        .get::<u64>(&self.redis_namespace, &self.redis_mutate_key)
                        .fire()
                        .await
                        .flatten()
                    {
                        // Check if different, simultaneously setting the new value:
                        current_mutate_id
                            != self
                                .mutate_id
                                .swap(current_mutate_id, std::sync::atomic::Ordering::Relaxed)
                    } else {
                        false
                    }
                };
                if mutate_id_changed
                    || utc_now_ms()
                        - self
                            .last_updated_utc_ms
                            .load(std::sync::atomic::Ordering::Relaxed)
                        > self.force_refresh_every_ms
                {
                    Ok(Some((self.getter)().await?))
                } else {
                    Ok(None)
                }
            }

            /// Resyncs the data with the source, when it becomes stale (hard refresh), or redis mutate id changes, meaning a different node has updated the data.
            async fn sync(&self, conn: &impl RedisConnLike) -> RResult<(), AnyErr> {
                if let Some(new_data) = self.sync_no_set(conn).await? {
                    self.set_data(new_data).await?;
                }
                Ok(())
            }

            /// Access the currently stored data, initializing the `OnceLock` if empty.
            async fn data(&self) -> RResult<&ArcSwap<T>, AnyErr> {
                if let Some(val) = self.data.get() {
                    Ok(val)
                } else {
                    let new_data = (self.getter)().await?;
                    let _ = self.data.set(ArcSwap::from(Arc::new(new_data)));
                    Ok(self
                        .data
                        .get()
                        .ok_or_else(|| anyerr!("Failed to set data"))?)
                }
            }

            /// Update the data in the refreshable with key features:
            /// - Locks the source using a redis dlock for the duration of the update.
            /// - Refreshes the data before the update inside the locked section,
            ///   to make sure you're doing the update on the latest data and not overwriting changes.
            /// - Updates the setter, thanks to above guaranteed no sibling node overwrites etc.
            ///
            /// NOTE: returns a double result to allow custom internal error types to be passed out.
            pub async fn mutate<R, E>(
                &self,
                conn: &impl RedisConnLike,
                mutator: impl FnOnce(&mut T) -> Result<R, E>,
            ) -> RResult<Result<R, E>, AnyErr> {
                self.redis
                    .dlock_for_fut(
                        &self.redis_namespace,
                        &self.redis_key,
                        // Really don't want to miss updates:
                        Some(TimeDelta::seconds(30)),
                        async move {
                            // Make sure working with up-to-date data:
                            let mut data = if let Some(data) = self.sync_no_set(conn).await? {
                                data
                            } else {
                                (**self.data().await?.load()).clone()
                            };
                            // Mutate the up-to-date data:
                            match mutator(&mut data) {
                                Ok(result) => {
                                    // Run the on_mutate hook if there:
                                    if let Some(on_mutate) = &self.on_mutate {
                                        on_mutate(&mut data)?;
                                    }

                                    // Update the source with the new data:
                                    (self.setter)(data.clone()).await?;
                                    // Update the mutate id to signal to other nodes that the data has changed:
                                    let new_mutate_id = rand::random();
                                    conn.batch()
                                        .set(
                                            &self.redis_namespace,
                                            &self.redis_mutate_key,
                                            new_mutate_id,
                                            None,
                                        )
                                        .fire()
                                        .await;
                                    self.mutate_id
                                        .store(new_mutate_id, std::sync::atomic::Ordering::Relaxed);
                                    self.set_data(data).await?;
                                    Ok::<_, Report<AnyErr>>(Ok(result))
                                }
                                Err(e) => Ok(Err(e)),
                            }
                        },
                    )
                    .await
                    .change_context(AnyErr)
            }

            /// Force a refresh of the data.
            pub async fn refresh(&self) -> RResult<(), AnyErr> {
                let new_data = (self.getter)().await?;
                self.set_data(new_data).await?;
                Ok(())
            }

            /// Get the underlying data for use.
            /// If the data is stale, it will be refreshed before returning.
            ///
            /// NOTE: the implementation of the guards means not too many should be alive at once, and keeping across await points should be discouraged.
            /// If you need long access to the underlying data, consider cloning it.
            pub async fn get(
                &self,
                conn: &impl RedisConnLike,
            ) -> RResult<RefreshableGuard<Arc<T>>, AnyErr> {
                // Refresh if stale or mutate id in redis changes:
                self.sync(conn).await?;
                Ok(self.data().await?.load())
            }
        }
    };
}

impl_refreshable_write_synced!(RefreshableWriteSynced, boxed, BoxFuture, [+ Send + Sync], [+ Send]);
impl_refreshable_write_synced!(
    RefreshableWriteSyncedLocal,
    boxed_local,
    LocalBoxFuture,
    [],
    []
);

macro_rules! impl_refreshable {
    ($name:ident, $box_method:ident, $box_future:ident, [$($cb_reqs:tt)*], [$($fut_reqs:tt)*]) => {
        /// A data wrapper that automatically updates the data given out when deemed stale.
        /// The data is set to refresh at a certain interval (triggered on access), or can be forcefully refreshed.
        pub struct $name<T: Clone> {
            // Don't want to hold lock when giving out data, so opposite to normal pattern:
            data: OnceLock<ArcSwap<T>>,
            // To prevent stopping Send/Sync working for this struct, these need to be both:
            getter: Arc<Box<dyn Fn() -> $box_future<'static, RResult<T, AnyErr>> $($cb_reqs)*>>,
            on_mutate: Option<Arc<Box<dyn Fn(&mut T) -> RResult<(), AnyErr> + 'static $($cb_reqs)*>>>,
            last_updated_utc_ms: AtomicU64,
            force_refresh_every_ms: u64,
        }

        impl<T: Clone> $name<T> {
            /// Creates a new refreshable data wrapper.
            /// This will only call the getter on first access, not on instanstiation.
            ///
            /// Arguments:
            /// - `force_refresh_every`: The interval for forceful data should be refreshed. For when something other than a `Refreshable` container updates, but still good as a backup.
            /// - `getter`: A function that returns a future that resolves to the data.
            pub fn new<
                FutGet: Future<Output = RResult<T, AnyErr>> $($fut_reqs)* + 'static,
            >(
                force_refresh_every: TimeDelta,
                getter: impl Fn() -> FutGet + 'static $($cb_reqs)*,
            ) -> RResult<Self, AnyErr> {
                Ok(Self {
                    data: OnceLock::new(),
                    getter: Arc::new(Box::new(move || getter().$box_method())),
                    on_mutate: None,
                    last_updated_utc_ms: AtomicU64::new(utc_now_ms()),
                    force_refresh_every_ms: force_refresh_every.num_milliseconds() as u64,
                })
            }

            /// Do something whenever a mutation happens, useful to hook in other mutations while a mut self is available.
            pub fn on_mutate(
                mut self,
                on_mutate: impl Fn(&mut T) -> RResult<(), AnyErr> + 'static $($cb_reqs)*,
            ) -> Self {
                self.on_mutate = Some(Arc::new(Box::new(on_mutate)));
                self
            }

            /// Update stored data:
            async fn set_data(&self, new_data: T) -> RResult<(), AnyErr> {
                self.last_updated_utc_ms
                    .store(utc_now_ms(), std::sync::atomic::Ordering::Relaxed);
                self.data().await?.store(Arc::new(new_data));
                Ok(())
            }

            /// Internal of `sync`, doesn't set the new data so can be used in mutator.
            async fn sync_no_set(&self) -> RResult<Option<T>, AnyErr> {
                if utc_now_ms()
                        - self
                            .last_updated_utc_ms
                            .load(std::sync::atomic::Ordering::Relaxed)
                        > self.force_refresh_every_ms
                {
                    Ok(Some((self.getter)().await?))
                } else {
                    Ok(None)
                }
            }

            /// Resyncs the data with the source, when it becomes stale (hard refresh).
            async fn sync(&self) -> RResult<(), AnyErr> {
                if let Some(new_data) = self.sync_no_set().await? {
                    self.set_data(new_data).await?;
                }
                Ok(())
            }

            /// Access the currently stored data, initializing the `OnceLock` if empty.
            async fn data(&self) -> RResult<&ArcSwap<T>, AnyErr> {
                if let Some(val) = self.data.get() {
                    Ok(val)
                } else {
                    let new_data = (self.getter)().await?;
                    let _ = self.data.set(ArcSwap::from(Arc::new(new_data)));
                    Ok(self
                        .data
                        .get()
                        .ok_or_else(|| anyerr!("Failed to set data"))?)
                }
            }

            /// Update the data in the refreshable.
            ///
            /// NOTE: returns a double result to allow custom internal error types to be passed out.
            pub async fn mutate<R, E>(
                &self,
                mutator: impl FnOnce(&mut T) -> Result<R, E>,
            ) -> RResult<Result<R, E>, AnyErr> {
                async move {
                    // Make sure working with up-to-date data:
                    let mut data = if let Some(data) = self.sync_no_set().await? {
                        data
                    } else {
                        (**self.data().await?.load()).clone()
                    };
                    // Mutate the up-to-date data:
                    match mutator(&mut data) {
                        Ok(result) => {
                            // Run the on_mutate hook if there:
                            if let Some(on_mutate) = &self.on_mutate {
                                on_mutate(&mut data)?;
                            }
                            self.set_data(data).await?;
                            Ok::<_, Report<AnyErr>>(Ok(result))
                        }
                        Err(e) => Ok(Err(e)),
                    }
                }
                .await
                .change_context(AnyErr)
            }

            /// Force a refresh of the data.
            pub async fn refresh(&self) -> RResult<(), AnyErr> {
                let new_data = (self.getter)().await?;
                self.set_data(new_data).await?;
                Ok(())
            }

            /// Mark the data as stale, so it'll be refreshed on next access.
            pub fn mark_stale(&self)  {
                self.last_updated_utc_ms
                    .store(0, std::sync::atomic::Ordering::Relaxed);
            }

            /// Get the underlying data for use.
            /// If the data is stale, it will be refreshed before returning.
            ///
            /// NOTE: the implementation of the guards means not too many should be alive at once, and keeping across await points should be discouraged.
            /// If you need long access to the underlying data, consider cloning it.
            pub async fn get(
                &self,
            ) -> RResult<RefreshableGuard<Arc<T>>, AnyErr> {
                // Refresh if stale or mutate id in redis changes:
                self.sync().await?;
                Ok(self.data().await?.load())
            }
        }
    };
}

impl_refreshable!(Refreshable, boxed, BoxFuture, [+ Send + Sync], [+ Send]);
impl_refreshable!(RefreshableLocal, boxed_local, LocalBoxFuture, [], []);

fn utc_now_ms() -> u64 {
    chrono::Utc::now().timestamp_millis() as u64
}
