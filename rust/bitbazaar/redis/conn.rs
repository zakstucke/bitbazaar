#![allow(async_fn_in_trait)]

use std::{
    borrow::Cow,
    future::Future,
    sync::{Arc, LazyLock},
};

use deadpool_redis::redis::{FromRedisValue, ToRedisArgs};

use super::batch::{RedisBatch, RedisBatchFire, RedisBatchReturningOps};
use crate::{errors::prelude::*, log::record_exception, redis::RedisScript};

/// Wrapper around a lazy redis connection.
pub struct RedisConn<'a> {
    pub(crate) prefix: &'a str,
    pool: &'a deadpool_redis::Pool,
    conn: Option<deadpool_redis::Connection>,
}

impl<'a> RedisConn<'a> {
    pub(crate) fn new(pool: &'a deadpool_redis::Pool, prefix: &'a str) -> Self {
        Self {
            pool,
            prefix,
            conn: None,
        }
    }
}

// Cloning is still technically heavy for the un-owned, as the active connection can't be reused.
impl<'a> Clone for RedisConn<'a> {
    fn clone(&self) -> Self {
        Self {
            prefix: self.prefix,
            pool: self.pool,
            conn: None,
        }
    }
}

/// An owned variant of [`RedisConn`]. Useful when parent struct lifetimes get out of hand.
/// [`RedisConn`] is better, so keeping local in crate until a real need for it outside.
/// (requires pool arc cloning, and prefix string cloning, so slightly less efficient).
pub struct RedisConnOwned {
    // Prefix and pool both Arced now at top level for easy cloning.
    // The conn will be reset to None on each clone, so it's a very heavy object I think.
    pub(crate) prefix: Arc<String>,
    pool: deadpool_redis::Pool,
    conn: Option<deadpool_redis::Connection>,
}

impl Clone for RedisConnOwned {
    fn clone(&self) -> Self {
        Self {
            prefix: self.prefix.clone(),
            pool: self.pool.clone(),
            conn: None,
        }
    }
}

macro_rules! impl_debug_for_conn {
    ($conn_type:ty, $name:literal) => {
        impl std::fmt::Debug for $conn_type {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.debug_struct($name)
                    .field("prefix", &self.prefix)
                    .field("pool", &self.pool)
                    .field("conn", &self.conn.is_some())
                    .finish()
            }
        }
    };
}

impl_debug_for_conn!(RedisConn<'_>, "RedisConn");
impl_debug_for_conn!(RedisConnOwned, "RedisConnOwned");

/// Generic methods over the RedisConn and RedisConnOwned types.
pub trait RedisConnLike: std::fmt::Debug + Send + Sized {
    /// Get an internal connection from the pool, connections are kept in the pool for reuse.
    /// If redis is acting up and unavailable, this will return None.
    /// NOTE: this mainly is used internally, but provides a fallback to the underlying connection, if the exposed interface does not provide options that fit an external user need (which could definitely happen).
    async fn get_inner_conn(&mut self) -> Option<&mut deadpool_redis::Connection>;

    /// Get the redis configured prefix.
    fn prefix(&self) -> &str;

    /// Convert to the owned variant.
    fn into_owned(self) -> RedisConnOwned;

    /// Ping redis, returning true if it's up.
    async fn ping(&mut self) -> bool {
        if let Some(conn) = self.get_inner_conn().await {
            redis::cmd("PING").query_async::<String>(conn).await.is_ok()
        } else {
            false
        }
    }

    // Commented out as untested, not sure if works.
    // /// Get all data from redis, only really useful during testing.
    // ///
    // async fn dev_all_data(&mut self) -> HashMap<String, redis::Value> {
    //     if let Some(conn) = self.get_inner_conn().await {
    //         let mut cmd = redis::cmd("SCAN");
    //         cmd.arg(0);
    //         let mut data = HashMap::new();
    //         loop {
    //             let (next_cursor, keys): (i64, Vec<String>) = cmd.query_async(conn).await.unwrap();
    //             for key in keys {
    //                 let val: redis::Value =
    //                     redis::cmd("GET").arg(&key).query_async(conn).await.unwrap();
    //                 data.insert(key, val);
    //             }
    //             if next_cursor == 0 {
    //                 break;
    //             }
    //             cmd.arg(next_cursor);
    //         }
    //         data
    //     } else {
    //         HashMap::new()
    //     }
    // }

    /// Flush the whole redis cache, will delete all data.
    async fn dev_flushall(&mut self, sync: bool) -> Option<String> {
        if let Some(conn) = self.get_inner_conn().await {
            let mut cmd = redis::cmd("FLUSHALL");
            if sync {
                cmd.arg("SYNC");
            } else {
                cmd.arg("ASYNC");
            }
            match cmd.query_async::<String>(conn).await {
                Ok(s) => Some(s),
                Err(e) => {
                    record_exception("Failed to reset redis cache.", format!("{:?}", e));
                    None
                }
            }
        } else {
            None
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
    async fn rate_limiter(
        &mut self,
        namespace: &str,
        caller_identifier: &str,
        start_delaying_after_attempt: usize,
        initial_delay: chrono::Duration,
        multiplier: f64,
    ) -> Option<chrono::Duration> {
        static LUA_BACKOFF_SCRIPT: LazyLock<RedisScript> =
            LazyLock::new(|| RedisScript::new(include_str!("lua_scripts/backoff_protector.lua")));

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
            .await
            .flatten();

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
    fn batch(&mut self) -> RedisBatch<'_, '_, Self, ()> {
        RedisBatch::new(self)
    }

    /// Redis keys are all prefixed, use this to finalise a namespace outside of built in commands, e.g. for use in a custom script.
    #[inline]
    fn final_namespace(&self, namespace: &str) -> String {
        format!("{}:{}", self.prefix(), namespace)
    }

    /// Redis keys are all prefixed, use this to finalise a key outside of built in commands, e.g. for use in a custom script.
    #[inline]
    fn final_key(&self, namespace: &str, key: Cow<'_, str>) -> String {
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
    async fn cached_fn<'b, T, Fut, K: Into<Cow<'b, str>>>(
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

impl<'a> RedisConnLike for RedisConn<'a> {
    async fn get_inner_conn(&mut self) -> Option<&mut deadpool_redis::Connection> {
        if self.conn.is_none() {
            match self.pool.get().await {
                Ok(conn) => self.conn = Some(conn),
                Err(e) => {
                    record_exception("Failed to get redis connection.", format!("{:?}", e));
                    return None;
                }
            }
        }
        self.conn.as_mut()
    }

    fn prefix(&self) -> &str {
        self.prefix
    }

    fn into_owned(self) -> RedisConnOwned {
        RedisConnOwned {
            prefix: Arc::new(self.prefix.to_string()),
            pool: self.pool.clone(),
            conn: self.conn,
        }
    }
}

impl RedisConnLike for RedisConnOwned {
    async fn get_inner_conn(&mut self) -> Option<&mut deadpool_redis::Connection> {
        if self.conn.is_none() {
            match self.pool.get().await {
                Ok(conn) => self.conn = Some(conn),
                Err(e) => {
                    record_exception("Failed to get redis connection.", format!("{:?}", e));
                    return None;
                }
            }
        }
        self.conn.as_mut()
    }

    fn prefix(&self) -> &str {
        &self.prefix
    }

    fn into_owned(self) -> RedisConnOwned {
        self
    }
}
