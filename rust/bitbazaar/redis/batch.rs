#![allow(private_interfaces)]

use std::{borrow::Cow, collections::HashSet, marker::PhantomData, sync::LazyLock};

use deadpool_redis::redis::{FromRedisValue, Pipeline, ToRedisArgs};

use crate::{log::record_exception, retry_flexi};

use super::{
    conn::RedisConnLike,
    fuzzy::{FuzzyFromRedisValue, Nilcase, RedisFuzzy, RedisFuzzyUnwrap},
    redis_retry::redis_retry_config,
    RedisJson, RedisScript, RedisScriptInvoker,
};

static CLEAR_NAMESPACE_SCRIPT: LazyLock<RedisScript> =
    LazyLock::new(|| RedisScript::new(include_str!("lua_scripts/clear_namespace.lua")));

static MEXISTS_SCRIPT: LazyLock<RedisScript> =
    LazyLock::new(|| RedisScript::new(include_str!("lua_scripts/mexists.lua")));

static MSET_WITH_EXPIRY_SCRIPT: LazyLock<RedisScript> =
    LazyLock::new(|| RedisScript::new(include_str!("lua_scripts/mset_with_expiry.lua")));

/// A command builder struct. Committed with [`RedisBatch::fire`].
///
/// Batched commands are run in order, but other commands from different sources may be interleaved.
/// Note each command may be run twice, if scripts needed caching to redis.
pub struct RedisBatch<'a, 'c, ConnType: RedisConnLike, ReturnType> {
    _returns: PhantomData<ReturnType>,
    redis_conn: &'a ConnType,
    pipe: Pipeline,
    /// Need to keep a reference to used scripts, these will all be reloaded to redis errors because one wasn't cached on the server.
    used_scripts: HashSet<&'c RedisScript>,
}

impl<'a, 'c, ConnType: RedisConnLike, ReturnType> RedisBatch<'a, 'c, ConnType, ReturnType> {
    pub(crate) fn new(redis_conn: &'a ConnType) -> Self {
        Self {
            _returns: PhantomData,
            redis_conn,
            pipe: deadpool_redis::redis::pipe(),
            used_scripts: HashSet::new(),
        }
    }

    async fn inner_fire<R: FuzzyFromRedisValue>(&self) -> Option<R> {
        if let Some(mut conn) = self.redis_conn.get_inner_conn().await {
            // Handling retryable errors internally:
            let result = match retry_flexi!(redis_retry_config(), {
                // Testing:
                // tracing::info!("{:?}", std::any::type_name::<R>());
                // let s = self.pipe.query_async::<redis::Value>(&mut conn).await;
                // tracing::info!("{:?}", s);
                match self.pipe.query_async::<redis::Value>(&mut conn).await {
                    Ok(v) => R::fuzzy_from_redis_value(&v),
                    Err(e) => Err(e),
                }
            }) {
                Ok(v) => Some(v),
                Err(e) => {
                    // Load the scripts into Redis if the any of the scripts weren't there before.
                    if matches!(e.kind(), redis::ErrorKind::NoScriptError) {
                        if self.used_scripts.is_empty() {
                            record_exception("Redis batch failed. Pipe returned NoScriptError, but no scripts were used.", format!("{:?}", e));
                            return None;
                        }

                        tracing::debug!(
                            "Redis batch will auto re-run. Pipe returned NoScriptError, reloading {} script{} to redis. Probably occurred due to a redis restart during this program's execution. Err: '{:?}'",
                            self.used_scripts.len(),
                            if self.used_scripts.len() == 1 { "" } else { "s" },
                            e
                        );

                        let mut load_pipe = deadpool_redis::redis::pipe();
                        for script in &self.used_scripts {
                            load_pipe.add_command(script.load_cmd());
                        }
                        match retry_flexi!(redis_retry_config(), {
                            load_pipe.query_async::<redis::Value>(&mut conn).await
                        }) {
                            // Now loaded the scripts, rerun the batch:
                            Ok(_) => {
                                match match self.pipe.query_async::<redis::Value>(&mut conn).await {
                                    Ok(v) => R::fuzzy_from_redis_value(&v),
                                    Err(e) => Err(e),
                                } {
                                    Ok(v) => Some(v),
                                    Err(err) => {
                                        record_exception("Redis batch failed. Pipe returned NoScriptError, but we've just loaded all scripts.", format!("{:?}", err));
                                        None
                                    }
                                }
                            }
                            Err(err) => {
                                record_exception(
                                    "Redis script reload during batch failed.",
                                    format!("{:?}", err),
                                );
                                None
                            }
                        }
                    } else {
                        record_exception("Redis batch failed.", format!("{:?}", e));
                        None
                    }
                }
            };

            // When the pipeline is successful, update internally the scripts we know to be loaded.
            if result.is_some() {
                let sl = self.redis_conn.scripts_loaded();
                for script in self.used_scripts.iter() {
                    sl.insert(script.hash.clone());
                }
            }

            result
        } else {
            None
        }
    }

    /// Run an arbitrary redis (lua script). But discards any return value.
    pub fn script_no_return(mut self, script_invokation: RedisScriptInvoker<'c>) -> Self {
        // Adding ignore() to ignore response.
        self.pipe
            .add_command(
                script_invokation.eval_cmd(
                    self.redis_conn
                        .scripts_loaded()
                        .contains(&script_invokation.script.hash),
                ),
            )
            .ignore();
        self.used_scripts.insert(script_invokation.script);
        RedisBatch {
            _returns: PhantomData,
            redis_conn: self.redis_conn,
            pipe: self.pipe,
            used_scripts: self.used_scripts,
        }
    }

    /// Low-level backdoor. Pass in a custom redis command to run, but don't expect a return value.
    /// After calling this, custom_arg() can be used to add arguments.
    ///
    /// E.g. `batch.custom_no_return("SET").custom_arg("key").custom_arg("value").fire().await;`
    pub fn custom_no_return(mut self, cmd: &str) -> Self {
        self.pipe.cmd(cmd).ignore();
        self
    }

    /// Low-level backdoor. Add a custom argument to the last custom command added with either `custom_no_return()` or `custom()`.
    pub fn custom_arg(mut self, arg: impl ToRedisArgs) -> Self {
        self.pipe.arg(arg);
        self
    }

    /// Publish a message to a pubsub channel.
    /// Json wrapped internally, makes easier to work with.
    pub fn publish(
        mut self,
        namespace: &str,
        channel: &str,
        message: impl serde::Serialize + for<'b> serde::Deserialize<'b>,
    ) -> Self {
        self.pipe
            .publish(
                self.redis_conn.final_key(namespace, channel.into()),
                RedisJson(message),
            )
            .ignore();
        self
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

    /// Add entries to a hashmap (auto creating the hashmap if it doesn't exist).
    /// <https://redis.io/docs/latest/commands/hset/>
    ///
    /// Arguments:
    /// - `hashmap_namespace`: The namespace of the hashmap.
    /// - `hashmap_key`: The key of the hashmap.
    /// - `hashmap_ttl`: The time to live of the hashmap. This will reset on each addition, meaning after the last update the hashmap will expire after this time.
    /// - `entries`: The key/value pairs to add.
    pub fn hset<S: AsRef<str>, Value: ToRedisArgs>(
        mut self,
        hashmap_namespace: &str,
        hashmap_key: &str,
        hashmap_ttl: Option<chrono::Duration>,
        entries: impl IntoIterator<Item = (S, Value)>,
    ) -> Self {
        let entries = entries
            .into_iter()
            .map(|(k, v)| (k.as_ref().to_string(), v))
            .collect::<Vec<_>>();

        // No-op if no entries so skip (redis would actually error if empty anyway)
        if entries.is_empty() {
            return self;
        }

        self.pipe
            .hset_multiple(
                self.redis_conn
                    .final_key(hashmap_namespace, hashmap_key.into()),
                &entries,
            )
            // Ignoring so it doesn't take up a space in the tuple response.
            .ignore();
        if let Some(set_ttl) = hashmap_ttl {
            self.expire(hashmap_namespace, hashmap_key, set_ttl)
        } else {
            RedisBatch {
                _returns: PhantomData,
                redis_conn: self.redis_conn,
                pipe: self.pipe,
                used_scripts: self.used_scripts,
            }
        }
    }

    /// Add an entry to a set (auto creating the set if it doesn't exist).
    /// <https://redis.io/commands/sadd/>
    ///
    /// Arguments:
    /// - `set_namespace`: The namespace of the set.
    /// - `set_key`: The key of the set.
    /// - `set_ttl`: The time to live of the set. This will reset on each addition, meaning after the last update the set will expire after this time.
    /// - `items`: The items to add, values of sets must be strings.
    pub fn sadd<S: AsRef<str>>(
        mut self,
        set_namespace: &str,
        set_key: &str,
        set_ttl: Option<chrono::Duration>,
        items: impl IntoIterator<Item = S>,
    ) -> Self {
        let items = items
            .into_iter()
            .map(|s| s.as_ref().to_string())
            .collect::<Vec<_>>();

        // No-op if no items so skip (redis would actually error if empty anyway)
        if items.is_empty() {
            return self;
        }

        self.pipe
            .sadd(
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

    /// remove an entries from a set.
    /// <https://redis.io/commands/srem/>
    ///
    /// Arguments:
    /// - `set_namespace`: The namespace of the set.
    /// - `set_key`: The key of the set.
    /// - `values`: The values to remove as an iterator.
    pub fn srem<S: AsRef<str>>(
        mut self,
        set_namespace: &str,
        set_key: &str,
        items: impl IntoIterator<Item = S>,
    ) -> Self {
        let items = items
            .into_iter()
            .map(|s| s.as_ref().to_string())
            .collect::<Vec<_>>();

        // No-op if no items so skip (redis would actually error if empty anyway)
        if items.is_empty() {
            return self;
        }

        self.pipe
            .srem(
                self.redis_conn.final_key(set_namespace, set_key.into()),
                items,
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
        // No-op if no items so skip (redis would actually error if empty anyway)
        if items.is_empty() {
            return self;
        }

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
    pub fn clear<S: AsRef<str>>(
        mut self,
        namespace: &str,
        keys: impl IntoIterator<Item = S>,
    ) -> Self {
        let final_keys = keys
            .into_iter()
            .map(|key| {
                self.redis_conn
                    .final_key(namespace, Cow::Borrowed(key.as_ref()))
            })
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
    #[allow(private_bounds)]
    type ReturnType: RedisFuzzyUnwrap;

    /// Commit the batch and return the result.
    /// If redis unavailable, or the types didn't match causing decoding to fail, `None` will be returned and the error logged.
    fn fire(
        self,
    ) -> impl std::future::Future<Output = <Option<Self::ReturnType> as RedisFuzzyUnwrap>::Output>;
}

// The special singular variant that returns the command output directly.
impl<'a, 'c, ConnType: RedisConnLike, R: FromRedisValue + RedisFuzzyUnwrap + Nilcase> RedisBatchFire
    for RedisBatch<'a, 'c, ConnType, (R,)>
{
    type ReturnType = R;

    async fn fire(self) -> <Option<Self::ReturnType> as RedisFuzzyUnwrap>::Output {
        self.inner_fire::<(R,)>().await.map(|(r,)| r).fuzzy_unwrap()
    }
}

macro_rules! impl_batch_fire {
    ( $($tup_item:ident)* ) => (
        impl<'a, 'c, ConnType: RedisConnLike, $($tup_item: FromRedisValue + RedisFuzzyUnwrap + Nilcase),*> RedisBatchFire for RedisBatch<'a, 'c, ConnType, ($($tup_item,)*)> {
            type ReturnType = ($($tup_item,)*);

            async fn fire(self) -> <Option<($($tup_item,)*)> as RedisFuzzyUnwrap>::Output {
                self.inner_fire::<($($tup_item,)*)>().await.fuzzy_unwrap()
            }
        }
    );
}

/// Implements all the supported redis operations that need to modify the return type and hence need macros.
pub trait RedisBatchReturningOps<'c> {
    /// The producer for the next batch struct sig.
    type NextType<T>;

    /// Run an arbitrary redis (lua script), but doesn't add the decode safe RedisFuzzy wrapper.
    /// Meaning if the returned value doesn't match the type, this can fail the pipeline.
    /// Useful internally in crate, external user scripts should probably not use this.
    fn script_no_decode_protection<ScriptOutput: FromRedisValue>(
        self,
        script_invokation: RedisScriptInvoker<'c>,
    ) -> Self::NextType<ScriptOutput>;

    /// Run an arbitrary redis (lua script).
    fn script<ScriptOutput: FromRedisValue>(
        self,
        script_invokation: RedisScriptInvoker<'c>,
    ) -> Self::NextType<RedisFuzzy<ScriptOutput>>;

    /// Low-level backdoor. Pass in a custom redis command to run, specifying the return value to coerce to.
    fn custom<T: FromRedisValue>(self, cmd: &str) -> Self::NextType<T>;

    /// Check if a key exists.
    fn exists(self, namespace: &str, key: &str) -> Self::NextType<bool>;

    /// Check if multiple keys exists.
    fn mexists(
        self,
        namespace: &str,
        keys: impl IntoIterator<Item = impl AsRef<str>>,
    ) -> Self::NextType<Vec<bool>>;

    /// Get a value from a key. Returning `None` if the key doesn't exist.
    fn get<Value: FromRedisValue>(
        self,
        namespace: &str,
        key: &str,
    ) -> Self::NextType<RedisFuzzy<Value>>;

    /// Get multiple values (MGET) of the same type at once. Returning `None` for each key that didn't exist.
    fn mget<Value: FromRedisValue>(
        self,
        namespace: &str,
        keys: impl IntoIterator<Item = impl AsRef<str>>,
    ) -> Self::NextType<Vec<RedisFuzzy<Value>>>;

    /// Get multiple values from a hashmap of the same type at once. Returning `None` for each key that didn't exist.
    /// <https://redis.io/docs/latest/commands/hmget/>
    fn hmget<Value: FromRedisValue>(
        self,
        hashmap_namespace: &str,
        hashmap_key: &str,
        keys: impl IntoIterator<Item = impl AsRef<str>>,
    ) -> Self::NextType<Vec<RedisFuzzy<Value>>>;

    /// Get all members of a set.
    fn smembers(self, set_namespace: &str, set_key: &str) -> Self::NextType<Vec<String>>;

    /// Check whether the provided keys are members of the set (if it exists).
    fn smismember(
        self,
        set_namespace: &str,
        set_key: &str,
        keys: impl IntoIterator<Item = impl AsRef<str>>,
    ) -> Self::NextType<Vec<bool>>;

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
    ) -> Self::NextType<Vec<(RedisFuzzy<Value>, i64)>>;

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
    ) -> Self::NextType<Vec<(RedisFuzzy<Value>, i64)>>;
}

macro_rules! impl_batch_ops {
    ( $($tup_item:ident)* ) => (
        impl<'a, 'c, ConnType: RedisConnLike, $($tup_item: FromRedisValue),*> RedisBatchReturningOps<'c> for RedisBatch<'a, 'c, ConnType, ($($tup_item,)*)> {
            type NextType<T> = RedisBatch<'a, 'c, ConnType, ($($tup_item,)* T,)>;

            fn script_no_decode_protection<ScriptOutput: FromRedisValue>(
                mut self,
                script_invokation: RedisScriptInvoker<'c>,
            ) -> Self::NextType<ScriptOutput> {
                self.pipe.add_command(script_invokation.eval_cmd(
                    self.redis_conn.scripts_loaded().contains(&script_invokation.script.hash)
                ));
                self.used_scripts.insert(script_invokation.script);
                RedisBatch {
                    _returns: PhantomData,
                    redis_conn: self.redis_conn,
                    pipe: self.pipe,
                    used_scripts: self.used_scripts
                }
            }

            fn script<ScriptOutput: FromRedisValue>(
                self,
                script_invokation: RedisScriptInvoker<'c>,
            ) -> Self::NextType<RedisFuzzy<ScriptOutput>> {
                self.script_no_decode_protection(script_invokation)
            }

            fn custom<T: FromRedisValue>(mut self, cmd: &str) -> Self::NextType<T> {
                self.pipe.cmd(cmd);
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

            fn mexists(
                mut self,
                namespace: &str,
                keys: impl IntoIterator<Item = impl AsRef<str>>,
            ) -> Self::NextType<Vec<bool>> {
                let keys = keys.into_iter().map(|key| self.redis_conn.final_key(namespace, key.as_ref().into())).collect::<Vec<_>>();

                // Command might would otherwise fail with an empty vec:
                if keys.is_empty() {
                    pipe_return_empty_vec(&mut self.pipe);
                    RedisBatch {
                        _returns: PhantomData,
                        redis_conn: self.redis_conn,
                        pipe: self.pipe,
                        used_scripts: self.used_scripts
                    }
                } else {
                    let mut invoker = MEXISTS_SCRIPT.invoker();
                    for key in &keys {
                        invoker = invoker.key(key);
                    }
                    self.script_no_decode_protection::<Vec<bool>>(invoker)
                }
            }

            fn get<Value: FromRedisValue>(
                mut self,
                namespace: &str,
                key: &str,
            ) -> Self::NextType<RedisFuzzy<Value>> {
                self.pipe.get(self.redis_conn.final_key(namespace, key.into()));
                RedisBatch {
                    _returns: PhantomData,
                    redis_conn: self.redis_conn,
                    pipe: self.pipe,
                    used_scripts: self.used_scripts
                }
            }

            fn mget<Value: FromRedisValue>(
                mut self,
                namespace: &str,
                keys: impl IntoIterator<Item = impl AsRef<str>>,
            ) -> Self::NextType<Vec<RedisFuzzy<Value>>> {
                let keys = keys.into_iter().map(|key| self.redis_conn.final_key(namespace, key.as_ref().into())).collect::<Vec<_>>();

                // Command might would otherwise fail with an empty vec:
                if keys.is_empty() {
                    pipe_return_empty_vec(&mut self.pipe);
                } else {
                    // self.pipe.hget internally switches between GET and MGET depending on the number of keys.
                    // but this causes problems with decoding, better to always use MGET.
                    self.pipe.cmd("MGET").arg(keys);
                }

                RedisBatch {
                    _returns: PhantomData,
                    redis_conn: self.redis_conn,
                    pipe: self.pipe,
                    used_scripts: self.used_scripts
                }
            }

            fn hmget<Value: FromRedisValue>(
                mut self,
                hashmap_namespace: &str,
                hashmap_key: &str,
                keys: impl IntoIterator<Item = impl AsRef<str>>,
            ) -> Self::NextType<Vec<RedisFuzzy<Value>>> {
                let keys = keys.into_iter().map(|key| key.as_ref().to_string()).collect::<Vec<_>>();

                // Command might would otherwise fail with an empty vec:
                if keys.is_empty() {
                    pipe_return_empty_vec(&mut self.pipe);
                } else {
                    // self.pipe.hget internally switches between HGET and HMGET depending on the number of keys.
                    // but this causes problems with decoding, better to always use HMGET.
                    self.pipe.cmd("HMGET").arg(self.redis_conn.final_key(hashmap_namespace, hashmap_key.into())).arg(keys);
                }

                RedisBatch {
                    _returns: PhantomData,
                    redis_conn: self.redis_conn,
                    pipe: self.pipe,
                    used_scripts: self.used_scripts
                }
            }

            fn smembers(mut self, set_namespace: &str, set_key: &str) -> Self::NextType<Vec<String>> {
                self.pipe
                    .smembers(self.redis_conn.final_key(set_namespace, set_key.into()));
                RedisBatch {
                    _returns: PhantomData,
                    redis_conn: self.redis_conn,
                    pipe: self.pipe,
                    used_scripts: self.used_scripts,
                }
            }

            fn smismember(mut self, set_namespace: &str, set_key: &str, keys: impl IntoIterator<Item = impl AsRef<str>>) -> Self::NextType<Vec<bool>> {
                let keys = keys.into_iter().map(|key| key.as_ref().to_string()).collect::<Vec<_>>();

                // Command might would otherwise fail with an empty vec:
                if keys.is_empty() {
                    pipe_return_empty_vec(&mut self.pipe);
                } else {
                    self.pipe.smismember(self.redis_conn.final_key(set_namespace, set_key.into()), keys);
                }

                RedisBatch {
                    _returns: PhantomData,
                    redis_conn: self.redis_conn,
                    pipe: self.pipe,
                    used_scripts: self.used_scripts,
                }
            }


            fn zrangebyscore_high_to_low<Value: FromRedisValue>(
                mut self,
                set_namespace: &str,
                set_key: &str,
                min: i64,
                max: i64,
                limit: Option<isize>,
            ) -> Self::NextType<Vec<(RedisFuzzy<Value>, i64)>> {
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
            ) -> Self::NextType<Vec<(RedisFuzzy<Value>, i64)>> {
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

fn pipe_return_empty_vec(pipe: &mut Pipeline) {
    // Just run inline script to return nil, which will be fuzzy deserialized to an empty vec:
    // https://redis.io/docs/latest/commands/eval/
    pipe.cmd("EVAL").arg("").arg(0);
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
