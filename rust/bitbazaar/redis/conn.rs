#![allow(async_fn_in_trait)]

use std::{
    borrow::Cow,
    future::Future,
    sync::{Arc, LazyLock},
};

use deadpool_redis::redis::{FromRedisValue, ToRedisArgs};

use super::{
    batch::{RedisBatch, RedisBatchFire, RedisBatchReturningOps},
    fuzzy::RedisFuzzy,
    pubsub::{pubsub_global::RedisPubSubGlobal, RedisChannelListener},
};
use crate::{errors::prelude::*, log::record_exception, redis::RedisScript};

/// A lazy redis connection.
#[derive(Debug, Clone)]
pub struct RedisConn<'a> {
    pub(crate) prefix: &'a str,
    pool: &'a deadpool_redis::Pool,
    // It uses it's own global connection so needed down here too, to abstract away and use from the same higher-order connection:
    pubsub_global: &'a Arc<RedisPubSubGlobal>,
    // We used to cache the [`deadpool_redis::Connection`] conn in here,
    // but after benching it literally costs about 20us to get a connection from deadpool because it rotates them internally.
    // so getting for each usage is fine, given:
    // - most conns will probably be used once anyway, e.g. get a conn in a handler and do some caching or whatever in a batch().
    // - prevents needing mutable references to the conn anymore, much nicer ergonomics.
    // - prevents the chance of a stale cached connection, had issues before with this, deadpool handles internally.
    // conn: Option<deadpool_redis::Connection>,
}

impl<'a> RedisConn<'a> {
    pub(crate) fn new(
        pool: &'a deadpool_redis::Pool,
        prefix: &'a str,
        pubsub_global: &'a Arc<RedisPubSubGlobal>,
    ) -> Self {
        Self {
            pool,
            prefix,
            pubsub_global,
        }
    }
}

/// An owned variant of [`RedisConn`].
/// Just requires a couple of Arc clones, so still quite lightweight.
#[derive(Debug, Clone)]
pub struct RedisConnOwned {
    // Prefix and pool both Arced now at top level for easy cloning.
    pub(crate) prefix: Arc<String>,
    pool: deadpool_redis::Pool,
    // It uses it's own global connection so needed down here too, to abstract away and use from the same higher-order connection:
    pubsub_global: Arc<RedisPubSubGlobal>,
    // We used to cache the [`deadpool_redis::Connection`] conn in here,
    // but after benching it literally costs about 20us to get a connection from deadpool because it rotates them internally.
    // so getting for each usage is fine, given:
    // - most conns will probably be used once anyway, e.g. get a conn in a handler and do some caching or whatever in a batch().
    // - prevents needing mutable references to the conn anymore, much nicer ergonomics.
    // - prevents the chance of a stale cached connection, had issues before with this, deadpool handles internally.
    // conn: Option<deadpool_redis::Connection>,
}

/// Generic methods over the RedisConn and RedisConnOwned types.
pub trait RedisConnLike: std::fmt::Debug + Send + Sized {
    /// Get an internal connection from the pool.
    /// Despite returning an owned object, the underlying real redis connection will be reused after this user drops it.
    /// If redis is acting up and unavailable, this will return None.
    /// NOTE: this mainly is used internally, but provides a fallback to the underlying connection, if the exposed interface does not provide options that fit an external user need (which could definitely happen).
    async fn get_inner_conn(&self) -> Option<deadpool_redis::Connection>;

    /// Get the redis configured prefix.
    fn prefix(&self) -> &str;

    /// Get the redis pubsub global manager.
    fn _pubsub_global(&self) -> &Arc<RedisPubSubGlobal>;

    /// Convert to the owned variant.
    fn to_owned(&self) -> RedisConnOwned;

    /// Ping redis, returning true if it's up and responsive.
    async fn ping(&self) -> bool {
        self.batch()
            .custom::<RedisFuzzy<String>>("PING")
            .fire()
            .await
            .flatten()
            .is_some()
    }

    /// Subscribe to a channel via pubsub, receiving messages through the returned receiver.
    /// The subscription will be dropped when the receiver is dropped.
    ///
    /// Sending can be done via normal batches using [`RedisBatch::publish`].
    ///
    /// Returns None when redis unavailable for some reason, after a few seconds of trying to connect.
    async fn subscribe<T: ToRedisArgs + FromRedisValue>(
        &self,
        namespace: &str,
        channel: &str,
    ) -> Option<RedisChannelListener<T>> {
        self._pubsub_global()
            .subscribe(self.final_key(namespace, channel.into()))
            .await
    }

    // Commented out as untested, not sure if works.
    // /// Get all data from redis, only really useful during testing.
    // ///
    // async fn dev_all_data(&self) -> HashMap<String, redis::Value> {
    //     if let Some(mut conn) = self.get_inner_conn().await {
    //         let mut cmd = redis::cmd("SCAN");
    //         cmd.arg(0);
    //         let mut data = HashMap::new();
    //         loop {
    //             let (next_cursor, keys): (i64, Vec<String>) = cmd.query_async(&mut conn).await.unwrap();
    //             for key in keys {
    //                 let val: redis::Value =
    //                     redis::cmd("GET").arg(&key).query_async(&mut conn).await.unwrap();
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
    /// Returns the resulting string from the command, or None if failed for some reason.
    async fn dev_flushall(&self, sync: bool) -> Option<String> {
        let mut batch = self.batch().custom::<RedisFuzzy<String>>("FLUSHALL");
        if sync {
            batch = batch.custom_arg("SYNC");
        } else {
            batch = batch.custom_arg("ASYNC");
        }
        batch.fire().await.flatten()
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
        &self,
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
    fn batch(&self) -> RedisBatch<'_, '_, Self, ()> {
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
        &self,
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
    async fn get_inner_conn(&self) -> Option<deadpool_redis::Connection> {
        match self.pool.get().await {
            Ok(conn) => Some(conn),
            Err(e) => {
                record_exception("Failed to get redis connection.", format!("{:?}", e));
                None
            }
        }
    }

    fn prefix(&self) -> &str {
        self.prefix
    }

    fn _pubsub_global(&self) -> &Arc<RedisPubSubGlobal> {
        self.pubsub_global
    }

    fn to_owned(&self) -> RedisConnOwned {
        RedisConnOwned {
            prefix: Arc::new(self.prefix.to_string()),
            pool: self.pool.clone(),
            pubsub_global: self.pubsub_global.clone(),
        }
    }
}

impl RedisConnLike for RedisConnOwned {
    async fn get_inner_conn(&self) -> Option<deadpool_redis::Connection> {
        match self.pool.get().await {
            Ok(conn) => Some(conn),
            Err(e) => {
                record_exception("Failed to get redis connection.", format!("{:?}", e));
                None
            }
        }
    }

    fn prefix(&self) -> &str {
        &self.prefix
    }

    fn _pubsub_global(&self) -> &Arc<RedisPubSubGlobal> {
        &self.pubsub_global
    }

    fn to_owned(&self) -> RedisConnOwned {
        self.clone()
    }
}
