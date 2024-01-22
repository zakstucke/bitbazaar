use deadpool_redis::{Config, Runtime};

use super::RedisConn;
use crate::errors::prelude::*;

/// A wrapper around redis to make it more concise to use and not need redis in the downstream Cargo.toml.
///
/// This wrapper attempts to return very few errors to help build in automatic redis failure handling into downstream code.
/// All redis errors (availability, unexpected content) will be logged as errors and results returned as `None` (or similar) where possible.
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
}