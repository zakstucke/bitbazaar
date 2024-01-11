use std::{borrow::Cow, future::Future};

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
    pub fn batch<'ref_lt>(&'ref_lt mut self) -> RedisBatch<'ref_lt, 'a, '_> {
        RedisBatch::new(self)
    }

    /// Redis keys are all prefixed, use this to finalise a namespace outside of built in commands, e.g. for use in a custom script.
    #[inline]
    pub fn final_namespace(&self, namespace: &'static str) -> String {
        format!("{}:{}", self.prefix, namespace)
    }

    /// Redis keys are all prefixed, use this to finalise a key outside of built in commands, e.g. for use in a custom script.
    #[inline]
    pub fn final_key(&self, namespace: &'static str, key: Cow<'_, str>) -> String {
        format!("{}:{}", self.final_namespace(namespace), key)
    }

    // /// Clear all keys under a namespace. Returning the number of keys deleted.
    // pub async fn clear_namespace<'c>(&mut self, namespace: &'static str) -> Option<usize> {
    //     let final_namespace = self.final_namespace(namespace);
    //     CLEAR_NAMESPACE_SCRIPT
    //         .run(self, |scr| {
    //             scr.arg(final_namespace);
    //         })
    //         .await
    // }

    /// Cache an async function in redis.
    /// If already stored, the cached value will be returned, otherwise the function will be stored in redis for next time.
    ///
    /// If redis is unavailable, or the existing contents at the key is wrong, the function output will be used.
    /// The only error coming out of here should be something wrong with the external callback.
    #[inline]
    pub async fn cached_fn<'b, T, Fut, K: Into<Cow<'b, str>>>(
        &mut self,
        namespace: &'static str,
        key: K,
        cb: impl FnOnce() -> Fut,
    ) -> TracedResult<T>
    where
        T: FromRedisValue + ToRedisArgs,
        Fut: Future<Output = TracedResult<T>>,
    {
        let key: Cow<'b, str> = key.into();

        let cached = self
            .batch()
            .get::<T, _>(namespace, key.clone())
            .fire()
            .await
            .flatten();
        if let Some(cached) = cached {
            Ok(cached)
        } else {
            let val = cb().await?;
            self.batch().set(namespace, key, &val).fire().await;
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
    /// If redis is acting up and unavailable, this will return None.
    pub(crate) async fn get_conn(&mut self) -> Option<&mut deadpool_redis::Connection> {
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
}
