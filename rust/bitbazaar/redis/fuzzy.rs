use std::{collections::HashSet, hash::Hash};

use redis::{FromRedisValue, RedisResult};

use crate::log::record_exception;

use super::RedisJson;

/// A wrapper for returned entries from redis which won't fail the surrounding context if the type is wrong.
/// - When missing, i.e. redis returns nil, sets to None.
/// - When decoding for the target type fails, value set to None, rather than the whole batch failing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RedisFuzzy<T>(Option<T>);

// Copied from upstream redis-rs/redis/src/types.rs
fn get_inner_value(v: &redis::Value) -> &redis::Value {
    if let redis::Value::Attribute {
        data,
        attributes: _,
    } = v
    {
        data.as_ref()
    } else {
        v
    }
}

impl<T: FromRedisValue> FromRedisValue for RedisFuzzy<T> {
    fn from_redis_value(v: &redis::Value) -> redis::RedisResult<Self> {
        let v = get_inner_value(v);

        // Nil is an annoying case, it's returned for both vec![] and false.
        // For empty vec, we take the tradeoff of correct None values when missing in mget and vec<T> etc, but empty vecs return as None.
        // For bool we manually handle here using type_name (tested in mod.rs), as there's no downside and this fixes returning None from certains fns.
        if *v == redis::Value::Nil && std::any::type_name::<T>() != "bool" {
            Ok(Self(None))
        } else {
            Ok(Self(match T::from_redis_value(v) {
                Ok(v) => Some(v),
                Err(e) => {
                    record_exception(
                        format!(
                            "Failed to decode redis value to type '{}'.",
                            std::any::type_name::<T>()
                        ),
                        format!("{:?}", e),
                    );
                    None
                }
            }))
        }
    }
}

/// Used internally to hide all usage of RedisFuzzy in public API.
pub trait RedisFuzzyUnwrap {
    type Output;
    fn fuzzy_unwrap(self) -> Self::Output;
}

// Core:
impl<T: FromRedisValue> RedisFuzzyUnwrap for RedisFuzzy<T> {
    type Output = Option<T>;
    fn fuzzy_unwrap(self) -> Self::Output {
        self.0
    }
}

// Layered options:
impl<T: RedisFuzzyUnwrap> RedisFuzzyUnwrap for Option<T> {
    type Output = Option<T::Output>;
    fn fuzzy_unwrap(self) -> Self::Output {
        self.map(|inner| inner.fuzzy_unwrap())
    }
}

// vecs:
impl<T: RedisFuzzyUnwrap> RedisFuzzyUnwrap for Vec<T> {
    type Output = Vec<T::Output>;
    fn fuzzy_unwrap(self) -> Self::Output {
        self.into_iter().map(|inner| inner.fuzzy_unwrap()).collect()
    }
}

// sets:
impl<T: RedisFuzzyUnwrap> RedisFuzzyUnwrap for HashSet<T>
where
    T::Output: Eq + Hash,
{
    type Output = HashSet<T::Output>;
    fn fuzzy_unwrap(self) -> Self::Output {
        self.into_iter().map(|inner| inner.fuzzy_unwrap()).collect()
    }
}

// Macro for basic types that need to be implemented for internal usage:
macro_rules! impl_basic {
    ($t:ty, $nilcase:expr) => {
        impl RedisFuzzyUnwrap for $t {
            type Output = $t;
            fn fuzzy_unwrap(self) -> Self::Output {
                self
            }
        }

        impl Nilcase for $t {
            fn nilcase() -> Option<Self> {
                $nilcase
            }
        }
    };
}

// Just put None when it should never happen.
impl_basic!(String, None);
impl_basic!(i8, None);
impl_basic!(i16, None);
impl_basic!(i32, None);
impl_basic!(i64, None);
impl_basic!(isize, None);
impl_basic!(u8, None);
impl_basic!(u16, None);
impl_basic!(u32, None);
impl_basic!(u64, None);
impl_basic!(usize, None);
// This does happen, false seems to get returned as nil.
impl_basic!(bool, Some(false));

// Macro to generate impls for RedisFuzzyUnwrap for tuples:
macro_rules! impl_tuple_fuzzy_unwrap {
    ($($n:ident),*) => {
        impl<$($n: RedisFuzzyUnwrap),*> RedisFuzzyUnwrap for ($($n,)*) {
            type Output = ($($n::Output,)*);
            fn fuzzy_unwrap(self) -> Self::Output {
                #[allow(non_snake_case)]
                let ($($n,)*) = self;
                #[allow(clippy::unused_unit)]
                ($($n.fuzzy_unwrap(),)*)
            }
        }
    };
}

// For up to 12:
impl_tuple_fuzzy_unwrap! {}
impl_tuple_fuzzy_unwrap! { A }
impl_tuple_fuzzy_unwrap! { A, B }
impl_tuple_fuzzy_unwrap! { A, B, C }
impl_tuple_fuzzy_unwrap! { A, B, C, D }
impl_tuple_fuzzy_unwrap! { A, B, C, D, E }
impl_tuple_fuzzy_unwrap! { A, B, C, D, E, F }
impl_tuple_fuzzy_unwrap! { A, B, C, D, E, F, G }
impl_tuple_fuzzy_unwrap! { A, B, C, D, E, F, G, H }
impl_tuple_fuzzy_unwrap! { A, B, C, D, E, F, G, H, I }
impl_tuple_fuzzy_unwrap! { A, B, C, D, E, F, G, H, I, J }
impl_tuple_fuzzy_unwrap! { A, B, C, D, E, F, G, H, I, J, K }
impl_tuple_fuzzy_unwrap! { A, B, C, D, E, F, G, H, I, J, K, L }

pub(crate) trait Nilcase: Sized {
    // Some() for specific nil handling.
    // None to default to from_redis_value.
    fn nilcase() -> Option<Self>;
}

impl<T> Nilcase for RedisJson<T> {
    fn nilcase() -> Option<Self> {
        // nil shouldn't happen for these, they're strings:
        None
    }
}

impl<T: Nilcase> Nilcase for RedisFuzzy<T> {
    fn nilcase() -> Option<Self> {
        Some(Self(T::nilcase()))
    }
}

impl<T> Nilcase for Option<T> {
    fn nilcase() -> Option<Self> {
        Some(None)
    }
}

impl<T> Nilcase for Vec<T> {
    fn nilcase() -> Option<Self> {
        Some(Vec::new())
    }
}

pub(crate) trait FuzzyFromRedisValue: Sized {
    fn fuzzy_from_redis_value(v: &redis::Value) -> RedisResult<Self>;
}

impl FuzzyFromRedisValue for () {
    fn fuzzy_from_redis_value(v: &redis::Value) -> RedisResult<Self> {
        <() as FromRedisValue>::from_redis_value(v)
    }
}

// Macro to generate impls for FuzzyFromRedisValue for >1 tuples:
macro_rules! impl_tuple_fuzzy_from_redis_value {
    ($($n:ident),*) => {
        impl<$($n: Nilcase + FromRedisValue),*> FuzzyFromRedisValue for ($($n,)*) {
            fn fuzzy_from_redis_value(v: &redis::Value) -> RedisResult<Self> {
                match v {
                    redis::Value::Nil => Ok(( $($n::nilcase().map(Ok).unwrap_or_else(|| $n::from_redis_value(v))?,)* )),
                    redis::Value::Array(arr) => {
                        // For each, if nil then set to nilcase, else decode.
                        let mut iter = arr.iter();
                        Ok(( $(match iter.next() {
                            Some(v) => match v {
                                redis::Value::Nil => $n::nilcase().map(Ok).unwrap_or_else(|| $n::from_redis_value(v))?,
                                _ => $n::from_redis_value(v)?,
                            },
                            None => return Err(redis::RedisError::from((
                                redis::ErrorKind::TypeError,
                                "Array too short",
                            ))),
                        },)* ))
                    },
                    _ => Ok(( $($n::from_redis_value(v)?,)* )),
                }
            }
        }
    };
}

impl_tuple_fuzzy_from_redis_value! { A }
impl_tuple_fuzzy_from_redis_value! { A, B }
impl_tuple_fuzzy_from_redis_value! { A, B, C }
impl_tuple_fuzzy_from_redis_value! { A, B, C, D }
impl_tuple_fuzzy_from_redis_value! { A, B, C, D, E }
impl_tuple_fuzzy_from_redis_value! { A, B, C, D, E, F }
impl_tuple_fuzzy_from_redis_value! { A, B, C, D, E, F, G }
impl_tuple_fuzzy_from_redis_value! { A, B, C, D, E, F, G, H }
impl_tuple_fuzzy_from_redis_value! { A, B, C, D, E, F, G, H, I }
impl_tuple_fuzzy_from_redis_value! { A, B, C, D, E, F, G, H, I, J }
impl_tuple_fuzzy_from_redis_value! { A, B, C, D, E, F, G, H, I, J, K }
impl_tuple_fuzzy_from_redis_value! { A, B, C, D, E, F, G, H, I, J, K, L }
