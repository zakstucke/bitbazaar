use crate::prelude::*;

/// The following link explains the distinction between normal tokio, [`tokio::task::spawn_blocking`], and rayon.
/// For most cases you think to use [`tokio::task::spawn_blocking`], but isn't actual IO like file read/write that isn't async, and actually CPU bound stuff, rayon should be used.
/// It goes without saying, if you ever want to use anything from rayon like [`rayon::iter::ParallelIterator`], it should be inside [`run_cpu_intensive`]
/// <https://ryhl.io/blog/async-what-is-blocking/#the-rayon-crate>
///
/// This also makes sure to maintain the active tracing span context across into the rayon block.
pub async fn run_cpu_intensive<R: Send + 'static>(
    cb: impl FnOnce() -> RResult<R, AnyErr> + Send + 'static,
) -> RResult<R, AnyErr> {
    // Rayon runs completely separately to this thread/span.
    // By creating one outside, then using it inside, we can connect up the 2 worlds in regards to tracing:
    // (this one is still in tokio/same thread, so will automatically connect to parent, we can then move this created span into the task to actually start there instead)
    let connector_span = tracing::span!(tracing::Level::INFO, "run_cpu_intensive");

    // The way we'll get the result back from rayon:
    let (send, recv) = tokio::sync::oneshot::channel();

    // Spawn a task on rayon.
    rayon::spawn(move || {
        // Run inside the original span to connect them up:
        connector_span.in_scope(move || {
            // Run the expensive function and send back to tokio:
            let _ = send.send(cb());
        })
    });

    // Wait for the rayon task's result:
    recv.await.change_context(AnyErr)?
}

// #[cfg(test)]
// mod tests {
//     use bitbazaar::log::GlobalLog;
//     use parking_lot::Mutex;

//     use crate::testing::prelude::*;

//     use super::*;

//     /// Need to confirm span contexts are maintained when transitioning into rayon from tokio:
//     #[rstest]
//     #[tokio::test]
//     async fn test_run_cpu_intensive_spans_maintained() -> RResult<(), AnyErr> {
//         // TODO this is silly to do like this Arc<Mutex>> would work if we change the callback signature, we can do better upstream now.
//         static LOGS: LazyLock<Mutex<Vec<String>>> = LazyLock::new(Mutex::default);
//         GlobalLog::builder()
//             .custom(false, false, false, false, |log| {
//                 LOGS.lock()
//                     .push(String::from_utf8_lossy(log).trim().to_string());
//             })
//             .level_from(tracing::Level::DEBUG)?
//             .build()?
//             .register_global()?;

//         #[tracing::instrument(level = "INFO")]
//         fn inner_span() {
//             tracing::info!("Nested span inside rayon.");
//         }
//         #[tracing::instrument(level = "INFO")]
//         async fn outer_span() -> RResult<(), AnyErr> {
//             tracing::info!("Parent span outside rayon.");
//             run_cpu_intensive(|| {
//                 tracing::info!("Inside rayon no nested span.");
//                 inner_span();
//                 Ok(())
//             })
//             .await
//         }
//         outer_span().await?;

//         assert_eq!(
//             LOGS.try_lock_for(std::time::Duration::from_secs(1))
//                 .unwrap()
//                 .clone(),
//             vec![
//                 "INFO outer_span: Parent span outside rayon.",
//                 "INFO outer_span:run_cpu_intensive: Inside rayon no nested span.",
//                 "INFO outer_span:run_cpu_intensive:inner_span: Nested span inside rayon."
//             ]
//         );
//         Ok(())
//     }
// }

// Haven't been able to get something like this to work yet.
// use std::sync::{atomic::AtomicBool, Arc};

// use bitbazaar::log::record_exception;
// use parking_lot::Mutex;
// pub fn run_external_multithreaded<R: Send + 'static>(
//     max_threads: u16,
//     cb: impl FnOnce(u16) -> RResult<R, AnyErr> + Send + 'static,
// ) -> RResult<R, AnyErr> {
//     let max_threads = max_threads
//         .min(rayon::current_num_threads() as u16) // Never use more threads than rayon configured for.
//         .max(1); // Always make sure at least one thread is allowed.

//     if max_threads == 1 {
//         // This is a no-op, just run the callback and return:
//         cb(1)
//     } else {
//         // We need the Thread objects from each of the "locked" threads, so we can "unlock" at the end:
//         let (parked_tx, parked_rx) = std::sync::mpsc::channel::<std::thread::Thread>();

//         let total_locked = Arc::new(Mutex::new(1));
//         let locking_complete = Arc::new(AtomicBool::new(false));

//         // Quoting from: https://docs.rs/rayon/latest/rayon/struct.ThreadPool.html#method.broadcast
//         // Broadcasts are executed on each thread after they have exhausted their local work queue, before they attempt work-stealing from other threads.
//         // The goal of that strategy is to run everywhere in a timely manner without being too disruptive to current work.
//         // There may be alternative broadcast styles added in the future for more or less aggressive injection, if the need arises.
//         let main_thread_id = std::thread::current().id();
//         let total_locked_clone = total_locked.clone();
//         let locking_complete_clone = locking_complete.clone();
//         rayon::spawn(move || {
//             let spawn_thread_id = std::thread::current().id();
//             rayon::broadcast(|_ctx| {
//                 let cur_thread = std::thread::current();
//                 // This one is intrinsically locked and shouldn't be parked.
//                 if cur_thread.id() == main_thread_id || cur_thread.id() == spawn_thread_id {
//                     return;
//                 }

//                 // Don't keep locking after the locking period has finished, these threads were busy:
//                 if locking_complete_clone.load(std::sync::atomic::Ordering::Relaxed) {
//                     return;
//                 }

//                 // Update the locked threads, or return if we've reached the max:
//                 // (in block to drop the mutex as fast as possible)
//                 {
//                     let mut tl = total_locked_clone.lock();
//                     if *tl >= max_threads {
//                         // Mark locking complete given we've reached the max:
//                         locking_complete_clone.store(true, std::sync::atomic::Ordering::Relaxed);
//                         return;
//                     }
//                     *tl += 1;
//                 }
//                 // We want to "remove" this thread from rayon until the callback completes,
//                 // we'll do that by parking it, then unparking it at the end.
//                 let thread = std::thread::current();
//                 match parked_tx.send(thread) {
//                     Ok(()) => std::thread::park(),
//                     Err(e) => {
//                         record_exception("Failed to send thread to parked_tx", format!("{:?}", e))
//                     }
//                 }
//             });
//         });

//         // Wait up to 10ms, or until max_threads reached, for the locking period:
//         let start = std::time::Instant::now();
//         while !locking_complete.load(std::sync::atomic::Ordering::Relaxed)
//             && start.elapsed() < std::time::Duration::from_millis(10)
//         {
//             tracing::info!("WAITING FOR LOCKING TO COMPLETE...");
//             rayon::yield_local();
//         }
//         locking_complete.store(true, std::sync::atomic::Ordering::Relaxed);

//         let threads_to_allow = (*total_locked.lock()).max(1);

//         // Run the callback whilst the threads are "locked".
//         let result = cb(threads_to_allow);

//         // Unpark/"unlock" all parked/"locked" threads:
//         while let Ok(thread) = parked_rx.try_recv() {
//             thread.unpark();
//         }

//         result
//     }
// }
