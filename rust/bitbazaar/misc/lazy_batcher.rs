use futures::Future;
use std::sync::Arc;

use cfg_if::cfg_if;
use chrono::TimeDelta;
use futures::future::{BoxFuture, LocalBoxFuture};
use parking_lot::Mutex;

use crate::misc::sleep_compat;

macro_rules! gen {
    ([$($fn_on_batch_impl_generics:tt)*], [$($fut_on_batch_impl_generics:tt)*], [$($t_impl_generics:tt)*], $box_fut_type:tt, $struct_name:ident, $spawner:ident) => {

        /// Group multiple updates into a single batch.
        /// The async callback will be provided all the values that were passed since the last call.
        /// Being called [`batch_after`] after the first value was passed.
        ///
        /// A pending batch will outlive the LazyBatcher being dropped itself.
        ///
        /// Wasm safe + Send and non Sent variants.
        #[derive(Clone)]
        pub struct $struct_name<T> {
            batch_after: chrono::TimeDelta,
            on_batch: Arc<Box<dyn Fn(Vec<T>) -> $box_fut_type<'static, ()> + $($fn_on_batch_impl_generics)*>>,
            active: Arc<Mutex<Vec<T>>>,
        }

        impl std::fmt::Debug for $struct_name<()> {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.debug_struct(stringify!($struct_name))
                    .field("batch_after", &self.batch_after)
                    .field("active", &self.active)
                    .finish()
            }
        }

        impl<T: $($t_impl_generics)*> $struct_name<T> {
            /// Create a batcher.
            ///
            /// Arguments:
            /// - `batch_after`: The time interval after the first "push" to run the callback
            /// - `on_batch`: The callback to run. Is passed all the values that were pushed since the last call.
            pub fn new<Fut: Future<Output = ()> + 'static+ $($fut_on_batch_impl_generics)*>(
                batch_after: TimeDelta,
                on_batch: impl Fn(Vec<T>) -> Fut + 'static+ $($fn_on_batch_impl_generics)*,
            ) -> Self {
                Self {
                    batch_after,
                    on_batch: Arc::new(Box::new(move |values| Box::pin(on_batch(values)))),
                    active: Arc::new(Mutex::new(Vec::new())),
                }
            }

            /// Push a value to the batcher.
            /// This will be supplied to `on_batch` after `batch_after` has passed since the first push/extension.
            pub fn push(&self, value: T) {
                self.extend(std::iter::once(value));
            }

            /// Push multiple values to the batcher.
            /// These will be supplied to `on_batch` after `batch_after` has passed since the first push/extension.
            pub fn extend(&self, values: impl IntoIterator<Item = T>) {
                // Sub-block to keep themutex lock as short as possible:
                let start_new_batch = {
                    let mut active = self.active.lock();
                    let start_new_batch = active.is_empty();
                    active.extend(values);
                    start_new_batch
                };

                // If this is the first in a new batch, trigger the next run:
                if start_new_batch {
                    {
                        let on_batch = self.on_batch.clone();
                        let active = self.active.clone();
                        let batch_after = self.batch_after;
                        let fut = async move {
                            sleep_compat(batch_after).await;
                            let active = std::mem::take(&mut *active.lock());
                            if !active.is_empty() {
                                (on_batch)(active).await;
                            }
                        };

                        $spawner(fut);
                    }
                }
            }
        }
    }
}

fn spawn_threadsafe_fut(fut: impl futures::Future<Output = ()> + Send + 'static) {
    cfg_if! {
        if #[cfg(target_arch = "wasm32")] {
            wasm_bindgen_futures::spawn_local(fut)
        } else {
            tokio::spawn(fut);
        }
    }
}

fn spawn_local_fut(fut: impl futures::Future<Output = ()> + 'static) {
    cfg_if! {
        if #[cfg(target_arch = "wasm32")] {
            wasm_bindgen_futures::spawn_local(fut)
        } else {
            tokio::task::spawn_local(fut);
        }
    }
}

gen!(
    [Send + Sync],
    [Send],
    [Send + 'static],
    BoxFuture,
    LazyBatcher,
    spawn_threadsafe_fut
);
gen!(
    [],
    [],
    ['static],
    LocalBoxFuture,
    LocalLazyBatcher,
    spawn_local_fut
);

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;
    use tokio::time::Duration;

    macro_rules! gen {
        ($test_name:ident, $struct_name:ident) => {
            #[tokio::test]
            async fn $test_name() {
                // Running inside a local set for the local version with tokio:
                let local = tokio::task::LocalSet::new();
                local
                    .run_until(async move {
                        let counter = Arc::new(AtomicUsize::new(0));
                        let on_batch_calls = Arc::new(AtomicUsize::new(0));

                        let batcher = {
                            let counter = counter.clone();
                            let on_batch_calls = on_batch_calls.clone();
                            let batcher =
                                $struct_name::new(TimeDelta::milliseconds(50), move |values| {
                                    assert!(!values.is_empty(), "Values should be non-empty");
                                    on_batch_calls.fetch_add(1, Ordering::Relaxed);
                                    let counter = counter.clone();
                                    async move {
                                        counter.fetch_add(values.len(), Ordering::Relaxed);
                                    }
                                });

                            batcher.push(1);
                            batcher.push(2);
                            batcher.push(3);
                            batcher
                        };
                        assert_eq!(counter.load(Ordering::Relaxed), 0);

                        tokio::time::sleep(Duration::from_millis(30)).await;
                        assert_eq!(counter.load(Ordering::Relaxed), 0);
                        tokio::time::sleep(Duration::from_millis(30)).await;
                        assert_eq!(counter.load(Ordering::Relaxed), 3);
                        assert_eq!(on_batch_calls.load(Ordering::Relaxed), 1);

                        // Shouldn't re-run or anything accidentally:
                        tokio::time::sleep(Duration::from_millis(100)).await;
                        assert_eq!(on_batch_calls.load(Ordering::Relaxed), 1);
                        assert_eq!(counter.load(Ordering::Relaxed), 3);

                        // On next batch, should run again:
                        batcher.push(4);
                        tokio::time::sleep(Duration::from_millis(100)).await;
                        assert_eq!(on_batch_calls.load(Ordering::Relaxed), 2);
                        assert_eq!(counter.load(Ordering::Relaxed), 4);

                        // Batch should still succeed after drop:
                        batcher.push(5);
                        drop(batcher);
                        tokio::time::sleep(Duration::from_millis(100)).await;
                        assert_eq!(on_batch_calls.load(Ordering::Relaxed), 3);
                        assert_eq!(counter.load(Ordering::Relaxed), 5);
                    })
                    .await;
            }
        };
    }

    gen!(test_lazy_batcher, LazyBatcher);
    gen!(test_lazy_batcher_local, LocalLazyBatcher);
}
