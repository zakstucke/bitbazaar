use std::{borrow::Cow, marker::PhantomData};

use deadpool_redis::redis::{FromRedisValue, Pipeline, ToRedisArgs};

use super::RedisConn;
use crate::errors::TracedResult;

/// A command builder struct. Committed with [`RedisBatch::fire`].
pub struct RedisBatch<'a, 'b, ReturnType = ()> {
    _returns: PhantomData<ReturnType>,
    redis_conn: &'a mut RedisConn<'b>,
    pipe: Pipeline,
}

impl<'a, 'b, ReturnType> RedisBatch<'a, 'b, ReturnType> {
    pub(crate) fn new(redis_conn: &'a mut RedisConn<'b>) -> Self {
        Self {
            _returns: PhantomData,
            redis_conn,
            pipe: deadpool_redis::redis::pipe(),
        }
    }
}

impl<'a, 'b, R: FromRedisValue> RedisBatch<'a, 'b, (R,)> {
    /// Commit the batch and return the result.
    ///
    /// Note this is the special singular variant that returns the command output directly.
    pub async fn fire(self) -> TracedResult<R>
    where
        (R,): FromRedisValue,
    {
        let result: (R,) = self
            .pipe
            .query_async(self.redis_conn.get_conn().await?)
            .await?;
        Ok(result.0)
    }
}

macro_rules! impl_batch_fire_tuple {
    ($($index:tt: $type:ident),*) => {
        impl<'a, 'b, $($type: FromRedisValue),*> RedisBatch<'a, 'b, ($($type,)*)> {
            /// Commit the batch and return the command results in a tuple.
            pub async fn fire(self) -> TracedResult<($($type,)*)>
            where
                ($($type,)*): FromRedisValue,
            {
                Ok(self
                    .pipe
                    .query_async(self.redis_conn.get_conn().await?)
                    .await?)
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
        impl<'a, 'b, $($type: FromRedisValue),*> RedisBatch<'a, 'b, ($($type,)*)> {
            /// Set a key to a value.
            pub fn set<'c, Key, Value>(mut self, key: Key, value: Value) -> RedisBatch<'a, 'b, ($($type,)*)>
            where
                Key: Into<Cow<'c, str>>,
                Value: ToRedisArgs,
            {
                // Ignoring so it doesn't take up a space in the tuple response.
                self.pipe.set(self.redis_conn.final_key(key.into()), value).ignore();
                RedisBatch {
                    _returns: PhantomData,
                    redis_conn: self.redis_conn,
                    pipe: self.pipe
                }
            }

            /// Get a value from a key. Returning `None` if the key doesn't exist.
            pub fn get<'c, Value, Key>(mut self, key: Key) -> RedisBatch<'a, 'b, ($($type,)* Option<Value>,)>
            where
                Key: Into<Cow<'c, str>>,
            {
                self.pipe.get(self.redis_conn.final_key(key.into()));
                RedisBatch {
                    _returns: PhantomData,
                    redis_conn: self.redis_conn,
                    pipe: self.pipe
                }
            }

            /// Get multiple values (MGET) of the same type at once. Returning `None` for each key that didn't exist.
            pub fn mget<'c, Value, Keys, Key>(mut self, keys: Keys) -> RedisBatch<'a, 'b, ($($type,)* Vec<Option<Value>>,)>
            where
                Keys: IntoIterator<Item = Key>,
                Key: Into<Cow<'c, str>>,
            {
                let final_keys = keys.into_iter().map(Into::into).map(|key| self.redis_conn.final_key(key)).collect::<Vec<_>>();

                self.pipe.get(final_keys);
                RedisBatch {
                    _returns: PhantomData,
                    redis_conn: self.redis_conn,
                    pipe: self.pipe
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
