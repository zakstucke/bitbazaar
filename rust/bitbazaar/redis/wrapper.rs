use std::{sync::Arc, time::Duration};

use deadpool_redis::{Config, Runtime};

use super::{RedisConn, RedisLock, RedisLockErr, RedisTempList};
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
    ) -> Result<Self, AnyErr> {
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
    /// - `lock_id`: The resource to lock. Will be used as the key in Redis.
    /// - `ttl`: The time to live for this lock. After this time, the lock will be automatically released.
    /// - `wait_up_to`: if the lock is busy elsewhere, wait this long trying to get it, before giving up and returning [`RedisLockErr::Unavailable`].
    pub async fn dlock(
        &self,
        lock_id: &str,
        time_to_live: Duration,
        wait_up_to: Option<Duration>,
    ) -> Result<RedisLock<'_>, RedisLockErr> {
        RedisLock::new(self, lock_id, time_to_live, wait_up_to).await
    }

    /// Connect up to a magic redis list that:
    /// - Has an expiry on the list itself, resetting on each read or write. (each change lives again for `expire_after` time)
    /// - Each item in the list has it's own expiry, so the list is always clean of old items.
    /// - Each item has a generated unique key, this key can be used to update or delete specific items directly.
    /// - Returned items are returned newest/last-updated to oldest
    /// This makes this distributed data structure perfect for stuff like:
    /// - recent/temporary logs/events of any sort.
    /// - pending actions, that can be updated in-place by the creator, but read as part of a list by a viewer etc.
    pub fn templist(
        &self,
        namespace: &'static str,
        key: impl Into<String>,
        list_inactive_ttl: Duration,
        item_inactive_ttl: Duration,
    ) -> Arc<RedisTempList> {
        RedisTempList::new(namespace, key.into(), list_inactive_ttl, item_inactive_ttl)
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
