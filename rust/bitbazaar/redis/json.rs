use redis::{FromRedisValue, ToRedisArgs};

/// A wrapper on an arbitrary json object to allow reading and writing to redis.
/// Access the inner with .0.
#[derive(Debug)]
pub struct RedisJson<T: serde::Serialize + for<'a> serde::Deserialize<'a>>(pub T);

impl<T: serde::Serialize + for<'a> serde::Deserialize<'a>> FromRedisValue for RedisJson<T> {
    fn from_redis_value(v: &redis::Value) -> redis::RedisResult<Self> {
        match v {
            redis::Value::Data(data) => Ok(Self(serde_json::from_slice(data)?)),
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
#[derive(Debug)]
pub struct RedisJsonBorrowed<'a, T>(pub &'a T)
where
    // Needs to be serializable from the reference, deserializable to T itself:
    T: serde::Deserialize<'a>,
    &'a T: serde::Serialize;

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
