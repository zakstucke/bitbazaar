/// Sleep for a duration, compatible with both WASM and native targets.
///
/// Wasm: uses gloo_timers::future::TimeoutFuture
/// Native: uses tokio::time::sleep
pub async fn sleep_compat(duration: std::time::Duration) {
    #[cfg(target_arch = "wasm32")]
    gloo_timers::future::TimeoutFuture::new(duration.as_millis()).await;
    #[cfg(not(target_arch = "wasm32"))]
    tokio::time::sleep(duration).await;
}
