use std::time::Duration;

use deadpool_redis::{Config, Runtime};
use futures::Future;

use super::{RedisConn, RedisLock, RedisLockErr};
use crate::errors::prelude::*;

/// A wrapper around redis to make it more concise to use and not need redis in the downstream Cargo.toml.
///
/// This wrapper attempts to return very few errors to help build in automatic redis failure handling into downstream code.
/// All redis errors (availability, unexpected content) will be logged as errors and results returned as `None` (or similar) where possible.
#[derive(Debug, Clone)]
pub struct Redis {
    pool: deadpool_redis::Pool,
    prefix: String,
}

impl Redis {
    /// Create a new global redis wrapper from the given Redis URL (like `redis://127.0.0.1`).
    ///
    /// Note this should only be done once at startup.
    pub fn new<A: Into<String>, B: Into<String>>(
        redis_conn_str: A,
        prefix: B,
    ) -> RResult<Self, AnyErr> {
        let cfg = Config::from_url(redis_conn_str);
        let pool = cfg
            .create_pool(Some(Runtime::Tokio1))
            .change_context(AnyErr)?;

        Ok(Self {
            pool,
            prefix: prefix.into(),
        })
    }

    /// Get a [`RedisConn`] redis can be called with.
    pub fn conn(&self) -> RedisConn<'_> {
        RedisConn::new(&self.pool, &self.prefix)
    }

    /// Get a distributed redis lock.
    ///
    /// This lock will prevent others getting the lock, until it's time to live expires. Or the lock is manually released with [`RedisLock::unlock`].
    ///
    /// Arguments:
    /// - `namespace`: The redis key namespace to use.
    /// - `lock_key`: The resource to lock. Will be used as the key in Redis.
    /// - `ttl`: The time to live for this lock. After this time, the lock will be automatically released.
    /// - `wait_up_to`: if the lock is busy elsewhere, wait this long trying to get it, before giving up and returning [`RedisLockErr::Unavailable`].
    pub async fn dlock(
        &self,
        namespace: &'static str,
        lock_key: &str,
        time_to_live: Duration,
        wait_up_to: Option<Duration>,
    ) -> RResult<RedisLock<'_>, RedisLockErr> {
        RedisLock::new(self, namespace, lock_key, time_to_live, wait_up_to).await
    }

    /// Get a distributed redis lock that is held for the duration of the closure.
    /// The lock will be automatically released when the closure finishes.
    ///
    /// Arguments:
    /// - `namespace`: The redis key namespace to use.
    /// - `lock_key`: The resource to lock. Will be used as the key in Redis.
    /// - `wait_up_to`: if the lock is busy elsewhere, wait this long trying to get it, before giving up and returning [`RedisLockErr::Unavailable`].
    pub async fn dlock_for_fut<R, Fut: Future<Output = RResult<R, AnyErr>>>(
        &self,
        namespace: &str,
        lock_key: &str,
        wait_up_to: Option<Duration>,
        fut: Fut,
    ) -> RResult<R, RedisLockErr> {
        let mut lock = RedisLock::new(
            self,
            namespace,
            lock_key,
            // 3 works well with hold_for_closure internals, means will lock again after 2 seconds, then 5, then double current processing time.
            // (albeit only if the closure hasn't already finished)
            Duration::from_secs(3),
            wait_up_to,
        )
        .await?;
        let result = lock.hold_for_fut(fut).await;
        // Always unlock, would expire eventually, but allows others to access straight away:
        lock.unlock().await;
        result
    }

    /// Escape hatch, access the inner deadpool_redis pool.
    pub fn get_inner_pool(&self) -> &deadpool_redis::Pool {
        &self.pool
    }

    /// Used for dlock, the dlock algo is setup with multiple servers in mind, and synchronising locking between them.
    /// It's a good, future proofed algo, so keeping the multi interface despite the current implementation only using one server.
    pub fn get_conn_to_each_server(&self) -> Vec<RedisConn<'_>> {
        vec![self.conn()]
    }
}
