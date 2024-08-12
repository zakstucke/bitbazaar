use std::{
    future::Future,
    sync::{Arc, LazyLock},
};

use dashmap::DashMap;

use crate::prelude::*;

/// APPLIES TO ALL PROCESSES ON HOST MACHINE.
/// If only needing for the current process, use [`global_lock_process_sync`] instead.
///
/// Global lock for a given lock id.
#[cfg(not(target_arch = "wasm32"))]
pub fn global_lock_host_sync<R>(
    lock_id: &str,
    with_lock: impl FnOnce() -> RResult<R, AnyErr>,
) -> RResult<R, AnyErr> {
    let lock =
        named_lock::NamedLock::create(&final_host_lock_id(lock_id)).change_context(AnyErr)?;
    let _guard = lock.lock().change_context(AnyErr)?;
    with_lock()
}

/// APPLIES TO ALL PROCESSES ON HOST MACHINE.
/// If only needing for the current process, use [`global_lock_process_async`] instead.
///
/// Global lock for a given lock id.
#[cfg(not(target_arch = "wasm32"))]
pub async fn global_lock_host_async<R>(
    lock_id: &str,
    with_lock: impl Future<Output = RResult<R, AnyErr>>,
) -> RResult<R, AnyErr> {
    // named_lock only provides a sync interface,
    // which could cause deadlocks in our current process if we're holding a lock and trying to acquire another.
    // So we'll wrap it in our process global lock that uses async mutexes and is safe, to get around this issue in async code.
    global_lock_process_async(lock_id, async move {
        let lock =
            named_lock::NamedLock::create(&final_host_lock_id(lock_id)).change_context(AnyErr)?;
        let _guard = lock.lock().change_context(AnyErr)?;
        with_lock.await
    })
    .await
}

fn final_host_lock_id(lock_id: &str) -> String {
    static HOST_LOCK_PREFIX: &str = "rs_bitbazaar_global_lock_";
    format!("{}{}", HOST_LOCK_PREFIX, lock_id)
}

macro_rules! process_body {
    ($store:expr, $lock_id:expr, $get_result:expr, $get_guard:ident) => {{
        // NOTE: the clone on the arc is very important, it's what's used below to auto cleanup ids from the map.
        let mutex = $store
            .entry($lock_id.to_string())
            .or_insert_with(|| Arc::new(Mutex::new(())))
            .clone();

        let _guard = $get_guard!(mutex);

        let result = $get_result;

        // We want to auto clear up the global map of ids, to prevent a theoretical memory leak.
        // If there's no one waiting for the lock, the arc count should be 2: 1 for that in the map, 1 for the clone we're holding in here.
        // If it's 2, we can remove it from the map.
        if Arc::strong_count(&mutex) == 2 {
            if let Some((key, value)) = $store.remove(&$lock_id.to_string()) {
                // Theoretically someone could grab the lock between the check and removing here on a different thread,
                // so still re-add if the count isn't 2 afterwards, to prevent the current run using a mutex that's just been removed from the global hashmap.
                // this is probably insanely unlikely, it's also "possible" but probably even more insanely unlikely another caller called between the remove and this re-insert, but given this approach the handling for this would be infinitely recursive.
                if Arc::strong_count(&mutex) != 2 {
                    $store.insert(key, value);
                }
            }
        }

        result
    }};
}

static STORE_SYNC_PROCESS: LazyLock<DashMap<String, Arc<parking_lot::Mutex<()>>>> =
    LazyLock::new(DashMap::new);

/// APPLIES TO ONLY THE CURRENT PROCESS.
/// If wanting to apply to all processes on the host machine, use [`global_lock_host`] instead.
///
/// Global lock for a given lock id.
pub fn global_lock_process_sync<R>(lock_id: &str, with_lock: impl FnOnce() -> R) -> R {
    use parking_lot::Mutex;

    macro_rules! get_guard {
        ($mutex:expr) => {
            $mutex.lock()
        };
    }

    process_body!(STORE_SYNC_PROCESS, lock_id, with_lock(), get_guard)
}

static STORE_ASYNC_PROCESS: LazyLock<DashMap<String, Arc<tokio::sync::Mutex<()>>>> =
    LazyLock::new(DashMap::new);

/// APPLIES TO ONLY THE CURRENT PROCESS.
/// If wanting to apply to all processes on the host machine, use [`global_lock_host`] instead.
///
/// Global lock for a given lock id.
pub async fn global_lock_process_async<R>(lock_id: &str, with_lock: impl Future<Output = R>) -> R {
    // Async and can be held across await points, need tokio mutex instead.
    use tokio::sync::Mutex;

    macro_rules! get_guard {
        ($mutex:expr) => {
            $mutex.lock().await
        };
    }

    process_body!(STORE_ASYNC_PROCESS, lock_id, with_lock.await, get_guard)
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use super::*;

    use crate::testing::prelude::*;

    /// Confirm correct when spawning onto different tokio threads.
    /// Testing all 4 variants as technically applies to all given multiple os threads with tokio.
    #[rstest]
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn test_global_lock_tokio_spawn() {
        macro_rules! test {
            ($to_run:expr) => {
                let elapsed = Instant::now();
                let mut handles = vec![];
                for _ in 0..2 {
                    let handle = tokio::spawn(async move {
                        assert_eq!($to_run.unwrap(), 42);
                    });
                    handles.push(handle);
                }
                for handle in handles {
                    handle.await.unwrap();
                }
                let millis_elapsed = elapsed.elapsed().as_millis();
                assert!(millis_elapsed >= 200, "{}", millis_elapsed);
                assert!(millis_elapsed < 220, "{}", millis_elapsed);
            };
        }
        test!(
            global_lock_host_async("test_global_lock_tokio_spawn_host_async", async {
                tokio::time::sleep(Duration::from_millis(100)).await;
                Ok(42)
            })
            .await
        );
        test!(
            global_lock_process_async("test_global_lock_tokio_spawn_process_async", async {
                tokio::time::sleep(Duration::from_millis(100)).await;
                Ok::<_, AnyErr>(42)
            })
            .await
        );
        test!(global_lock_host_sync(
            "test_global_lock_tokio_spawn_host_sync",
            || {
                std::thread::sleep(Duration::from_millis(100));
                Ok(42)
            }
        ));

        test!(global_lock_process_sync(
            "test_global_lock_tokio_spawn_process_sync",
            || {
                std::thread::sleep(Duration::from_millis(100));
                Ok::<_, AnyErr>(42)
            }
        ));
    }

    /// Processes use a global map of ids to mutexes.
    /// Ids should only persist whilst contended.
    /// Specifically, should always be cleaned up, unless there's a second caller already waiting on the mutex.
    /// Tests both sync and async process variants.
    #[rstest]
    #[tokio::test]
    async fn test_global_lock_process_id_map_cleanup() -> RResult<(), AnyErr> {
        let lock_id = "test_global_lock_process_id_map_cleanup";

        // ASYNC:
        // No contention, should be cleaned up instantly after running:
        global_lock_process_async(lock_id, async {
            tokio::time::sleep(Duration::from_millis(5)).await;
            Ok::<_, AnyErr>(42)
        })
        .await?;
        assert_eq!(STORE_ASYNC_PROCESS.len(), 0);

        // SYNC:
        // No contention, should be cleaned up instantly after running:
        global_lock_process_sync(lock_id, || {
            std::thread::sleep(Duration::from_millis(5));
            Ok::<_, AnyErr>(42)
        })?;
        assert_eq!(STORE_SYNC_PROCESS.len(), 0);

        // ASYNC:
        // Contention, until there aren't any more callers waiting on the lock, the id should persist, then it should be cleaned up:
        let (a, b, c, d) = tokio::join!(
            global_lock_process_async(lock_id, async {
                tokio::time::sleep(Duration::from_millis(100)).await;
                Ok::<_, AnyErr>(42)
            }),
            global_lock_process_async(lock_id, async {
                tokio::time::sleep(Duration::from_millis(100)).await;
                Ok::<_, AnyErr>(42)
            }),
            global_lock_process_async(lock_id, async {
                tokio::time::sleep(Duration::from_millis(100)).await;
                Ok::<_, AnyErr>(42)
            }),
            async move {
                tokio::time::sleep(Duration::from_millis(10)).await;
                let elapsed = Instant::now();
                while elapsed.elapsed().as_millis() < 250 {
                    assert_eq!(STORE_ASYNC_PROCESS.len(), 1);
                    tokio::time::sleep(Duration::from_millis(1)).await;
                }
                Ok::<_, AnyErr>(42)
            }
        );
        a?;
        b?;
        c?;
        d?;
        // No more waiting, should clean up now:
        assert_eq!(STORE_ASYNC_PROCESS.len(), 0);

        // SYNC:
        // Contention, until there aren't any more callers waiting on the lock, the id should persist, then it should be cleaned up:
        let a = std::thread::spawn(|| {
            global_lock_process_sync(lock_id, || {
                std::thread::sleep(Duration::from_millis(100));
                Ok::<_, AnyErr>(42)
            })
            .unwrap();
        });
        let b = std::thread::spawn(|| {
            global_lock_process_sync(lock_id, || {
                std::thread::sleep(Duration::from_millis(100));
                Ok::<_, AnyErr>(42)
            })
            .unwrap();
        });
        let c = std::thread::spawn(|| {
            global_lock_process_sync(lock_id, || {
                std::thread::sleep(Duration::from_millis(100));
                Ok::<_, AnyErr>(42)
            })
            .unwrap();
        });
        let d = std::thread::spawn(|| {
            let elapsed = Instant::now();
            while elapsed.elapsed().as_millis() < 250 {
                // Sleep should be before in case this thread is the first to run, in which case would be 0:
                std::thread::sleep(Duration::from_millis(5));
                assert_eq!(STORE_SYNC_PROCESS.len(), 1);
            }
        });
        a.join().unwrap();
        b.join().unwrap();
        c.join().unwrap();
        d.join().unwrap();
        // No more waiting, should clean up now:
        assert_eq!(STORE_SYNC_PROCESS.len(), 0);

        Ok(())
    }
}
