use std::{borrow::Cow, future::Future};

use deadpool_redis::redis::{FromRedisValue, ToRedisArgs};
use once_cell::sync::Lazy;

use super::batch::{RedisBatch, RedisBatchFire, RedisBatchReturningOps};
use crate::{errors::prelude::*, redis::RedisScript};

/// Wrapper around a lazy redis connection.
pub struct RedisConn<'a> {
    pub(crate) prefix: &'a str,
    pool: &'a deadpool_redis::Pool,
    conn: Option<deadpool_redis::Connection>,
}

impl std::fmt::Debug for RedisConn<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RedisConn")
            .field("prefix", &self.prefix)
            .field("pool", &self.pool)
            .field("conn", &self.conn.is_some())
            .finish()
    }
}

/// Public methods for RedisConn.
impl<'a> RedisConn<'a> {
    /// Get an internal connection from the pool, connections are kept in the pool for reuse.
    /// If redis is acting up and unavailable, this will return None.
    /// NOTE: this mainly is used internally, but provides a fallback to the underlying connection, if the exposed interface does not provide options that fit an external user need (which could definitely happen).
    pub async fn get_inner_conn(&mut self) -> Option<&mut deadpool_redis::Connection> {
        if self.conn.is_none() {
            match self.pool.get().await {
                Ok(conn) => self.conn = Some(conn),
                Err(e) => {
                    tracing::error!("Could not get redis connection: {}", e);
                    return None;
                }
            }
        }
        self.conn.as_mut()
    }

    /// Ping redis, returning true if it's up.
    pub async fn ping(&mut self) -> bool {
        if let Some(conn) = self.get_inner_conn().await {
            redis::cmd("PING")
                .query_async::<_, String>(conn)
                .await
                .is_ok()
        } else {
            false
        }
    }

    /// A simple rate_limiter/backoff helper.
    /// Can be used to protect against repeated attempts in quick succession.
    /// Once `start_delaying_after_attempt` is hit, the operation delay will multiplied by the multiplier each time.
    /// Only once no call is made for the duration of the current delay (so current delay doubled) will the attempt number reset to zero.
    ///
    /// Arguments:
    /// - `namespace`: A unique identifier for the endpoint, e.g. user-login.
    /// - `caller_identifier`: A unique identifier for the caller, e.g. a user id.
    /// - `start_delaying_after_attempt`: The number of attempts before the delays start being imposed.
    /// - `initial_delay`: The initial delay to impose.
    /// - `multiplier`: The multiplier to apply, `(attempt-start_delaying_after_attempt) * multiplier * initial_delay = delay`.
    ///
    /// Returns:
    /// - `None`: Continue with the operation.
    /// - `Some<chrono::Duration>`: Retry after the duration.
    pub async fn rate_limiter(
        &mut self,
        namespace: &str,
        caller_identifier: &str,
        start_delaying_after_attempt: usize,
        initial_delay: chrono::Duration,
        multiplier: f64,
    ) -> Option<chrono::Duration> {
        static LUA_BACKOFF_SCRIPT: Lazy<RedisScript> =
            Lazy::new(|| RedisScript::new(include_str!("lua_scripts/backoff_protector.lua")));

        let final_key = self.final_key(namespace, caller_identifier.into());
        let result = self
            .batch()
            .script::<i64>(
                LUA_BACKOFF_SCRIPT
                    .invoker()
                    .key(final_key)
                    .arg(start_delaying_after_attempt)
                    .arg(initial_delay.num_milliseconds())
                    .arg(multiplier),
            )
            .fire()
            .await;

        if let Some(result) = result {
            if result > 0 {
                Some(chrono::Duration::milliseconds(result))
            } else {
                None
            }
        } else {
            None
        }
    }

    /// Get a new [`RedisBatch`] for this connection that commands can be piped together with.
    pub fn batch<'ref_lt>(&'ref_lt mut self) -> RedisBatch<'ref_lt, 'a, '_, ()> {
        RedisBatch::new(self)
    }

    /// Redis keys are all prefixed, use this to finalise a namespace outside of built in commands, e.g. for use in a custom script.
    #[inline]
    pub fn final_namespace(&self, namespace: &str) -> String {
        format!("{}:{}", self.prefix, namespace)
    }

    /// Redis keys are all prefixed, use this to finalise a key outside of built in commands, e.g. for use in a custom script.
    #[inline]
    pub fn final_key(&self, namespace: &str, key: Cow<'_, str>) -> String {
        format!("{}:{}", self.final_namespace(namespace), key)
    }

    /// Cache an async function in redis with an optional expiry.
    /// If already stored, the cached value will be returned, otherwise the function will be stored in redis for next time.
    ///
    /// If redis is unavailable, or the existing contents at the key is wrong, the function output will be used.
    /// The only error coming out of here should be something wrong with the external callback.
    ///
    /// Expiry accurate to a millisecond.
    #[inline]
    pub async fn cached_fn<'b, T, Fut, K: Into<Cow<'b, str>>>(
        &mut self,
        namespace: &str,
        key: K,
        expiry: Option<chrono::Duration>,
        cb: impl FnOnce() -> Fut,
    ) -> RResult<T, AnyErr>
    where
        T: FromRedisValue + ToRedisArgs,
        Fut: Future<Output = Result<T, AnyErr>>,
    {
        let key: Cow<'b, str> = key.into();

        let cached = self
            .batch()
            .get::<T>(namespace, &key)
            .fire()
            .await
            .flatten();
        if let Some(cached) = cached {
            Ok(cached)
        } else {
            let val = cb().await?;
            self.batch().set(namespace, &key, &val, expiry).fire().await;
            Ok(val)
        }
    }
}

/// Private (public inside crate)
impl<'a> RedisConn<'a> {
    pub(crate) fn new(pool: &'a deadpool_redis::Pool, prefix: &'a str) -> Self {
        Self {
            pool,
            prefix,
            conn: None,
        }
    }
}
