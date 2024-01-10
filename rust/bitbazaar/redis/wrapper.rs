use deadpool_redis::{Config, Runtime};

use super::RedisConn;
use crate::errors::TracedResult;

/// A wrapper around redis to make it more concise to use and not need redis in the downstream Cargo.toml.
pub struct Redis {
    pool: deadpool_redis::Pool,
    prefix: String,
}

impl Redis {
    /// Create a new redis wrapper from the given Redis URL (like `redis://127.0.0.1`).
    pub fn new<A: Into<String>, B: Into<String>>(
        redis_conn_str: A,
        prefix: B,
    ) -> TracedResult<Self> {
        let cfg = Config::from_url(redis_conn_str);
        let pool = cfg.create_pool(Some(Runtime::Tokio1))?;

        Ok(Self {
            pool,
            prefix: prefix.into(),
        })
    }

    /// Get a [`RedisConn`] redis can be called with.
    pub fn conn(&self) -> RedisConn<'_> {
        RedisConn::new(&self.pool, &self.prefix)
    }
}
