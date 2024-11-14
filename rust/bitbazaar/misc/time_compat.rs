use crate::chrono::chrono_format_td;

/// Sleep for a duration, compatible with both WASM and native targets.
///
/// Wasm: uses gloo_timers::future::TimeoutFuture
/// Native: uses tokio::time::sleep
pub async fn sleep_compat(timedelta: chrono::TimeDelta) {
    // The SendWrapper fixes the js waiter not being send.
    // https://github.com/rustwasm/gloo/issues/109
    #[cfg(target_arch = "wasm32")]
    send_wrapper::SendWrapper::new(gloo_timers::future::TimeoutFuture::new(
        timedelta.num_milliseconds().min(u32::MAX as i64) as u32,
    ))
    .await;
    #[cfg(not(target_arch = "wasm32"))]
    tokio::time::sleep(timedelta.to_std().unwrap_or_default()).await;
}

/// A wasm compatible version of std::time::Instant.
/// Uses chrono::TimeDelta for elapsed.
pub struct InstantCompat {
    #[cfg(target_arch = "wasm32")]
    inner: web_time::Instant,
    #[cfg(not(target_arch = "wasm32"))]
    inner: std::time::Instant,
}

impl std::fmt::Debug for InstantCompat {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let elapsed = chrono_format_td(self.elapsed(), true);
        f.debug_struct("InstantCompat")
            .field("elapsed", &elapsed)
            .finish()
    }
}

impl InstantCompat {
    /// Create a new InstantCompat.
    pub fn now() -> Self {
        #[cfg(target_arch = "wasm32")]
        let inner = web_time::Instant::now();
        #[cfg(not(target_arch = "wasm32"))]
        let inner = std::time::Instant::now();
        Self { inner }
    }

    /// Get the elapsed time since creation.
    pub fn elapsed(&self) -> chrono::TimeDelta {
        #[cfg(target_arch = "wasm32")]
        let elapsed = self.inner.elapsed().as_millis() as i64;
        #[cfg(not(target_arch = "wasm32"))]
        let elapsed = self.inner.elapsed().as_millis() as i64;
        chrono::TimeDelta::milliseconds(elapsed)
    }
}
