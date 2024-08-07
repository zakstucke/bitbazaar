use redis::FromRedisValue;

use crate::log::record_exception;

/// A wrapper for returned entries from redis.
/// - When missing, i.e. redis returns nil, sets to None.
/// - When decoding for the target type fails, value set to None, rather than the whole batch failing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RVal<T: FromRedisValue>(Option<T>);

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

impl<T: FromRedisValue> FromRedisValue for RVal<T> {
    fn from_redis_value(v: &redis::Value) -> redis::RedisResult<Self> {
        let v = get_inner_value(v);
        if *v == redis::Value::Nil {
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

/// Used internally to hide all usage of RVal in public API.
pub(crate) trait RValIntoSensible {
    type Output;
    fn into_sensible(self) -> Self::Output;
}

// Core:
impl<T: FromRedisValue> RValIntoSensible for RVal<T> {
    type Output = Option<T>;
    fn into_sensible(self) -> Self::Output {
        self.0
    }
}

// Layered options:
impl<T: RValIntoSensible> RValIntoSensible for Option<T> {
    type Output = Option<T::Output>;
    fn into_sensible(self) -> Self::Output {
        self.map(|inner| inner.into_sensible())
    }
}

// vecs:
impl<T: RValIntoSensible> RValIntoSensible for Vec<T> {
    type Output = Vec<T::Output>;
    fn into_sensible(self) -> Self::Output {
        self.into_iter()
            .map(|inner| inner.into_sensible())
            .collect()
    }
}

// Macro for basic types that need to be implemented for internal usage:
macro_rules! impl_basic {
    ($t:ty) => {
        impl RValIntoSensible for $t {
            type Output = $t;
            fn into_sensible(self) -> Self::Output {
                self
            }
        }
    };
}

impl_basic!(i64);
impl_basic!(bool);

// Macro to generate impls for tuples:
macro_rules! impl_tuple {
    ($($n:ident),*) => {
        impl<$($n: RValIntoSensible),*> RValIntoSensible for ($($n,)*) {
            type Output = ($($n::Output,)*);
            fn into_sensible(self) -> Self::Output {
                #[allow(non_snake_case)]
                let ($($n,)*) = self;
                #[allow(clippy::unused_unit)]
                ($($n.into_sensible(),)*)
            }
        }
    };
}

// For up to 12:
impl_tuple! {}
impl_tuple! { A }
impl_tuple! { A, B }
impl_tuple! { A, B, C }
impl_tuple! { A, B, C, D }
impl_tuple! { A, B, C, D, E }
impl_tuple! { A, B, C, D, E, F }
impl_tuple! { A, B, C, D, E, F, G }
impl_tuple! { A, B, C, D, E, F, G, H }
impl_tuple! { A, B, C, D, E, F, G, H, I }
impl_tuple! { A, B, C, D, E, F, G, H, I, J }
impl_tuple! { A, B, C, D, E, F, G, H, I, J, K }
impl_tuple! { A, B, C, D, E, F, G, H, I, J, K, L }
