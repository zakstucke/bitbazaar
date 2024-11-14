use std::fmt::Debug;

use redis::{FromRedisValue, ToRedisArgs};

/// A wrapper on an arbitrary json object to allow reading and writing to redis.
/// Access the inner with .0.
#[derive(Clone, PartialEq, Eq)]
pub struct RedisJson<T>(pub T);

impl<T: Debug> Debug for RedisJson<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("RedisJson").field(&self.0).finish()
    }
}

impl<T: serde::Serialize + for<'a> serde::Deserialize<'a>> FromRedisValue for RedisJson<T> {
    fn from_redis_value(v: &redis::Value) -> redis::RedisResult<Self> {
        match v {
            redis::Value::BulkString(data) => Ok(Self(serde_json::from_slice(data)?)),
            _ => Err(redis::RedisError::from((
                redis::ErrorKind::TypeError,
                "Cannot convert to Serialize",
            ))),
        }
    }
}

impl<T: serde::Serialize + for<'a> serde::Deserialize<'a>> ToRedisArgs for RedisJson<T> {
    fn write_redis_args<W>(&self, out: &mut W)
    where
        W: ?Sized + redis::RedisWrite,
    {
        let data = serde_json::to_vec(&self.0).unwrap();
        out.write_arg(&data)
    }
}

/// A borrowed wrapper on an arbitrary json object to writing to redis.
/// Use this over [`RedisJson`] when you don't want to own the data and prevent unnecessary cloning.
/// Access the inner with .0.
pub struct RedisJsonBorrowed<'a, T>(pub &'a T);

impl<'a, T: Debug> Debug for RedisJsonBorrowed<'a, T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("RedisJsonBorrowed").field(&self.0).finish()
    }
}

impl<'a, T> ToRedisArgs for RedisJsonBorrowed<'a, T>
where
    // Needs to be serializable from the reference, deserializable to T itself:
    T: serde::Deserialize<'a>,
    &'a T: serde::Serialize,
{
    fn write_redis_args<W>(&self, out: &mut W)
    where
        W: ?Sized + redis::RedisWrite,
    {
        let data = serde_json::to_vec(&self.0).unwrap();
        out.write_arg(&data)
    }
}
