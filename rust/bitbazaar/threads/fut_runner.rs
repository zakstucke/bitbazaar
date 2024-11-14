use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use std::{cmp::Reverse, collections::BinaryHeap};

use chrono::TimeDelta;
use futures::{stream::FuturesUnordered, Future, FutureExt, StreamExt};
use tokio::sync::mpsc::{unbounded_channel, UnboundedReceiver, UnboundedSender};
use tokio::sync::{OwnedSemaphorePermit, Semaphore, SemaphorePermit};

use crate::misc::{sleep_compat, InstantCompat};

tokio::task_local! {
    static PARENT_SEM: Arc<Semaphore>;
}

/// The default priority of shared runners.
/// By default everything has the highest priority.
pub static FUT_RUNNER_PRIORITY_DEFAULT: u8 = 0;

/// Rust can sometimes fail to type hint the stream_cb callback argument.
///
/// Until async closures come in and those replace it, try wrapping your closure in this to fix compile errors.
///
/// Got from:
/// https://users.rust-lang.org/t/async-closure-that-references-arguments/73534/6
pub fn hint_stream_cb<'a, R, O: 'a, Fut: Future<Output = R> + 'a, Fun: FnOnce(O) -> Fut>(
    f: Fun,
) -> Fun {
    f
}

/// No need to internally drive if the stream is remote, because in that case the stream is being driven by the remote anyway.
async fn maybe_drive_whilst<'a, R, T, Fut: Future<Output = R>>(
    stream: &mut MaybeRemoteStream<'a, R, Fut>,
    results_store: &mut ResultStore<R>,
    fut: impl Future<Output = T>,
) -> T {
    if matches!(stream, MaybeRemoteStream::Remote { .. }) {
        fut.await
    } else {
        let driver_fut = async {
            let mut seen_stream_empty = false;
            let mut told_logic_unexpected = false;
            loop {
                if let Some(FutResult { index, data, .. }) = stream.next().await {
                    if seen_stream_empty && !told_logic_unexpected {
                        told_logic_unexpected = true;
                        tracing::error!("Logic unexpected, never expected to get a result from stream here after seeing the stream is empty. See comments in file.");
                    }
                    results_store.store(index, data);
                } else {
                    // Here if the stream is empty, meaning this block effectively could sleep infinitely as nothing to drive, the other future being selected on must happen before the stream has items again.
                    // Doing 100ms and the error!() log so we know if this isn't true and our understanding is incorrect. 100ms is slow enough to not cause performance issues.
                    seen_stream_empty = true;

                    sleep_compat(TimeDelta::milliseconds(100)).await;
                }
            }
        };
        futures::select_biased! {
            output = fut.fuse() => output,
            _ = {driver_fut.fuse()} => unreachable!(),
        }
    }
}

// TODO once async closures come in, would prefer to give out the stream as a mutable ref rather than owned too:
// -  prevent use after the closure completes (drive would stop then)
// - allow a final join_remaining() to be built into the cleanup of the closure.
async fn with_remote_autodrive<
    'a,
    'b,
    Fut: Future<Output = R> + 'b,
    R: 'b,
    StreamCbOutput,
    StreamCbOutputFut: Future<Output = StreamCbOutput>,
>(
    max_concurrent: Option<MaxConcurrent<'a>>,
    max_throughput: Option<MaxThroughput<'a>>,
    order_output: bool,
    stream_cb: impl FnOnce(FutRunner<'a, R, Fut>) -> StreamCbOutputFut + 'b,
    maybe_prioritizer: Option<&'a Arc<Prioritizer>>,
    priority: u8,
) -> StreamCbOutput
where
    'a: 'b,
{
    let mut real_stream = FuturesUnordered::new();
    let (driver_tx, mut driver_rx) = unbounded_channel();
    let (runner_tx, runner_rx) = unbounded_channel();

    let fut_runner = FutRunner::new(
        max_concurrent,
        max_throughput,
        order_output,
        MaybeRemoteStream::Remote {
            len: 0,
            receiver: runner_rx,
            pusher: driver_tx,
        },
        maybe_prioritizer,
        priority,
    );

    let mut stream_len = 0;
    let driver_fut = async {
        loop {
            macro_rules! process_push {
                ($result:expr) => {
                    if let Some(new_fut) = $result {
                        real_stream.push(new_fut);
                        stream_len += 1;
                    } else {
                        tracing::error!("fut_runner remote driver_rx closed, this was never expected to happen.");
                        sleep_compat(TimeDelta::milliseconds(100)).await;
                    }
                };
            }

            if stream_len == 0 {
                process_push!(driver_rx.recv().await);
            }

            futures::select! {
                result = driver_rx.recv().fuse() => process_push!(result),
                result = real_stream.next().fuse() => {
                    if let Some(OrderWrapper { index, data, .. }) = result {
                        match runner_tx.send(FutResult { index, data }) {
                            Ok(()) => {
                                stream_len -= 1;
                            },
                            Err(_e) => {
                                tracing::error!("fut_runner remote driver couldn't send a completed result back to the runner because the runner_tx has been dropped/closed.");
                            },
                        }
                    }
                }
            }
        }
    };

    futures::select! {
        result = stream_cb(fut_runner).fuse() => result,
        _ = driver_fut.fuse() => unreachable!()
    }
}

/// Create a new builder for a standalone or shared fut_runner.
pub fn new_fut_runner() -> FutRunnerBuilder {
    FutRunnerBuilder::default()
}

#[derive(Debug)]
struct PriorityBlock {
    priority: u8,
    waker: Arc<tokio::sync::Notify>,
    released: Arc<AtomicBool>,
}

impl PartialEq for PriorityBlock {
    fn eq(&self, other: &Self) -> bool {
        self.priority == other.priority
    }
}

impl Eq for PriorityBlock {}

impl PartialOrd for PriorityBlock {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for PriorityBlock {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        // Lowest number = highest priority
        self.priority.cmp(&other.priority).reverse()
    }
}

#[derive(Debug, Default)]
struct Prioritizer {
    queue: parking_lot::Mutex<BinaryHeap<PriorityBlock>>,
}

/// A fut_runner producing factory that can be cloned and shares it's limits
/// with all other fut_runners spawned from this factory or any of it's clones.
#[derive(Debug, Clone)]
pub struct FutRunnerShared {
    prioritizer: Arc<Prioritizer>,
    max_concurrent: Option<Arc<Semaphore>>,
    max_throughput: Option<Arc<MaxThroughputShared>>,
    order_output: bool,
}

impl FutRunnerShared {
    /// Get a new fut_runner that shares it's limits
    /// with all other fut_runners spawned from this factory.
    pub async fn stream<
        'a,
        'b,
        R: 'b,
        Fut: Future<Output = R> + 'b,
        StreamCbOutput,
        StreamCbOutputFut: Future<Output = StreamCbOutput>,
    >(
        &'a self,
        stream_cb: impl FnOnce(FutRunner<'a, R, Fut>) -> StreamCbOutputFut + 'b,
    ) -> StreamCbOutput
    where
        'a: 'b,
    {
        self.stream_with_priority(FUT_RUNNER_PRIORITY_DEFAULT, stream_cb)
            .await
    }

    /// Same as [`FutRunnerShared::stream`] but with a custom priority.
    ///
    /// 0-255, 0 being the highest priority.
    /// 0 (max) is default to allow for slowing things down,
    /// but not introduce a footgun of increasing priority of things and blocking everything with default.
    pub async fn stream_with_priority<
        'a,
        'b,
        R: 'b,
        Fut: Future<Output = R> + 'b,
        StreamCbOutput,
        StreamCbOutputFut: Future<Output = StreamCbOutput>,
    >(
        &'a self,
        priority: u8,
        stream_cb: impl FnOnce(FutRunner<'a, R, Fut>) -> StreamCbOutputFut + 'b,
    ) -> StreamCbOutput
    where
        'a: 'b,
    {
        with_remote_autodrive(
            self.max_concurrent.as_ref().map(MaxConcurrent::Shared),
            self.max_throughput.as_ref().map(MaxThroughput::Shared),
            self.order_output,
            stream_cb,
            Some(&self.prioritizer),
            priority,
        )
        .await
    }

    /// [`FutRunnerShared::join`] but for a single future.
    pub async fn run<T>(&self, fut: impl Future<Output = T>) -> T {
        self.run_with_priority(FUT_RUNNER_PRIORITY_DEFAULT, fut)
            .await
    }

    /// Same as [`FutRunnerShared::run`] but with a custom priority.
    ///
    /// 0-255, 0 being the highest priority.
    /// 0 (max) is default to allow for reducing priority of things,
    /// but not introduce a footgun of increasing priority of things and blocking everything with default.
    pub async fn run_with_priority<T>(&self, priority: u8, fut: impl Future<Output = T>) -> T {
        let mut fut_runner = self.new_local(priority);
        fut_runner.push(fut).await;
        fut_runner.next().await.expect("Expected a single result.")
    }

    /// Same as [`FutRunnerBuilder::into_join`] except it shares
    /// it's limits with others called from this [`FutRunnerShared`].
    pub async fn join<T, Fut: Future<Output = T>>(
        &self,
        futs: impl IntoIterator<Item = Fut>,
    ) -> Vec<T> {
        self.join_with_priority(FUT_RUNNER_PRIORITY_DEFAULT, futs)
            .await
    }

    /// Same as [`FutRunnerShared::join`] but with a custom priority.
    ///
    /// 0-255, 0 being the highest priority.
    /// 0 (max) is default to allow for slowing things down,
    /// but not introduce a footgun of increasing priority of things and blocking everything with default.
    pub async fn join_with_priority<T, Fut: Future<Output = T>>(
        &self,
        priority: u8,
        futs: impl IntoIterator<Item = Fut>,
    ) -> Vec<T> {
        let mut fut_runner = self.new_local(priority);
        fut_runner.extend(futs).await;
        fut_runner.join_remaining().await
    }

    fn new_local<R, Fut: Future<Output = R>>(&self, priority: u8) -> FutRunner<'_, R, Fut> {
        FutRunner::new(
            self.max_concurrent.as_ref().map(MaxConcurrent::Shared),
            self.max_throughput.as_ref().map(MaxThroughput::Shared),
            self.order_output,
            MaybeRemoteStream::Local {
                stream: FuturesUnordered::new(),
            },
            Some(&self.prioritizer),
            priority,
        )
    }
}

#[derive(Debug)]
enum MaybeRemoteStream<'a, R, Fut> {
    Local {
        stream: FuturesUnordered<OrderWrapper<'a, Fut>>,
    },
    Remote {
        len: usize,
        receiver: UnboundedReceiver<FutResult<R>>,
        pusher: UnboundedSender<OrderWrapper<'a, Fut>>,
    },
}

impl<'a, R, Fut> MaybeRemoteStream<'a, R, Fut>
where
    Fut: Future<Output = R>,
{
    async fn next(&mut self) -> Option<FutResult<R>> {
        match self {
            MaybeRemoteStream::Local { stream } => {
                if let Some(OrderWrapper { index, data, .. }) = stream.next().await {
                    Some(FutResult { index, data })
                } else {
                    None
                }
            }
            MaybeRemoteStream::Remote { len, receiver, .. } => {
                if *len == 0 {
                    None
                } else {
                    match receiver.recv().await {
                        Some(result) => {
                            *len -= 1;
                            Some(result)
                        }
                        None => None,
                    }
                }
            }
        }
    }

    fn len(&self) -> usize {
        match self {
            MaybeRemoteStream::Local { stream } => stream.len(),
            MaybeRemoteStream::Remote { len, .. } => *len,
        }
    }

    fn push(&mut self, item: OrderWrapper<'a, Fut>) {
        match self {
            MaybeRemoteStream::Local { stream } => stream.push(item),
            MaybeRemoteStream::Remote { len, pusher, .. } => match pusher.send(item) {
                Ok(()) => {
                    *len += 1;
                }
                Err(_) => tracing::error!("fut_runner remote pusher has been dropped/closed."),
            },
        }
    }
}

/// A fut_runner for a single future type.
/// Either produced from the builder as a standalone fut_runner
/// with it's own limits, or from the [`FutRunnerShared`] factory where it shares limits with other fut_runners produced by said factory.
///
/// NOTE: purposely not implementing Clone!
#[derive(Debug)]
pub struct FutRunner<'a, R, Fut> {
    max_concurrent: Option<MaxConcurrent<'a>>,
    max_throughput: Option<MaxThroughput<'a>>,
    stream: MaybeRemoteStream<'a, R, Fut>,
    results_store: ResultStore<R>,
    index: i64,
    ordered: bool,
    // If shared this will be supplied to allow some runners to take priority over others:
    prioritizer: Option<&'a Arc<Prioritizer>>,
    priority: u8,
    priority_waker: Option<Arc<tokio::sync::Notify>>,
    priority_released: Option<Arc<AtomicBool>>,
}

impl<'a, R, Fut> FutRunner<'a, R, Fut>
where
    Fut: Future<Output = R>,
{
    fn new(
        max_concurrent: Option<MaxConcurrent<'a>>,
        max_throughput: Option<MaxThroughput<'a>>,
        order_output: bool,
        stream: MaybeRemoteStream<'a, R, Fut>,
        maybe_prioritizer: Option<&'a Arc<Prioritizer>>,
        priority: u8,
    ) -> Self {
        Self {
            max_concurrent,
            max_throughput,
            stream,
            results_store: ResultStore::new(order_output),
            index: 0,
            ordered: order_output,
            prioritizer: maybe_prioritizer,
            priority,
            priority_waker: None,
            priority_released: None,
        }
    }
}

#[derive(Debug)]
enum MaybeOwnedPermit<'a> {
    #[allow(dead_code)]
    Owned(OwnedSemaphorePermit),
    #[allow(dead_code)]
    Borrowed(SemaphorePermit<'a>),
}

impl<'a, R, Fut> FutRunner<'a, R, Fut>
where
    Fut: Future<Output = R>,
{
    /// Push a new future to be processed.
    pub async fn push(&mut self, fut: Fut) {
        self.register_priority();

        let maybe_permit = self.wait_for_requirements().await;

        // Start processing the next future with it's index
        // to resort if ordered=true:
        self.stream.push(OrderWrapper {
            index: self.index,
            data: fut,
            maybe_permit,
        });

        self.release_priority();

        self.index += 1;
    }

    /// Push multiple futures to be processed.
    pub async fn extend(&mut self, futs: impl IntoIterator<Item = Fut>) {
        for fut in futs {
            self.push(fut).await;
        }
    }

    /// Extract the next completed future.
    /// When None, all futures have been consumed.
    ///
    /// When ordered, the results are buffered internally to still output in order.
    pub async fn next(&mut self) -> Option<R> {
        loop {
            // Check if the next thing is available from the store, will be ordered if needed:
            if let Some(result) = self.results_store.next() {
                return Some(result);
            }
            // Otherwise, drive the stream:
            if let Some(FutResult { index, data, .. }) = self.stream.next().await {
                // Can bypass the store if unordered, otherwise have to store and reloop as maybe out of order:
                if self.ordered {
                    self.results_store.store(index, data);
                } else {
                    return Some(data);
                }
            } else {
                return None;
            }
        }
    }

    /// Drives the remaining futures in the stream to completion, returning all in a Vec<R>.
    pub async fn join_remaining(&mut self) -> Vec<R> {
        // Rather than using fut_runner.next() and rebuilding a vec![] in here, more efficient to just consume the store itself:
        while let Some(result) = self.stream.next().await {
            self.results_store.store(result.index, result.data);
        }
        self.results_store.consume()
    }

    /// Check if no futures remain in the system and need extracting
    /// with either [`FutRunner::next_batch`] or [`FutRunner::drive_remaining`].
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Get the amount of futures remaining in the system and need extracting
    /// with either [`FutRunner::next_batch`] or [`FutRunner::drive_remaining`].
    pub fn len(&self) -> usize {
        self.stream.len() + self.results_store.len()
    }

    /// NOTE: at the moment in here we wait on:
    /// - a tokio semaphore (concurrency limit)
    /// - a tokio mutex lock (concurrency limit)
    ///
    /// Both of these are fair in that they give in order of request,
    /// adding any new primitives should ideally also keep this behaviour to allow recalling after priority change.
    ///
    /// This should also not update anything as may be rolled back, hence the update_requirements should be used instead.
    async fn wait_for_requirements(&mut self) -> Option<MaybeOwnedPermit<'a>> {
        loop {
            // Don't try and get requirements until the highest priority:
            // This is a wait_for_highest_priority() function,
            // weirdly fails to compile if extracted into it's own function with nonsync future error..
            if !self.is_highest_priority() {
                if let Some(waker) = &self.priority_waker {
                    maybe_drive_whilst(&mut self.stream, &mut self.results_store, waker.notified())
                        .await;
                }
            }

            // Handle max_concurrent:
            let maybe_permit = if let Some(limit) = &self.max_concurrent {
                let maybe_permit = match limit {
                    MaxConcurrent::Shared(sem) => {
                        // Need to use the PARENT_SEM if there is one (the in-built permit for when already inside a throttled future) should be tried before another permit, to avoid hogging a permit and leaving another free.
                        Some(if let Ok(parent_sem) = PARENT_SEM.try_with(Clone::clone) {
                            // Biased as we definitely want the parent sem to be used instead of a pool permit if possible.
                            // to prevent potentially stealing an extra permit when no need.
                            maybe_drive_whilst(&mut self.stream, &mut self.results_store, async {
                                futures::select_biased! {
                                    permit = parent_sem.acquire_owned().fuse() => {
                                        MaybeOwnedPermit::Owned(permit.expect("The fut_runner PARENT_SEM has been closed, which should never have happened."))
                                    },
                                    permit = sem.acquire().fuse() => {
                                        MaybeOwnedPermit::Borrowed(permit.expect("The fut_runner semaphore has been closed, which should never have happened."))
                                    },
                                }
                            }).await
                        } else {
                            let permit = maybe_drive_whilst(
                                &mut self.stream,
                                &mut self.results_store,
                                sem.acquire(),
                            )
                            .await;
                            MaybeOwnedPermit::Borrowed(permit.expect("The fut_runner semaphore has been closed, which should never have happened."))
                        })
                    }
                    MaxConcurrent::Standalone(limit) => {
                        if self.stream.len() >= *limit {
                            // Standalone, so just need to wait for the next to finish:
                            if let Some(FutResult { index, data, .. }) = self.stream.next().await {
                                self.results_store.store(index, data);
                            }
                        }
                        None
                    }
                };

                // Drop the semaphore and reloop if no longer the highest priority:
                if !self.is_highest_priority() {
                    continue;
                }

                maybe_permit
            } else {
                None
            };

            // Handle max_throughput:
            if let Some(max_throughput) = &mut self.max_throughput {
                match max_throughput {
                    MaxThroughput::Shared(shared) => {
                        // Async tokio mutex is simpler as all can be stuck waiting for guard,
                        // then the one that gets it can keep holding it whilst waiting for the time to pass.
                        // rather than a sync one where everyone would have to get mutex check drop then wait then check again etc.
                        let mut last_called = maybe_drive_whilst(
                            &mut self.stream,
                            &mut self.results_store,
                            shared.last_called.lock(),
                        )
                        .await;
                        if let Some(last_called) = &*last_called {
                            let time_since = last_called.elapsed();
                            if time_since < shared.max_throughput {
                                maybe_drive_whilst(
                                    &mut self.stream,
                                    &mut self.results_store,
                                    sleep_compat(shared.max_throughput - time_since),
                                )
                                .await
                            }
                            // Drop the semaphore/last_called mutex and reloop before updating last_called if no longer the highest priority:
                            if !self.is_highest_priority() {
                                continue;
                            }
                        }
                        *last_called = Some(InstantCompat::now());
                    }
                    MaxThroughput::Standalone {
                        max_throughput,
                        last_called,
                    } => {
                        if let Some(last_called) = last_called {
                            let time_since = last_called.elapsed();
                            if time_since < *max_throughput {
                                maybe_drive_whilst(
                                    &mut self.stream,
                                    &mut self.results_store,
                                    sleep_compat(*max_throughput - time_since),
                                )
                                .await;
                            }
                        }
                        // Update for next time:
                        *last_called = Some(InstantCompat::now());
                    }
                }
            }

            // If we've got here, still the highest priority and have all the required allowances:
            break maybe_permit;
        }
    }

    fn register_priority(&mut self) {
        let priority_released = Arc::new(AtomicBool::new(false));
        self.priority_waker = {
            if let Some(prioritizer) = &self.prioritizer {
                let waker = Arc::new(tokio::sync::Notify::new());
                let mut queue = prioritizer.queue.lock();
                queue.push(PriorityBlock {
                    priority: self.priority,
                    waker: waker.clone(),
                    released: priority_released.clone(),
                });
                Some(waker)
            } else {
                None
            }
        };
        self.priority_released = Some(priority_released);
    }

    fn release_priority(&mut self) {
        if let Some(priority_released) = self.priority_released.take() {
            priority_released.store(true, std::sync::atomic::Ordering::Relaxed);
        }
        if let Some(prioritizer) = &self.prioritizer {
            let mut queue = prioritizer.queue.lock();
            self.priority_queue_pop_released_and_wake_next_if_needed(&mut queue);
            // The next might have been newly at the front and not woken yet:
            if let Some(block) = queue.peek() {
                block.waker.notify_one();
            }
        }
    }

    fn is_highest_priority(&self) -> bool {
        if let Some(prioritizer) = &self.prioritizer {
            let mut queue = prioritizer.queue.lock();
            self.priority_queue_pop_released_and_wake_next_if_needed(&mut queue);
            if let Some(block) = queue.peek() {
                self.priority <= block.priority
            } else {
                false
            }
        } else {
            true
        }
    }

    fn priority_queue_pop_released_and_wake_next_if_needed(
        &self,
        queue: &mut BinaryHeap<PriorityBlock>,
    ) {
        while {
            if let Some(block) = queue.peek() {
                block.released.load(std::sync::atomic::Ordering::Relaxed)
            } else {
                false
            }
        } {
            queue.pop();
        }
        // The next might have been newly at the front and not woken yet:
        if let Some(block) = queue.peek() {
            block.waker.notify_one();
        }
    }
}

/// A builder to configure either a standalone of shared fut_runner.
#[derive(Clone, Debug, Default)]
pub struct FutRunnerBuilder {
    order_output: bool,
    max_concurrent: Option<usize>,
    max_throughput: Option<TimeDelta>,
}

impl FutRunnerBuilder {
    /// Limit the fut_runner to never concurrently process
    /// more than the specified amount of futures at once.
    pub fn max_concurrent(mut self, max_concurrent: usize) -> Self {
        self.max_concurrent = Some(max_concurrent);
        self
    }

    /// Limit the fut_runner to never start a new future before the last
    /// was started more than the specified amount of time ago.
    pub fn max_throughput(mut self, max_throughput: TimeDelta) -> Self {
        self.max_throughput = Some(max_throughput);
        self
    }

    /// By default outputs arrive in the order they finish for performance.
    /// To only return in order, call this method.
    pub fn order_output(mut self) -> Self {
        self.order_output = true;
        self
    }

    /// A reusable fut_runner that can be used in lots of locations
    /// at once, providing a global limit.
    pub fn into_shared(self) -> FutRunnerShared {
        FutRunnerShared {
            max_concurrent: self
                .max_concurrent
                .map(|limit| Arc::new(Semaphore::new(limit))),
            max_throughput: self.max_throughput.map(|max_throughput| {
                Arc::new(MaxThroughputShared {
                    max_throughput,
                    last_called: tokio::sync::Mutex::new(None),
                })
            }),
            order_output: self.order_output,
            prioritizer: Arc::new(Prioritizer::default()),
        }
    }

    /// A non-reusable fut_runner, more efficient as doesn't need a semaphore.
    /// Preferable when used in one place with a single future type.
    pub async fn into_standalone<
        'a,
        'b,
        R: 'b,
        Fut: Future<Output = R> + 'b,
        StreamCbOutput,
        StreamCbOutputFut: Future<Output = StreamCbOutput>,
    >(
        self,
        stream_cb: impl FnOnce(FutRunner<'a, R, Fut>) -> StreamCbOutputFut + 'b,
    ) -> StreamCbOutput
    where
        'a: 'b,
    {
        with_remote_autodrive(
            self.max_concurrent.map(MaxConcurrent::Standalone),
            self.max_throughput
                .map(|max_throughput| MaxThroughput::Standalone {
                    max_throughput,
                    last_called: None,
                }),
            self.order_output,
            stream_cb,
            None,
            FUT_RUNNER_PRIORITY_DEFAULT,
        )
        .await
    }

    /// The most concise fut_runner:
    /// - pass in an iterator of futures
    /// - receive a vector back of the results
    ///
    /// If you need to share a throttle in lots of places, or add futures whilst processing, or read outputs before all are complete, use [`FutRunnerBuilder::into_standalone`] or [`FutRunnerBuilder::into_shared`]
    pub async fn into_join<T, Fut: Future<Output = T>>(
        self,
        futs: impl IntoIterator<Item = Fut>,
    ) -> Vec<T> {
        let mut fut_runner = FutRunner::new(
            self.max_concurrent.map(MaxConcurrent::Standalone),
            self.max_throughput
                .map(|max_throughput| MaxThroughput::Standalone {
                    max_throughput,
                    last_called: None,
                }),
            self.order_output,
            MaybeRemoteStream::Local {
                stream: FuturesUnordered::new(),
            },
            None,
            FUT_RUNNER_PRIORITY_DEFAULT,
        );
        fut_runner.extend(futs).await;
        fut_runner.join_remaining().await
    }
}

#[derive(Debug)]
struct FutResult<R> {
    index: i64,
    data: R,
}

impl<R> PartialEq for FutResult<R> {
    fn eq(&self, other: &Self) -> bool {
        self.index == other.index
    }
}

impl<R> Eq for FutResult<R> {}

impl<R> PartialOrd for FutResult<R> {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl<R> Ord for FutResult<R> {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        // Want the lowest index to come out the heap first:
        Reverse(self.index).cmp(&Reverse(other.index))
    }
}

#[derive(Debug)]
enum ResultStore<R> {
    Ordered {
        // usize is the index
        heap: BinaryHeap<FutResult<R>>,
        current_pending_index: i64,
    },
    Unordered {
        buf: Vec<R>,
    },
}

impl<R> ResultStore<R> {
    fn new(ordered: bool) -> Self {
        if ordered {
            Self::Ordered {
                heap: BinaryHeap::new(),
                current_pending_index: 0,
            }
        } else {
            Self::Unordered { buf: vec![] }
        }
    }

    /// Store a result, ordering (if needed) internally managed.
    fn store(&mut self, index: i64, result: R) {
        match self {
            Self::Ordered { heap, .. } => {
                heap.push(FutResult {
                    index,
                    data: result,
                });
            }
            Self::Unordered { buf } => buf.push(result),
        }
    }

    fn next(&mut self) -> Option<R> {
        match self {
            Self::Ordered {
                heap,
                current_pending_index,
            } => {
                if heap
                    .peek()
                    .map(|x| x.index == *current_pending_index)
                    .unwrap_or(false)
                {
                    Some(
                        heap.pop()
                            .expect("Just checked an item existed and is next to pop.")
                            .data,
                    )
                } else {
                    None
                }
            }
            Self::Unordered { buf } => buf.pop(),
        }
    }

    fn len(&self) -> usize {
        match self {
            Self::Ordered { heap, .. } => heap.len(),
            Self::Unordered { buf } => buf.len(),
        }
    }

    /// Consume the store into a vec of (ordered if applicable) results.
    ///
    /// NOTE: should only be called when you know the store contains everything and the underlying stream is empty.
    fn consume(&mut self) -> Vec<R> {
        match self {
            Self::Ordered { heap, .. } => std::mem::take(heap)
                .into_sorted_vec()
                .into_iter()
                .rev()
                .map(|x| x.data)
                .collect(),
            Self::Unordered { buf } => std::mem::take(buf),
        }
    }
}

#[derive(Debug)]
enum MaxConcurrent<'a> {
    Shared(&'a Arc<Semaphore>),
    Standalone(usize),
}

#[derive(Debug)]
enum MaxThroughput<'a> {
    Shared(&'a Arc<MaxThroughputShared>),
    Standalone {
        max_throughput: TimeDelta,
        last_called: Option<InstantCompat>,
    },
}

#[derive(Debug)]
struct MaxThroughputShared {
    max_throughput: TimeDelta,
    last_called: tokio::sync::Mutex<Option<InstantCompat>>,
}

// We need a future that keeps it's index and maybe a semaphore permit,
// but doesn't wrap the original future generic,
// to avoid having to box inside the FutRunner struct.
// This actually came from futures-util-0.3.30/src/stream/futures_ordered.rs
pin_project_lite::pin_project! {
    #[must_use = "futures do nothing unless you `.await` or poll them"]
    #[derive(Debug)]
    struct OrderWrapper<'a, T> {
        #[pin]
        data: T, // A future or a future's output
        // Use i64 for index since isize may overflow in 32-bit targets.
        index: i64,
        maybe_permit: Option<MaybeOwnedPermit<'a>>,
    }
}

impl<'a, T> PartialEq for OrderWrapper<'a, T> {
    fn eq(&self, other: &Self) -> bool {
        self.index == other.index
    }
}

impl<'a, T> Eq for OrderWrapper<'a, T> {}

impl<'a, T> PartialOrd for OrderWrapper<'a, T> {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl<'a, T> Ord for OrderWrapper<'a, T> {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        // BinaryHeap is a max heap, so compare backwards here.
        other.index.cmp(&self.index)
    }
}

impl<'a, T> Future for OrderWrapper<'a, T>
where
    T: Future,
{
    type Output = OrderWrapper<'a, T::Output>;

    fn poll(
        self: std::pin::Pin<&mut Self>,
        cx: &mut futures::task::Context<'_>,
    ) -> futures::task::Poll<Self::Output> {
        let index = self.index;
        let is_shared = self.maybe_permit.is_some();

        let projected = self.project();

        // Need to provide a local highway to allow throttled parent future to provide a single extra permit to be shared between it's children to prevent deadlocks:
        let result = if is_shared {
            PARENT_SEM.sync_scope(Arc::new(Semaphore::new(1)), || projected.data.poll(cx))
        } else {
            projected.data.poll(cx)
        };

        match result {
            std::task::Poll::Ready(output) => {
                // Complete now so throw away the permit.
                drop(projected.maybe_permit.take());
                std::task::Poll::Ready(OrderWrapper {
                    data: output,
                    index,
                    maybe_permit: None,
                })
            }
            std::task::Poll::Pending => std::task::Poll::Pending,
        }
    }
}

// Timing related testing, disable for windows as always inaccurate:
#[cfg(all(test, not(windows)))]
mod test {
    use std::sync::atomic::AtomicUsize;

    use chrono::Utc;
    use futures::future::join_all;

    pub use super::*;

    use crate::chrono::chrono_format_dt;
    pub use crate::test::prelude::*;

    fn simulate_expected_ms(
        max_concurrent: Option<usize>,
        max_throughput: Option<i64>,
        fut_duration: i64,
        num_futs: usize,
    ) -> i64 {
        let max_concurrent = max_concurrent.unwrap_or(usize::MAX);
        let max_throughput = max_throughput.unwrap_or(0);
        let mut expected_ms = 0;

        let mut last_started_ms_ago = i64::MAX;
        let mut running = vec![];

        let tick_duration_ms = 1;

        // Each loop of this is a tick of length tick_duration_ms:
        let mut remaining_futs = num_futs;
        while remaining_futs > 0 || !running.is_empty() {
            for fut_duration in running.iter_mut() {
                *fut_duration -= tick_duration_ms;
            }
            running.retain(|fut_duration| *fut_duration > 0);
            while remaining_futs > 0
                && running.len() < max_concurrent
                && ((max_throughput == 0) || last_started_ms_ago > max_throughput)
            {
                running.push(fut_duration);
                last_started_ms_ago = 0;
                remaining_futs -= 1
            }
            expected_ms += tick_duration_ms;
            last_started_ms_ago += tick_duration_ms;
        }

        expected_ms
    }

    // Push the futures shared between all fut_runners,
    // note we have to do extend and drive_remaining in the same future for each fut_runner,
    // because they're pull, and otherwise pushing to other fut_runners before sucking with drive_remaining
    // can cause unnecessary waiting.
    // Not really a worry in real usage as this would be how its done anyway,
    // but in this test we have to make sure to not mess up the calculation.
    async fn run_futs_shared_between_fut_runners<T: Send, Fut: Future<Output = T>>(
        shared: bool,
        builder: FutRunnerBuilder,
        futs: impl IntoIterator<Item = Fut>,
    ) -> Vec<T> {
        let maybe_used_shared_builder = builder.clone().into_shared();

        let num_fut_runners = if shared { 3 } else { 1 };
        let mut futs_per_fut_runner = (0..num_fut_runners).map(|_| vec![]).collect::<Vec<_>>();
        for (index, fut) in futs.into_iter().enumerate() {
            futs_per_fut_runner[index % num_fut_runners].push(fut);
        }

        join_all(futs_per_fut_runner.into_iter().map(|futs| {
            let maybe_used_shared_builder = maybe_used_shared_builder.clone();
            let builder = builder.clone();
            async move {
                let stream_cb = hint_stream_cb(|mut stream: FutRunner<T, _>| async move {
                    stream.extend(futs).await;
                    stream.join_remaining().await
                });

                if shared {
                    maybe_used_shared_builder.stream(stream_cb).await
                } else {
                    builder.into_standalone(stream_cb).await
                }
            }
        }))
        .await
        .into_iter()
        .flatten()
        .collect::<Vec<_>>()
    }

    #[tokio::test]
    #[rstest]
    async fn test_fut_runner_shared_nested_recursive(_logging: ()) {
        let max_concurrent = 10;
        let max_throughput = TimeDelta::milliseconds(5);
        let sleep_ms = 5;
        let shared = FutRunnerBuilder::default()
            .order_output()
            .max_concurrent(max_concurrent)
            .max_throughput(max_throughput)
            .into_shared();

        #[allow(clippy::manual_async_fn)]
        fn recurse_into(
            num_calls: Arc<AtomicUsize>,
            shared: FutRunnerShared,
            remaining_depth: usize,
            sleep_ms: i64,
        ) -> impl Future<Output = ()> + Send {
            async move {
                if remaining_depth == 0 {
                    return;
                }

                shared
                    .stream(|mut stream| {
                        let shared = shared.clone();
                        async move {
                            let futs = (0..4).map(|index| {
                                let num_calls = num_calls.clone();
                                let shared = shared.clone();
                                async move {
                                    sleep_compat(TimeDelta::milliseconds(sleep_ms)).await;
                                    num_calls.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                                    recurse_into(num_calls, shared, remaining_depth - 1, sleep_ms)
                                        .boxed()
                                        .await;
                                    index
                                }
                            });
                            stream.extend(futs).await;
                            let results = stream.join_remaining().await;
                            assert_eq!(results, (0..4).collect::<Vec<_>>());
                        }
                    })
                    .await;
            }
        }

        let expected_num_calls = {
            let mut tot = 0;
            for x in 1..=4 {
                tot += 4usize.pow(x)
            }
            tot
        };
        let expected_ms = simulate_expected_ms(
            Some(max_concurrent),
            Some(max_throughput.num_milliseconds()),
            sleep_ms,
            expected_num_calls,
        );

        // Means in total 5**4 futs, and a naive semaphore without a local highway would deadlock.
        let num_calls = Arc::new(AtomicUsize::new(0));
        let instant = InstantCompat::now();
        recurse_into(num_calls.clone(), shared, 4, sleep_ms).await;
        let actual_elapsed = instant.elapsed();
        assert_eq!(
            num_calls.load(std::sync::atomic::Ordering::Relaxed),
            expected_num_calls
        );

        // The timing for this one seems to take slightly longer than it should,
        // but that might make sense because of the recursive nature of the futures,
        // it acts slightly different to the simulation.
        tracing::info!(
            "Expected ms: {}, actual ms: {}",
            expected_ms,
            actual_elapsed.num_milliseconds()
        );
        assert_td_in_range!(
            actual_elapsed,
            (TimeDelta::milliseconds((expected_ms as f64 * 0.9).floor() as i64)
                ..TimeDelta::milliseconds((expected_ms as f64 * 1.2).ceil() as i64))
        );
    }

    #[tokio::test]
    #[rstest]
    #[case(None, None, 10)]
    #[case(Some(5), None, 10)]
    #[case(None, Some(5), 10)]
    #[case(Some(5), Some(5), 10)]
    // The other so small it's useless:
    #[case(Some(50), Some(0), 10)]
    #[case(Some(10000), Some(5), 10)]
    async fn test_fut_runner_throttle_config(
        _logging: (),
        #[values(false, true)] shared: bool,
        #[case] max_concurrent: Option<usize>,
        #[case] max_throughput: Option<i64>,
        #[case] num_futs: usize,
    ) {
        let sleep_ms = 100;

        let mut builder = FutRunnerBuilder::default();
        if let Some(max_concurrent) = max_concurrent {
            builder = builder.max_concurrent(max_concurrent);
        }
        if let Some(max_throughput) = max_throughput {
            builder = builder.max_throughput(TimeDelta::milliseconds(max_throughput));
        }

        let instant = InstantCompat::now();

        let futs = (0..num_futs).map(|index| async move {
            sleep_compat(TimeDelta::milliseconds(sleep_ms)).await;
            index
        });

        let mut results = run_futs_shared_between_fut_runners(shared, builder, futs).await;
        let actual_elapsed = instant.elapsed();

        let expected_ms = simulate_expected_ms(max_concurrent, max_throughput, sleep_ms, num_futs);

        results.sort();
        assert_eq!(results, (0..num_futs).collect::<Vec<_>>());

        // tracing::info!(
        //     "Expected ms: {}, actual ms: {}",
        //     expected_ms,
        //     actual_elapsed.num_milliseconds()
        // );
        assert_td_in_range!(
            actual_elapsed,
            (TimeDelta::milliseconds((expected_ms as f64 * 0.9).floor() as i64)
                ..TimeDelta::milliseconds((expected_ms as f64 * 1.1).ceil() as i64))
        );
    }

    // When using as a pushable stream, need to make sure the futures are being auto driven to completion.
    #[tokio::test]
    #[rstest]
    async fn test_fut_runner_autodrive(
        _logging: (),
        #[values(false, true)] shared: bool,
        #[values(false, true)] order_output: bool,
    ) {
        let mut builder = FutRunnerBuilder::default();
        if order_output {
            builder = builder.order_output();
        }

        let maybe_used_shared_builder = builder.clone().into_shared();

        let stream_cb = hint_stream_cb(move |mut stream: FutRunner<i64, _>| async move {
            let instant = InstantCompat::now();
            for x in 0..5 {
                stream
                    .push(async move {
                        sleep_compat(TimeDelta::milliseconds(20)).await;
                        x
                    })
                    .await;
                sleep_compat(TimeDelta::milliseconds(20)).await;
            }
            stream.join_remaining().await;
            let elapsed = instant.elapsed();

            // Each fut should finish by the time the outer sleep finishes, so the whole thing complete in around 100ms,
            // if the futs weren't auto driving, this would take much longer:
            assert_td_in_range!(
                elapsed,
                (TimeDelta::milliseconds(100)..TimeDelta::milliseconds(115))
            );
        });

        if shared {
            builder.into_standalone(stream_cb).await;
        } else {
            maybe_used_shared_builder.stream(stream_cb).await;
        };
    }

    #[tokio::test]
    #[rstest]
    async fn test_fut_runner_output_ordering(
        _logging: (),
        #[values(false, true)] shared: bool,
        #[values(false, true)] order_output: bool,
    ) {
        let mut builder = FutRunnerBuilder::default();
        if order_output {
            builder = builder.order_output();
        }

        let stream_cb = hint_stream_cb(|mut stream: FutRunner<i64, _>| {
            async move {
                // Put progressively shorter sleeps, so theoretically the last should complete first.
                stream
                    .extend((0..5).map(|index| async move {
                        sleep_compat(TimeDelta::milliseconds(100 - (index * 10))).await;
                        index
                    }))
                    .await;
                let results = stream.join_remaining().await;

                if order_output {
                    assert_eq!(results, vec![0, 1, 2, 3, 4]);
                } else {
                    assert_eq!(results, vec![4, 3, 2, 1, 0]);
                }
            }
        });

        // Doesn't really make sense using more than one in the shared case,
        // just that the internals are part of the shared system and it still works:
        let maybe_used_shared_builder = builder.clone().into_shared();
        if shared {
            builder.into_standalone(stream_cb).await;
        } else {
            maybe_used_shared_builder.stream(stream_cb).await;
        };
    }

    #[tokio::test]
    #[rstest]
    async fn test_fut_runner_prioritization(
        _logging: (),
        #[values(false, true)] order_output: bool,
    ) {
        let mut builder = FutRunnerBuilder::default().max_throughput(TimeDelta::milliseconds(10));
        if order_output {
            builder = builder.order_output();
        }
        let shared = builder.clone().into_shared();

        let (results_1, results_2, results_3) = tokio::join!(
            // By default everything has max priority of 0.
            shared.join((0..5).map(|index| async move {
                sleep_compat(TimeDelta::milliseconds(10)).await;
                (Utc::now(), index)
            })),
            shared.join_with_priority(
                100,
                (5..10).map(|index| async move {
                    sleep_compat(TimeDelta::milliseconds(10)).await;
                    (Utc::now(), index)
                })
            ),
            shared.join_with_priority(
                254,
                (10..15).map(|index| async move {
                    sleep_compat(TimeDelta::milliseconds(10)).await;
                    (Utc::now(), index)
                })
            )
        );
        if order_output {
            assert_eq!(
                results_1
                    .iter()
                    .map(|(_, index)| *index)
                    .collect::<Vec<_>>(),
                (0..5).collect::<Vec<_>>()
            );
            assert_eq!(
                results_2
                    .iter()
                    .map(|(_, index)| *index)
                    .collect::<Vec<_>>(),
                (5..10).collect::<Vec<_>>()
            );
            assert_eq!(
                results_3
                    .iter()
                    .map(|(_, index)| *index)
                    .collect::<Vec<_>>(),
                (10..15).collect::<Vec<_>>()
            );
        }

        for (when, _) in results_1.iter() {
            for (when_2, _) in results_2.iter() {
                assert!(
                    when < when_2,
                    "{} - {}",
                    chrono_format_dt(*when),
                    chrono_format_dt(*when_2)
                );
            }
        }
        for (when_2, _) in results_2.iter() {
            for (when_3, _) in results_3.iter() {
                assert!(
                    when_2 < when_3,
                    "{} - {}",
                    chrono_format_dt(*when_2),
                    chrono_format_dt(*when_3)
                );
            }
        }
    }
}
