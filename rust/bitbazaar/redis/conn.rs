use std::{borrow::Cow, future::Future};

use deadpool_redis::redis::{FromRedisValue, ToRedisArgs};

use super::batch::{RedisBatch, RedisBatchFire, RedisBatchReturningOps};
use crate::errors::prelude::*;

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
        expiry: Option<std::time::Duration>,
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
