/// Sleep for a duration, compatible with both WASM and native targets.
///
/// Wasm: uses gloo_timers::future::TimeoutFuture
/// Native: uses tokio::time::sleep
pub async fn sleep_compat(duration: chrono::Duration) {
    #[cfg(target_arch = "wasm32")]
    gloo_timers::future::TimeoutFuture::new(
        duration.num_milliseconds().min(u32::MAX as i64) as u32
    )
    .await;
    #[cfg(not(target_arch = "wasm32"))]
    tokio::time::sleep(duration.to_std().unwrap_or_default()).await;
}
