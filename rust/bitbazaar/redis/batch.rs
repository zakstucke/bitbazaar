use std::{collections::HashSet, marker::PhantomData};

use deadpool_redis::redis::{FromRedisValue, Pipeline, ToRedisArgs};
use once_cell::sync::Lazy;

use super::{RedisConn, RedisScript, RedisScriptInvoker};

static CLEAR_NAMESPACE_SCRIPT: Lazy<RedisScript> =
    Lazy::new(|| RedisScript::new(include_str!("lua_scripts/clear_namespace.lua")));

static MEXISTS_SCRIPT: Lazy<RedisScript> =
    Lazy::new(|| RedisScript::new(include_str!("lua_scripts/mexists.lua")));

static MSET_WITH_EXPIRY_SCRIPT: Lazy<RedisScript> =
    Lazy::new(|| RedisScript::new(include_str!("lua_scripts/mset_with_expiry.lua")));

/// A command builder struct. Committed with [`RedisBatch::fire`].
///
/// Batched commands are run in order, but other commands from different sources may be interleaved.
/// Note each command may be run twice, if scripts needed caching to redis.
pub struct RedisBatch<'a, 'b, 'c, ReturnType> {
    _returns: PhantomData<ReturnType>,
    redis_conn: &'a mut RedisConn<'b>,
    pipe: Pipeline,
    /// Need to keep a reference to used scripts, these will all be reloaded to redis errors because one wasn't cached on the server.
    used_scripts: HashSet<&'c RedisScript>,
}

impl<'a, 'b, 'c, ReturnType> RedisBatch<'a, 'b, 'c, ReturnType> {
    pub(crate) fn new(redis_conn: &'a mut RedisConn<'b>) -> Self {
        Self {
            _returns: PhantomData,
            redis_conn,
            pipe: deadpool_redis::redis::pipe(),
            used_scripts: HashSet::new(),
        }
    }

    async fn inner_fire<R: FromRedisValue>(&mut self) -> Option<R> {
        if let Some(conn) = self.redis_conn.get_inner_conn().await {
            match self.pipe.query_async(conn).await {
                Ok(result) => Some(result),
                Err(err) => {
                    // Load the scripts into Redis if the any of the scripts weren't there before.
                    if err.kind() == redis::ErrorKind::NoScriptError {
                        if self.used_scripts.is_empty() {
                            tracing::error!("Redis batch failed. Pipe returned NoScriptError, but not scripts were used. Err: '{}'", err);
                            return None;
                        }

                        tracing::info!(
                            "Redis batch failed. Pipe returned NoScriptError, reloading {} script{} to redis. Err: '{}'",
                            self.used_scripts.len(),
                            if self.used_scripts.len() == 1 { "" } else { "s" },
                            err
                        );

                        let mut load_pipe = deadpool_redis::redis::pipe();
                        for script in &self.used_scripts {
                            load_pipe.add_command(script.load_cmd());
                        }
                        match load_pipe
                            .query_async::<deadpool_redis::Connection, redis::Value>(conn)
                            .await
                        {
                            // Now loaded the scripts, rerun the batch:
                            Ok(_) => match self.pipe.query_async(conn).await {
                                Ok(result) => Some(result),
                                Err(err) => {
                                    tracing::error!("Redis batch failed. Second attempt as first required reloading of scripts (not necessarily related). Err: '{}'", err);
                                    None
                                }
                            },
                            Err(err) => {
                                tracing::error!(
                                    "Redis script reload during batch failed. Err: '{}'",
                                    err
                                );
                                None
                            }
                        }
                    } else {
                        tracing::error!("Redis batch failed. Err: '{}'", err);
                        None
                    }
                }
            }
        } else {
            None
        }
    }

    /// Run an arbitrary redis (lua script). But discards any return value.
    pub fn script_no_return(mut self, script_invokation: RedisScriptInvoker<'c>) -> Self {
        // Adding ignore() to ignore response.
        self.pipe.add_command(script_invokation.eval_cmd()).ignore();
        self.used_scripts.insert(script_invokation.script);
        RedisBatch {
            _returns: PhantomData,
            redis_conn: self.redis_conn,
            pipe: self.pipe,
            used_scripts: self.used_scripts,
        }
    }

    /// Expire an existing key with a new/updated ttl.
    ///
    /// <https://redis.io/commands/pexpire/>
    pub fn expire(mut self, namespace: &str, key: &str, ttl: chrono::Duration) -> Self {
        self.pipe
            .pexpire(
                self.redis_conn.final_key(namespace, key.into()),
                ttl.num_milliseconds(),
            )
            // Ignoring so it doesn't take up a space in the tuple response.
            .ignore();

        RedisBatch {
            _returns: PhantomData,
            redis_conn: self.redis_conn,
            pipe: self.pipe,
            used_scripts: self.used_scripts,
        }
    }

    /// Add an entry to an ordered set (auto creating the set if it doesn't exist).
    /// <https://redis.io/commands/zadd/>
    ///
    /// Arguments:
    /// - `set_namespace`: The namespace of the set.
    /// - `set_key`: The key of the set.
    /// - `set_ttl`: The time to live of the set. This will reset on each addition, meaning after the last update the set will expire after this time.
    /// - `score`: The score of the entry.
    /// - `value`: The value of the entry. (values of sets must be strings)
    pub fn zadd(
        mut self,
        set_namespace: &str,
        set_key: &str,
        set_ttl: Option<chrono::Duration>,
        score: i64,
        value: impl ToRedisArgs,
    ) -> Self {
        self.pipe
            .zadd(
                self.redis_conn.final_key(set_namespace, set_key.into()),
                value,
                score,
            )
            // Ignoring so it doesn't take up a space in the tuple response.
            .ignore();
        if let Some(set_ttl) = set_ttl {
            self.expire(set_namespace, set_key, set_ttl)
        } else {
            RedisBatch {
                _returns: PhantomData,
                redis_conn: self.redis_conn,
                pipe: self.pipe,
                used_scripts: self.used_scripts,
            }
        }
    }

    /// remove an entries from an ordered set.
    /// <https://redis.io/commands/zrem/>
    ///
    /// Arguments:
    /// - `set_namespace`: The namespace of the set.
    /// - `set_key`: The key of the set.
    /// - `values`: The values to remove as an iterator.
    pub fn zrem<T: ToRedisArgs>(
        mut self,
        set_namespace: &str,
        set_key: &str,
        values: impl IntoIterator<Item = T>,
    ) -> Self {
        let members = values.into_iter().collect::<Vec<_>>();
        // No-op if no members so skip (redis would actually error if empty anyway)
        if members.is_empty() {
            return self;
        }
        self.pipe
            .zrem(
                self.redis_conn.final_key(set_namespace, set_key.into()),
                members,
            )
            // Ignoring so it doesn't take up a space in the tuple response.
            .ignore();
        RedisBatch {
            _returns: PhantomData,
            redis_conn: self.redis_conn,
            pipe: self.pipe,
            used_scripts: self.used_scripts,
        }
    }

    /// Add multiple entries at once to an ordered set (auto creating the set if it doesn't exist).
    /// <https://redis.io/commands/zadd/>
    ///
    /// Arguments:
    /// - `set_namespace`: The namespace of the set.
    /// - `set_key`: The key of the set.
    /// - `set_ttl`: The time to live of the set. This will reset on each addition, meaning after the last update the set will expire after this time.
    /// - `items`: The scores and values of the entries. (set values must be strings)
    pub fn zadd_multi(
        mut self,
        set_namespace: &str,
        set_key: &str,
        set_ttl: Option<chrono::Duration>,
        items: &[(i64, impl ToRedisArgs)],
    ) -> Self {
        self.pipe
            .zadd_multiple(
                self.redis_conn.final_key(set_namespace, set_key.into()),
                items,
            )
            // Ignoring so it doesn't take up a space in the tuple response.
            .ignore();
        if let Some(set_ttl) = set_ttl {
            self.expire(set_namespace, set_key, set_ttl)
        } else {
            RedisBatch {
                _returns: PhantomData,
                redis_conn: self.redis_conn,
                pipe: self.pipe,
                used_scripts: self.used_scripts,
            }
        }
    }

    /// Remove entries from an ordered set by score range. (range is inclusive)
    ///
    /// <https://redis.io/commands/zremrangebyscore/>
    pub fn zremrangebyscore(
        mut self,
        set_namespace: &str,
        set_key: &str,
        min: i64,
        max: i64,
    ) -> Self {
        self.pipe
            .zrembyscore(
                self.redis_conn.final_key(set_namespace, set_key.into()),
                min,
                max,
            )
            // Ignoring so it doesn't take up a space in the tuple response.
            .ignore();
        RedisBatch {
            _returns: PhantomData,
            redis_conn: self.redis_conn,
            pipe: self.pipe,
            used_scripts: self.used_scripts,
        }
    }

    /// Set a key to a value with an optional expiry.
    ///
    /// (expiry accurate to the millisecond)
    pub fn set<T: ToRedisArgs>(
        mut self,
        namespace: &str,
        key: &str,
        value: T,
        expiry: Option<chrono::Duration>,
    ) -> Self {
        let final_key = self.redis_conn.final_key(namespace, key.into());

        if let Some(expiry) = expiry {
            // If expiry is 0 or negative don't send to prevent redis error:
            if expiry > chrono::Duration::zero() {
                // Ignoring so it doesn't take up a space in the tuple response.
                self.pipe
                    .pset_ex(final_key, value, expiry.num_milliseconds() as u64)
                    .ignore();
            }
        } else {
            // Ignoring so it doesn't take up a space in the tuple response.
            self.pipe.set(final_key, value).ignore();
        }

        RedisBatch {
            _returns: PhantomData,
            redis_conn: self.redis_conn,
            pipe: self.pipe,
            used_scripts: self.used_scripts,
        }
    }

    /// Set multiple values (MSET) of the same type at once. If expiry used will use a custom lua script to achieve the functionality.
    ///
    /// (expiry accurate to the millisecond)
    pub fn mset<Value: ToRedisArgs>(
        mut self,
        namespace: &str,
        pairs: impl IntoIterator<Item = (impl AsRef<str>, Value)>,
        expiry: Option<chrono::Duration>,
    ) -> Self {
        let final_pairs = pairs
            .into_iter()
            .map(|(key, value)| {
                (
                    self.redis_conn.final_key(namespace, key.as_ref().into()),
                    value,
                )
            })
            .collect::<Vec<_>>();

        if let Some(expiry) = expiry {
            // If expiry is weirdly 0 don't send to prevent redis error:
            if (expiry) > chrono::Duration::milliseconds(0) {
                let mut invoker = MSET_WITH_EXPIRY_SCRIPT
                    .invoker()
                    .arg(expiry.num_milliseconds() as u64);
                for (key, value) in final_pairs {
                    invoker = invoker.key(key).arg(value);
                }
                self.script_no_return(invoker)
            } else {
                RedisBatch {
                    _returns: PhantomData,
                    redis_conn: self.redis_conn,
                    pipe: self.pipe,
                    used_scripts: self.used_scripts,
                }
            }
        } else {
            // Ignoring so it doesn't take up a space in the tuple response.
            self.pipe.mset(&final_pairs).ignore();
            RedisBatch {
                _returns: PhantomData,
                redis_conn: self.redis_conn,
                pipe: self.pipe,
                used_scripts: self.used_scripts,
            }
        }
    }

    /// Clear one or more keys.
    pub fn clear<'key>(
        mut self,
        namespace: &str,
        keys: impl IntoIterator<Item = &'key str>,
    ) -> Self {
        let final_keys = keys
            .into_iter()
            .map(Into::into)
            .map(|key| self.redis_conn.final_key(namespace, key))
            .collect::<Vec<_>>();
        // Ignoring so it doesn't take up a space in the tuple response.
        self.pipe.del(final_keys).ignore();
        RedisBatch {
            _returns: PhantomData,
            redis_conn: self.redis_conn,
            pipe: self.pipe,
            used_scripts: self.used_scripts,
        }
    }

    /// Clear all keys under a given namespace
    pub fn clear_namespace(self, namespace: &str) -> Self {
        let final_namespace = self.redis_conn.final_namespace(namespace);
        self.script_no_return(CLEAR_NAMESPACE_SCRIPT.invoker().arg(final_namespace))
    }
}

/// Trait implementing the fire() method on a batch, variable over the items in the batch.
pub trait RedisBatchFire {
    /// The final return type of the batch.
    type ReturnType;

    /// Commit the batch and return the result.
    /// If redis unavailable, or the types didn't match causing decoding to fail, `None` will be returned and the error logged.
    fn fire(self) -> impl std::future::Future<Output = Option<Self::ReturnType>>;
}

// The special singular variant that returns the command output directly.
impl<'a, 'b, 'c, R: FromRedisValue> RedisBatchFire for RedisBatch<'a, 'b, 'c, (R,)> {
    type ReturnType = R;

    async fn fire(mut self) -> Option<R> {
        self.inner_fire().await.map(|(r,)| r)
    }
}

macro_rules! impl_batch_fire {
    ( $($tup_item:ident)* ) => (
        impl<'a, 'b, 'c, $($tup_item: FromRedisValue),*> RedisBatchFire for RedisBatch<'a, 'b, 'c, ($($tup_item,)*)> {
            type ReturnType = ($($tup_item,)*);

            async fn fire(mut self) -> Option<($($tup_item,)*)> {
                self.inner_fire().await
            }
        }
    );
}

/// Implements all the supported redis operations that need to modify the return type and hence need macros.
pub trait RedisBatchReturningOps<'c> {
    /// The producer for the next batch struct sig.
    type NextType<T>;

    /// Run an arbitrary redis (lua script).
    fn script<ScriptOutput: FromRedisValue>(
        self,
        script_invokation: RedisScriptInvoker<'c>,
    ) -> Self::NextType<ScriptOutput>;

    /// Check if a key exists.
    fn exists(self, namespace: &str, key: &str) -> Self::NextType<bool>;

    /// Check if multiple keys exists.
    fn mexists<'key>(
        self,
        namespace: &str,
        keys: impl IntoIterator<Item = &'key str>,
    ) -> Self::NextType<Vec<bool>>;

    /// Get a value from a key. Returning `None` if the key doesn't exist.
    fn get<Value: FromRedisValue>(
        self,
        namespace: &str,
        key: &str,
    ) -> Self::NextType<Option<Value>>;

    /// Get multiple values (MGET) of the same type at once. Returning `None` for each key that didn't exist.
    fn mget<Value>(
        self,
        namespace: &str,
        keys: impl IntoIterator<Item = impl AsRef<str>>,
    ) -> Self::NextType<Vec<Option<Value>>>;

    /// HIGHEST TO LOWEST SCORES.
    /// Retrieve entries from an ordered set by score range. (range is inclusive)
    /// Items that cannot be decoded into the specified type are returned as `None`.
    ///
    /// VARIATIONS FROM DEFAULT:
    /// - Rev used: returned high to low.
    ///
    /// Arguments:
    /// - `set_namespace`: The namespace of the set.
    /// - `set_key`: The key of the set.
    /// - `min`: The minimum score.
    /// - `max`: The maximum score.
    /// - `limit`: The maximum number of items to return.
    ///
    /// `https://redis.io/commands/zrangebyscore/`
    fn zrangebyscore_high_to_low<Value: FromRedisValue>(
        self,
        set_namespace: &str,
        set_key: &str,
        min: i64,
        max: i64,
        limit: Option<isize>,
    ) -> Self::NextType<Vec<(Option<Value>, i64)>>;

    /// LOWEST TO HIGHEST SCORES.
    /// Retrieve entries from an ordered set by score range. (range is inclusive)
    /// Items that cannot be decoded into the specified type are returned as `None`.
    ///
    /// Arguments:
    /// - `set_namespace`: The namespace of the set.
    /// - `set_key`: The key of the set.
    /// - `min`: The minimum score.
    /// - `max`: The maximum score.
    /// - `limit`: The maximum number of items to return.
    ///
    /// `https://redis.io/commands/zrangebyscore/`
    fn zrangebyscore_low_to_high<Value: FromRedisValue>(
        self,
        set_namespace: &str,
        set_key: &str,
        min: i64,
        max: i64,
        limit: Option<isize>,
    ) -> Self::NextType<Vec<(Option<Value>, i64)>>;
}

macro_rules! impl_batch_ops {
    ( $($tup_item:ident)* ) => (
        impl<'a, 'b, 'c, $($tup_item: FromRedisValue),*> RedisBatchReturningOps<'c> for RedisBatch<'a, 'b, 'c, ($($tup_item,)*)> {
            type NextType<T> = RedisBatch<'a, 'b, 'c, ($($tup_item,)* T,)>;

            fn script<ScriptOutput: FromRedisValue>(
                mut self,
                script_invokation: RedisScriptInvoker<'c>,
            ) -> Self::NextType<ScriptOutput> {
                self.pipe.add_command(script_invokation.eval_cmd());
                self.used_scripts.insert(script_invokation.script);
                RedisBatch {
                    _returns: PhantomData,
                    redis_conn: self.redis_conn,
                    pipe: self.pipe,
                    used_scripts: self.used_scripts
                }
            }



            fn exists(mut self, namespace: &str, key: &str) -> Self::NextType<bool> {
                self.pipe.exists(self.redis_conn.final_key(namespace, key.into()));
                RedisBatch {
                    _returns: PhantomData,
                    redis_conn: self.redis_conn,
                    pipe: self.pipe,
                    used_scripts: self.used_scripts
                }
            }

            fn mexists<'key>(
                self,
                namespace: &str,
                keys: impl IntoIterator<Item = &'key str>,
            ) -> Self::NextType<Vec<bool>> {
                let final_keys = keys.into_iter().map(Into::into).map(|key| self.redis_conn.final_key(namespace, key)).collect::<Vec<_>>();
                let mut invoker = MEXISTS_SCRIPT.invoker();
                for key in &final_keys {
                    invoker = invoker.key(key);
                }
                self.script::<Vec<bool>>(invoker)
            }

            fn get<Value: FromRedisValue>(
                mut self,
                namespace: &str,
                key: &str,
            ) -> Self::NextType<Option<Value>> {
                self.pipe.get(self.redis_conn.final_key(namespace, key.into()));
                RedisBatch {
                    _returns: PhantomData,
                    redis_conn: self.redis_conn,
                    pipe: self.pipe,
                    used_scripts: self.used_scripts
                }
            }

            fn mget<Value>(
                mut self,
                namespace: &str,
                keys: impl IntoIterator<Item = impl AsRef<str>>,
            ) -> Self::NextType<Vec<Option<Value>>> {
                let final_keys = keys.into_iter().map(|key| self.redis_conn.final_key(namespace, key.as_ref().into())).collect::<Vec<_>>();

                self.pipe.get(final_keys);
                RedisBatch {
                    _returns: PhantomData,
                    redis_conn: self.redis_conn,
                    pipe: self.pipe,
                    used_scripts: self.used_scripts
                }
            }

            fn zrangebyscore_high_to_low<Value: FromRedisValue>(
                mut self,
                set_namespace: &str,
                set_key: &str,
                min: i64,
                max: i64,
                limit: Option<isize>,
            ) -> Self::NextType<Vec<(Option<Value>, i64)>> {
                self.pipe.zrevrangebyscore_limit_withscores(
                    self.redis_conn.final_key(set_namespace, set_key.into()),
                    max,
                    min,
                    0,
                    limit.unwrap_or(isize::MAX)
                );
                RedisBatch {
                    _returns: PhantomData,
                    redis_conn: self.redis_conn,
                    pipe: self.pipe,
                    used_scripts: self.used_scripts,
                }
            }

            fn zrangebyscore_low_to_high<Value: FromRedisValue>(
                mut self,
                set_namespace: &str,
                set_key: &str,
                min: i64,
                max: i64,
                limit: Option<isize>,
            ) -> Self::NextType<Vec<(Option<Value>, i64)>> {
                self.pipe.zrangebyscore_limit_withscores(
                    self.redis_conn.final_key(set_namespace, set_key.into()),
                    min,
                    max,
                    0,
                    limit.unwrap_or(isize::MAX)
                );
                RedisBatch {
                    _returns: PhantomData,
                    redis_conn: self.redis_conn,
                    pipe: self.pipe,
                    used_scripts: self.used_scripts,
                }
            }
        }
    );
}

// fire() trait for up to 12 operations:
impl_batch_fire! {}
// impl_batch_fire! { A } // Special case that returns the command output directly (not in tuple)
impl_batch_fire! { A B }
impl_batch_fire! { A B C }
impl_batch_fire! { A B C D }
impl_batch_fire! { A B C D E }
impl_batch_fire! { A B C D E F }
impl_batch_fire! { A B C D E F G }
impl_batch_fire! { A B C D E F G H }
impl_batch_fire! { A B C D E F G H I }
impl_batch_fire! { A B C D E F G H I J }
impl_batch_fire! { A B C D E F G H I J K }
impl_batch_fire! { A B C D E F G H I J K L }

// redis ops trait for up to 12 operations:
impl_batch_ops! {}
impl_batch_ops! { A }
impl_batch_ops! { A B }
impl_batch_ops! { A B C }
impl_batch_ops! { A B C D }
impl_batch_ops! { A B C D E }
impl_batch_ops! { A B C D E F }
impl_batch_ops! { A B C D E F G }
impl_batch_ops! { A B C D E F G H }
impl_batch_ops! { A B C D E F G H I }
impl_batch_ops! { A B C D E F G H I J }
impl_batch_ops! { A B C D E F G H I J K }
impl_batch_ops! { A B C D E F G H I J K L }
