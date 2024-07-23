use std::collections::HashMap;
#[cfg(target_arch = "wasm32")]
use std::rc::Rc;
#[cfg(not(target_arch = "wasm32"))]
use std::sync::Arc;

use async_semaphore::Semaphore;
use futures::{
    select,
    stream::{FuturesOrdered, FuturesUnordered},
    Future, FutureExt, StreamExt,
};

use crate::misc::sleep_compat;

macro_rules! batch_futures_flat_impl {
    ($limit:expr, $fut_cbs:expr, |$result:ident| $call_cb:expr) => {{
        let mut return_index = 0;
        let mut result_cache: HashMap<usize, R> = HashMap::new();

        macro_rules! manage_results {
            ($index:expr, $current_result:expr) => {
                // Handle sending a result out if one avail:
                if $index == return_index {
                    // Was the next in line to return, send without touching the cache:
                    return_index += 1; // Must be before callback in-case async!
                    let $result = $current_result;
                    $call_cb
                } else {
                    // Something else needs to be sent next, cache until ready:
                    result_cache.insert($index, $current_result);
                }
                // The next result might be in the cache, drain until we're waiting on something:
                while let Some($result) = result_cache.remove(&return_index) {
                    return_index += 1; // Must be before callback in-case async!
                    $call_cb
                }
            };
        }

        // Can't use FuturesOrdered here, we use the length of the stream to decide when something new can be added, if it were ordered we'd have to wait for specifically the first added to finish before adding more.
        // with unordered, we can add more the moment a permit is available, regardless of the order.
        // the cache is used to maintain ordering.
        let mut stream = FuturesUnordered::new();
        for (index, fut_cb) in $fut_cbs.into_iter().enumerate() {
            stream.push(async move { (index, fut_cb().await) });
            // If full, wait for one to finish.
            if stream.len() >= $limit {
                if let Some((index, result)) = stream.next().await {
                    manage_results!(index, result);
                }
            }
        }

        // Wait for the remaining to finish.
        while let Some((index, result)) = stream.next().await {
            manage_results!(index, result);
        }

        Ok(())
    }};
}

/// A simple and performant runner for an iterator of future creators.
/// The futures are run in parallel up to the supplied limit.
/// The results are returned in the same order as the inputted futures.
///
/// Returns:
/// - Vec of results in the same order as the inputted futures.
pub async fn batch_futures_flat<R, Fut: Future<Output = R>>(
    limit: usize,
    fut_cbs: impl IntoIterator<Item = impl FnOnce() -> Fut>,
) -> Vec<R> {
    let mut results = vec![];
    let _ = batch_futures_flat_stream_sync_cb(limit, fut_cbs, |result| {
        results.push(result);
        Ok::<(), ()>(())
    })
    .await;
    results
}

/// A more performant version of [`batch_futures_descendants`] due to no descendant requirement.
/// By removing descendant limiting, semaphores no longer need to be used.
pub async fn batch_futures_flat_stream_async_cb<
    R,
    Fut: Future<Output = R>,
    E,
    CbFut: Future<Output = Result<(), E>>,
>(
    limit: usize,
    fut_cbs: impl IntoIterator<Item = impl FnOnce() -> Fut>,
    result_sync_cb: impl Fn(R) -> CbFut,
) -> Result<(), E> {
    batch_futures_flat_impl!(limit, fut_cbs, |result| {
        result_sync_cb(result).await?;
    })
}

/// A more performant version of [`batch_futures_descendants`] due to no descendant requirement.
/// By removing descendant limiting, semaphores no longer need to be used.
pub async fn batch_futures_flat_stream_sync_cb<R, Fut: Future<Output = R>, E>(
    limit: usize,
    fut_cbs: impl IntoIterator<Item = impl FnOnce() -> Fut>,
    mut result_sync_cb: impl FnMut(R) -> Result<(), E>,
) -> Result<(), E> {
    batch_futures_flat_impl!(limit, fut_cbs, |result| {
        result_sync_cb(result)?;
    })
}

/// How batched futures are limited, either using a parent's limit to prevent concurrency explosion, or direct.
#[derive(Debug, Clone)]
pub enum BatchLimit {
    /// The entrypoint, where the total limit is set.
    Direct(usize),
    #[cfg(not(target_arch = "wasm32"))]
    /// This level is limited by the parent's config.
    Parent(Arc<async_semaphore::Semaphore>),
    #[cfg(target_arch = "wasm32")]
    /// This level is limited by the parent's config.
    Parent(Rc<async_semaphore::Semaphore>),
}

/// Batch run futures but with a limiter on parents and descendants.
/// IF HIGHEST PERFORMANCE IS NEEDED AND YOU DON'T NEED DESCENDANT LIMITING, USE [`batch_futures_flat_stream_async_cb`] or just normal [`futures::StreamExt::buffer_unordered`].
///
/// Key specialised features:
/// - Limits can be shared with descendants/parents, preventing concurrency explosion, but also internally making sure no deadlocks.
/// - Light on memory, fut_cbs only called when imminently being processed, not requiring all in memory.
/// - Despite batching, vec of results/callbacks of results are in same order as inputted (keeping finished out of order futures in buffer until they're next in line.).
/// - The future callback will only be called when the future will imminently be polled, allowing sync setup.
///
/// If you need to start processing the results before all the futures are done, use [`batch_futures_descendants_stream_sync_cb`] or [`batch_futures_descendants_stream_async_cb`].
///
/// If neither of the above features are needed, you may as well use `buffer_unordered` <https://users.rust-lang.org/t/batch-execution-of-futures-in-the-tokio-runtime-or-max-number-of-active-futures-at-a-time/47659>
pub async fn batch_futures_descendants<R, Fut: Future<Output = R>>(
    batch_limit: &BatchLimit,
    fut_cbs: impl IntoIterator<Item = impl FnOnce(BatchLimit) -> Fut>,
) -> Vec<R> {
    let mut results = vec![];
    let _ = batch_futures_descendants_stream_sync_cb(batch_limit, fut_cbs, |result| {
        results.push(result);
        Ok::<(), ()>(())
    })
    .await;
    results
}

macro_rules! batch_futures_descendants_impl {
    ($batch_limit:expr, $fut_cbs:expr, |$result:ident| $call_cb:expr) => {{
        // As the semaphore, either using the parent's or creating a new one with the specified limit:
        let main_sem = match $batch_limit {
            BatchLimit::Parent(parent) => parent.clone(),
            #[cfg(not(target_arch = "wasm32"))]
            BatchLimit::Direct(max) => Arc::new(Semaphore::new(*max)),
            #[cfg(target_arch = "wasm32")]
            BatchLimit::Direct(max) => Rc::new(Semaphore::new(*max)),
        };

        // The crux of this manual implementation, if there's a parent, we already have 1 in-built permit, so we can use a direct route (i.e. a semaphore of 1) as well as getting parallelization permits from the parent.
        let local_highway_sem = if matches!($batch_limit, BatchLimit::Parent(_)) {
            Some(Semaphore::new(1))
        } else {
            None
        };

        // Needs to be ordered due to a key req that the results are returned in the same order as the input futures.
        // (Can be created automatically from the futures with collect())
        let mut stream: FuturesOrdered<_> = FuturesOrdered::new();
        for fut_cb in $fut_cbs.into_iter() {
            macro_rules! stream_poller_fut {
                () => {
                    async {
                        let mut seen_stream_empty = false;
                        let mut told_logic_unexpected = false;
                        loop {
                            if let Some($result) = stream.next().await {
                                if seen_stream_empty && !told_logic_unexpected {
                                    told_logic_unexpected = true;
                                    tracing::error!("Logic unexpected, never expected to get a result from stream here after seeing the stream is empty. See comments in file.");
                                }
                                $call_cb
                            } else {
                                // Here if the stream is empty, meaning all the permits must have been taken by parents or descendants, so this block effectively could sleep infinitely, as the event that will break the select!{} will be a permit becoming available from a parent or descendant.
                                // Doing 100ms and the error!() log so we know if this isn't true and our understanding is incorrect. 100ms is slow enough to not cause performance issues.
                                seen_stream_empty = true;

                                sleep_compat(chrono::Duration::milliseconds(100)).await;
                            }
                        }
                        #[allow(unreachable_code)]
                        Ok::<async_semaphore::SemaphoreGuard, E>(unreachable!())
                    }
                    .fuse()
                };
            }
            let permit = if let Some(local_highway_sem) = &local_highway_sem {
                // Always try using the local highway first, to prevent taking a permit from the parent semaphore unnecessarily (or polling the stream) when we've already obviously already got 1 in-built and ready to go:
                if let Some(permit) = local_highway_sem.try_acquire() {
                    permit
                } else if let Some(permit) = main_sem.try_acquire() {
                    // Then try getting one from main sem, to prevent spinning up the select!{} if we can avoid it:
                    permit
                } else {
                    // Otherwise, select on:
                    // - local highway, still want to use if comes avail
                    // - parent's semaphore, can be used to parallelize instead of local highway
                    // - stream poller, if results become ready, their callbacks should be run (this also wakes unpolled futures in the stream)

                    // Using the fuse thing to avoid pin: (probs not needed, but obvs a core fn so any perf gain is good)
                    // https://users.rust-lang.org/t/why-macro-select-needs-to-work-with-unpin-futures/70898
                    select! {
                        permit = {local_highway_sem.acquire().fuse()} => permit,
                        permit = {main_sem.acquire().fuse()} => permit,
                        permit = {stream_poller_fut!()} => permit?,
                    }
                }
            } else {
                // Try getting first from main_sem, to avoid spinning up the select!{} if we can avoid it:
                if let Some(permit) = main_sem.try_acquire() {
                    permit
                } else {
                    // This is top-level, no local highway, select on:
                    // - main sem which is the same as parent sem in other block
                    // - stream poller, if results become ready, their callbacks should be run (this also wakes unpolled futures in the stream)
                    select! {
                        permit = {main_sem.acquire().fuse()} => permit,
                        permit = {stream_poller_fut!()} => permit?,
                    }
                }
            };

            // Got a permit, spawn off the task, only releasing the permit once the task is done.
            // Passing down the current main_sem as the batch limit to allow descendants to limit themselves by it:
            let fut = fut_cb(BatchLimit::Parent(main_sem.clone()));
            stream.push_back(async move {
                let result = fut.await;
                drop(permit);
                result
            });
        }

        // All the tasks spawned off, process the remaining in the stream:
        while let Some($result) = stream.next().await {
            $call_cb
        }

        Ok(())
    }};
}

/// Underlying of [`batch_futures_descendants`], use if need to process in order during execution.
pub async fn batch_futures_descendants_stream_sync_cb<R, Fut: Future<Output = R>, E>(
    batch_limit: &BatchLimit,
    fut_cbs: impl IntoIterator<Item = impl FnOnce(BatchLimit) -> Fut>,
    mut result_sync_cb: impl FnMut(R) -> Result<(), E>,
) -> Result<(), E> {
    batch_futures_descendants_impl!(batch_limit, fut_cbs, |result| {
        result_sync_cb(result)?;
    })
}

/// Underlying of [`batch_futures_descendants`], use if need to process in order during execution.
pub async fn batch_futures_descendants_stream_async_cb<
    R,
    Fut: Future<Output = R>,
    E,
    CbFut: Future<Output = Result<(), E>>,
>(
    batch_limit: &BatchLimit,
    fut_cbs: impl IntoIterator<Item = impl FnOnce(BatchLimit) -> Fut>,
    result_async_cb: impl Fn(R) -> CbFut,
) -> Result<(), E> {
    batch_futures_descendants_impl!(batch_limit, fut_cbs, |result| {
        result_async_cb(result).await?;
    })
}
