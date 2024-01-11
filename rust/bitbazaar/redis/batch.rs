use std::{borrow::Cow, collections::HashSet, marker::PhantomData};

use deadpool_redis::redis::{FromRedisValue, Pipeline, ToRedisArgs};
use once_cell::sync::Lazy;

use super::{RedisConn, RedisScript, RedisScriptInvoker};

static CLEAR_NAMESPACE_SCRIPT: Lazy<RedisScript> =
    Lazy::new(|| RedisScript::new(include_str!("clear_namespace.lua")));

static MEXISTS_SCRIPT: Lazy<RedisScript> =
    Lazy::new(|| RedisScript::new(include_str!("mexists.lua")));

/// A command builder struct. Committed with [`RedisBatch::fire`].
///
/// Batched commands are run in order, but other commands from different sources may be interleaved.
/// Note each command may be run twice, if scripts needed caching to redis.
pub struct RedisBatch<'a, 'b, 'c, ReturnType = ()> {
    _returns: PhantomData<ReturnType>,
    redis_conn: &'a mut RedisConn<'b>,
    pipe: Pipeline,
    /// Need to keep a reference to used scripts, these will all be reloaded to redis errors because one wasn't cached on the server.
    used_scripts: HashSet<&'c RedisScript>,
}

impl<'a, 'b, 'c, ReturnType> RedisBatch<'a, 'b, 'c, ReturnType> {
    pub(crate) fn new(redis_conn: &'a mut RedisConn<'b>) -> Self {
        let mut pipe = deadpool_redis::redis::pipe();
        // Make sure if anything goes wrong the whole thing rolls back:
        pipe.atomic();
        Self {
            _returns: PhantomData,
            redis_conn,
            pipe,
            used_scripts: HashSet::new(),
        }
    }

    async fn inner_fire<R: FromRedisValue>(&mut self) -> Option<R> {
        if let Some(conn) = self.redis_conn.get_conn().await {
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
}

// The special singular variant that returns the command output directly.
impl<'a, 'b, 'c, R: FromRedisValue> RedisBatch<'a, 'b, 'c, (R,)> {
    /// Commit the batch and return the result.
    /// If redis unavailable, or the types didn't match causing decoding to fail, `None` will be returned and the error logged.
    ///
    /// Note this is the special singular variant that returns the command output directly (no tuple).
    pub async fn fire(mut self) -> Option<R>
    where
        (R,): FromRedisValue,
    {
        self.inner_fire().await.map(|(r,)| r)
    }
}

macro_rules! impl_batch_fire_tuple {
    ($($index:tt: $type:ident),*) => {
        impl<'a, 'b, 'c, $($type: FromRedisValue),*> RedisBatch<'a, 'b, 'c, ($($type,)*)> {
            /// Commit the batch and return the command results in a tuple.
            /// If redis unavailable, or the types didn't match causing decoding to fail, `None` will be returned and the error logged.
            pub async fn fire(mut self) -> Option<($($type,)*)>
            where
                ($($type,)*): FromRedisValue,
            {
                self.inner_fire().await
            }
        }
    };
}

// Implement batch fire() for up to 16 operations: (EXCEPT FOR one command, which is implemented separately to return the value itself rather than the tuple)
impl_batch_fire_tuple!();
// impl_batch_fire_tuple!(0: A); // Not this one, its got a custom implementation.
impl_batch_fire_tuple!(0: A, 1: B);
impl_batch_fire_tuple!(0: A, 1: B, 2: C);
impl_batch_fire_tuple!(0: A, 1: B, 2: C, 3: D);
impl_batch_fire_tuple!(0: A, 1: B, 2: C, 3: D, 4: E);
impl_batch_fire_tuple!(0: A, 1: B, 2: C, 3: D, 4: E, 5: F);
impl_batch_fire_tuple!(0: A, 1: B, 2: C, 3: D, 4: E, 5: F, 6: G);
impl_batch_fire_tuple!(0: A, 1: B, 2: C, 3: D, 4: E, 5: F, 6: G, 7: H);
impl_batch_fire_tuple!(0: A, 1: B, 2: C, 3: D, 4: E, 5: F, 6: G, 7: H, 8: I);
impl_batch_fire_tuple!(0: A, 1: B, 2: C, 3: D, 4: E, 5: F, 6: G, 7: H, 8: I, 9: J);
impl_batch_fire_tuple!(0: A, 1: B, 2: C, 3: D, 4: E, 5: F, 6: G, 7: H, 8: I, 9: J, 10: K);
impl_batch_fire_tuple!(0: A, 1: B, 2: C, 3: D, 4: E, 5: F, 6: G, 7: H, 8: I, 9: J, 10: K, 11: L);
impl_batch_fire_tuple!(0: A, 1: B, 2: C, 3: D, 4: E, 5: F, 6: G, 7: H, 8: I, 9: J, 10: K, 11: L, 12: M);
impl_batch_fire_tuple!(0: A, 1: B, 2: C, 3: D, 4: E, 5: F, 6: G, 7: H, 8: I, 9: J, 10: K, 11: L, 12: M, 13: N);
impl_batch_fire_tuple!(0: A, 1: B, 2: C, 3: D, 4: E, 5: F, 6: G, 7: H, 8: I, 9: J, 10: K, 11: L, 12: M, 13: N, 14: O);
impl_batch_fire_tuple!(0: A, 1: B, 2: C, 3: D, 4: E, 5: F, 6: G, 7: H, 8: I, 9: J, 10: K, 11: L, 12: M, 13: N, 14: O, 15: P);

macro_rules! impl_batch_methods {
    ($($index:tt: $type:ident),*) => {
        impl<'a, 'b, 'c, $($type: FromRedisValue),*> RedisBatch<'a, 'b, 'c, ($($type,)*)> {
            /// Run an arbitrary redis (lua script).
            pub fn script<ScriptOutput>(mut self, script_invokation: RedisScriptInvoker<'c>) -> RedisBatch<'a, 'b, 'c, ($($type,)* ScriptOutput,)>
            where
                ScriptOutput: FromRedisValue,
            {
                self.pipe.add_command(script_invokation.eval_cmd());
                self.used_scripts.insert(script_invokation.script);
                RedisBatch {
                    _returns: PhantomData,
                    redis_conn: self.redis_conn,
                    pipe: self.pipe,
                    used_scripts: self.used_scripts
                }
            }

            /// Run an arbitrary redis (lua script). But discards any return value.
            pub fn script_no_return(mut self, script_invokation: RedisScriptInvoker<'c>) -> RedisBatch<'a, 'b, 'c, ($($type,)*)>
            {
                // Adding ignore() to ignore response.
                self.pipe.add_command(script_invokation.eval_cmd()).ignore();
                self.used_scripts.insert(script_invokation.script);
                RedisBatch {
                    _returns: PhantomData,
                    redis_conn: self.redis_conn,
                    pipe: self.pipe,
                    used_scripts: self.used_scripts
                }
            }

            /// Set a key to a value.
            pub fn set<'key, Key, Value>(mut self, namespace: &'static str, key: Key, value: Value) -> RedisBatch<'a, 'b, 'c, ($($type,)*)>
            where
                Key: Into<Cow<'key, str>>,
                Value: ToRedisArgs,
            {
                // Ignoring so it doesn't take up a space in the tuple response.
                self.pipe.set(self.redis_conn.final_key(namespace, key.into()), value).ignore();
                RedisBatch {
                    _returns: PhantomData,
                    redis_conn: self.redis_conn,
                    pipe: self.pipe,
                    used_scripts: self.used_scripts
                }
            }

            /// Set multiple values (MSET) of the same type at once.
            pub fn mset<'key, Key, Value, Pairs>(mut self, namespace: &'static str, pairs: Pairs) -> RedisBatch<'a, 'b, 'c, ($($type,)*)>
            where
                Value: ToRedisArgs,
                Key: Into<Cow<'key, str>>,
                Pairs: IntoIterator<Item = (Key, Value)>,
            {
                let final_pairs = pairs.into_iter().map(|(key, value)| (self.redis_conn.final_key(namespace, key.into()), value)).collect::<Vec<_>>();

                // Ignoring so it doesn't take up a space in the tuple response.
                self.pipe.mset(&final_pairs).ignore();
                RedisBatch {
                    _returns: PhantomData,
                    redis_conn: self.redis_conn,
                    pipe: self.pipe,
                    used_scripts: self.used_scripts
                }
            }

            /// Clear one or more keys.
            pub fn clear<'key, Keys, Key>(mut self, namespace: &'static str, keys: Keys) -> RedisBatch<'a, 'b, 'c, ($($type,)*)>
            where
                Keys: IntoIterator<Item = Key>,
                Key: Into<Cow<'key, str>>,
            {
                let final_keys = keys.into_iter().map(Into::into).map(|key| self.redis_conn.final_key(namespace, key)).collect::<Vec<_>>();
                // Ignoring so it doesn't take up a space in the tuple response.
                self.pipe.del(final_keys).ignore();
                RedisBatch {
                    _returns: PhantomData,
                    redis_conn: self.redis_conn,
                    pipe: self.pipe,
                    used_scripts: self.used_scripts
                }
            }

            /// Clear all keys under a given namespace
            pub fn clear_namespace(self, namespace: &'static str) -> RedisBatch<'a, 'b, 'c, ($($type,)*)>
            {
                let final_namespace = self.redis_conn.final_namespace(namespace);
                self.script_no_return(CLEAR_NAMESPACE_SCRIPT.invoker().arg(final_namespace))
            }

            /// Check if a key exists.
            pub fn exists<'key, Key>(mut self, namespace: &'static str, key: Key) -> RedisBatch<'a, 'b, 'c, ($($type,)* bool,)>
            where
                Key: Into<Cow<'key, str>>,
            {
                self.pipe.exists(self.redis_conn.final_key(namespace, key.into()));
                RedisBatch {
                    _returns: PhantomData,
                    redis_conn: self.redis_conn,
                    pipe: self.pipe,
                    used_scripts: self.used_scripts
                }
            }

            /// Check if multiple keys exists.
            pub fn mexists<'key, Keys, Key>(self, namespace: &'static str, keys: Keys) -> RedisBatch<'a, 'b, 'c, ($($type,)* Vec<bool>,)>
            where
                Keys: IntoIterator<Item = Key>,
                Key: Into<Cow<'key, str>>,
            {
                let final_keys = keys.into_iter().map(Into::into).map(|key| self.redis_conn.final_key(namespace, key)).collect::<Vec<_>>();
                let mut invoker = MEXISTS_SCRIPT.invoker();
                for key in &final_keys {
                    invoker = invoker.key(key);
                }
                self.script::<Vec<bool>>(invoker)
            }

            /// Get a value from a key. Returning `None` if the key doesn't exist.
            pub fn get<'key, Value, Key>(mut self, namespace: &'static str, key: Key) -> RedisBatch<'a, 'b, 'c, ($($type,)* Option<Value>,)>
            where
                Key: Into<Cow<'key, str>>,
                Value: FromRedisValue
            {
                self.pipe.get(self.redis_conn.final_key(namespace, key.into()));
                RedisBatch {
                    _returns: PhantomData,
                    redis_conn: self.redis_conn,
                    pipe: self.pipe,
                    used_scripts: self.used_scripts
                }
            }

            /// Get multiple values (MGET) of the same type at once. Returning `None` for each key that didn't exist.
            pub fn mget<'key, Value, Keys, Key>(mut self, namespace: &'static str, keys: Keys) -> RedisBatch<'a, 'b, 'c, ($($type,)* Vec<Option<Value>>,)>
            where
                Keys: IntoIterator<Item = Key>,
                Key: Into<Cow<'key, str>>,
                Value: FromRedisValue
            {
                let final_keys = keys.into_iter().map(Into::into).map(|key| self.redis_conn.final_key(namespace, key)).collect::<Vec<_>>();

                self.pipe.get(final_keys);
                RedisBatch {
                    _returns: PhantomData,
                    redis_conn: self.redis_conn,
                    pipe: self.pipe,
                    used_scripts: self.used_scripts
                }
            }
        }
    };
}

// Implement batch methods for up to 16 operations:
impl_batch_methods!();
impl_batch_methods!(0: A);
impl_batch_methods!(0: A, 1: B);
impl_batch_methods!(0: A, 1: B, 2: C);
impl_batch_methods!(0: A, 1: B, 2: C, 3: D);
impl_batch_methods!(0: A, 1: B, 2: C, 3: D, 4: E);
impl_batch_methods!(0: A, 1: B, 2: C, 3: D, 4: E, 5: F);
impl_batch_methods!(0: A, 1: B, 2: C, 3: D, 4: E, 5: F, 6: G);
impl_batch_methods!(0: A, 1: B, 2: C, 3: D, 4: E, 5: F, 6: G, 7: H);
impl_batch_methods!(0: A, 1: B, 2: C, 3: D, 4: E, 5: F, 6: G, 7: H, 8: I);
impl_batch_methods!(0: A, 1: B, 2: C, 3: D, 4: E, 5: F, 6: G, 7: H, 8: I, 9: J);
impl_batch_methods!(0: A, 1: B, 2: C, 3: D, 4: E, 5: F, 6: G, 7: H, 8: I, 9: J, 10: K);
impl_batch_methods!(0: A, 1: B, 2: C, 3: D, 4: E, 5: F, 6: G, 7: H, 8: I, 9: J, 10: K, 11: L);
impl_batch_methods!(0: A, 1: B, 2: C, 3: D, 4: E, 5: F, 6: G, 7: H, 8: I, 9: J, 10: K, 11: L, 12: M);
impl_batch_methods!(0: A, 1: B, 2: C, 3: D, 4: E, 5: F, 6: G, 7: H, 8: I, 9: J, 10: K, 11: L, 12: M, 13: N);
impl_batch_methods!(0: A, 1: B, 2: C, 3: D, 4: E, 5: F, 6: G, 7: H, 8: I, 9: J, 10: K, 11: L, 12: M, 13: N, 14: O);
impl_batch_methods!(0: A, 1: B, 2: C, 3: D, 4: E, 5: F, 6: G, 7: H, 8: I, 9: J, 10: K, 11: L, 12: M, 13: N, 14: O, 15: P);
