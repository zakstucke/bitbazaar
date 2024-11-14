use std::sync::Arc;

use chrono::TimeDelta;
use dashmap::DashSet;
use deadpool_redis::{Config, Runtime};
use futures::{Future, FutureExt};
use redis::RedisError;

use super::{pubsub::pubsub_global::RedisPubSubGlobal, RedisConn, RedisLock, RedisLockErr};
use crate::{errors::prelude::*, misc::Retry};

/// A wrapper around redis to make it more concise to use and not need redis in the downstream Cargo.toml.
///
/// This wrapper attempts to return very few errors to help build in automatic redis failure handling into downstream code.
/// All redis errors (availability, unexpected content) will be logged as errors and results returned as `None` (or similar) where possible.
#[derive(Debug, Clone)]
pub struct Redis {
    // deadpool arced internally.
    pool: deadpool_redis::Pool,
    pub(crate) pubsub_listener: Arc<RedisPubSubGlobal>,
    prefix: String,

    // Used to track which scripts have been called already successfully with EVAL rather than EVALSHA.
    // This allows us to avoid the noscripterror and double pipeline send in all cases but:
    // - Redis is restarted whilst the server is running.
    // This is nice as it makes testing simpler, and does cover most cases, but should still assume as a user commands might be double sent.
    // (String is the hash of the script)
    scripts_loaded: Arc<DashSet<String>>,
}

impl Redis {
    /// Create a new global redis wrapper from the given Redis URL (like `redis://127.0.0.1`).
    ///
    /// Note this should only be done once at startup.
    pub fn new(
        redis_conn_str: impl Into<String>,
        prefix: impl Into<String>,
    ) -> RResult<Self, AnyErr> {
        let redis_conn_str = redis_conn_str.into();
        let prefix = prefix.into();
        let cfg = Config::from_url(&redis_conn_str);
        let pool = cfg
            .create_pool(Some(Runtime::Tokio1))
            .change_context(AnyErr)?;
        let pubsub_listener = Arc::new(RedisPubSubGlobal::new(&redis_conn_str)?);
        Ok(Self {
            pool,
            prefix,
            pubsub_listener,
            scripts_loaded: Arc::new(DashSet::new()),
        })
    }

    /// Get a [`RedisConn`] redis can be called with.
    pub fn conn(&self) -> RedisConn<'_> {
        RedisConn::new(
            &self.pool,
            &self.prefix,
            &self.pubsub_listener,
            &self.scripts_loaded,
        )
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
        time_to_live: TimeDelta,
        wait_up_to: Option<TimeDelta>,
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
        wait_up_to: Option<TimeDelta>,
        fut: Fut,
    ) -> RResult<R, RedisLockErr> {
        let mut lock = RedisLock::new(
            self,
            namespace,
            lock_key,
            // 3 works well with hold_for_closure internals, means will lock again after 2 seconds, then 5, then double current processing time.
            // (albeit only if the closure hasn't already finished)
            TimeDelta::seconds(3),
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

    /// Used by the [`redis::aio::ConnectionLike`] implementation.
    async fn connectionlike_conn(&mut self) -> Result<deadpool_redis::Connection, RedisError> {
        let conn = Retry::fibonacci(TimeDelta::milliseconds(50)).until_total_delay(TimeDelta::seconds(60)).on_retry(|info| {
                tracing::info!(
                    "Redis connection for apalis failed, retrying. Last attempt no: '{}'. Error: \n{:?}",
                    info.last_attempt_no,
                    info.last_error
                );
                None
            }).call(|| self.pool.get())
            .await.map_err(|e|         {
                match e {
                    deadpool_redis::PoolError::Backend(e) => e,
                    e => RedisError::from((
                        redis::ErrorKind::ResponseError,
                        "Pool -> redis conn error",
                        format!("{:?}", e),
                    )),
                }
            })?;
        Ok(conn)
    }
}

/// To make some external usage easier to use on top of a deadpool pool, we'll implement ConnectionLike for the redis object itself.
/// This is useful for e.g. apalis.
impl redis::aio::ConnectionLike for Redis {
    fn req_packed_command<'a>(
        &'a mut self,
        cmd: &'a redis::Cmd,
    ) -> redis::RedisFuture<'a, redis::Value> {
        async move {
            self.connectionlike_conn()
                .await?
                .req_packed_command(cmd)
                .await
        }
        .boxed()
    }

    fn req_packed_commands<'a>(
        &'a mut self,
        cmd: &'a redis::Pipeline,
        offset: usize,
        count: usize,
    ) -> redis::RedisFuture<'a, Vec<redis::Value>> {
        async move {
            self.connectionlike_conn()
                .await?
                .req_packed_commands(cmd, offset, count)
                .await
        }
        .boxed()
    }

    fn get_db(&self) -> i64 {
        // Redis lib itself uses 0 for clusters, we don't have a valid value for this, so copying them and returning 0.
        // https://github.com/redis-rs/redis-rs/blob/186841ae51f21192d7c2975509ff73b3a33c29a3/redis/src/cluster.rs#L907
        0
    }
}
