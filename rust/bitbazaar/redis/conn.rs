use std::borrow::Cow;

use deadpool_redis::redis::{FromRedisValue, ToRedisArgs};

use super::batch::RedisBatch;
use crate::errors::TracedResult;

/// Wrapper around a lazy redis connection.
pub struct RedisConn<'a> {
    pub(crate) prefix: &'a str,
    pool: &'a deadpool_redis::Pool,
    conn: Option<deadpool_redis::Connection>,
}

/// Public methods for RedisConn.
impl<'a> RedisConn<'a> {
    /// Get a new [`RedisBatch`] for this connection that commands can be piped together with.
    pub fn batch<'ref_lt>(&'ref_lt mut self) -> RedisBatch<'ref_lt, 'a> {
        RedisBatch::new(self)
    }

    /// Cache a function in redis.
    /// If already stored, the cached value will be returned, otherwise the function will be stored in redis for next time.
    pub async fn cache_fn<T, N: Into<String>, K: Into<String>>(
        &mut self,
        namespace: N,
        key: K,
        cb: impl FnOnce() -> TracedResult<T>,
    ) -> TracedResult<T>
    where
        T: FromRedisValue + ToRedisArgs + Clone,
    {
        let combined_key = format!("{}:{}", namespace.into(), key.into());
        let cached = self.batch().get::<T, _>(&combined_key).fire().await?;
        if let Some(cached) = cached {
            Ok(cached)
        } else {
            let val = cb()?;
            self.batch().set(&combined_key, val.clone()).fire().await?;
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

    /// Get an internal connection from the pool, reused after first call.
    pub(crate) async fn get_conn(&mut self) -> TracedResult<&mut deadpool_redis::Connection> {
        if self.conn.is_none() {
            let conn = self.pool.get().await?;
            self.conn = Some(conn);
        }
        Ok(self.conn.as_mut().unwrap())
    }

    /// To prevent clashes with other services, the wrapper has its own prefix, apply that to every key used:
    pub(crate) fn final_key(&self, key: Cow<'_, str>) -> String {
        format!("{}:{}", self.prefix, key)
    }
}
