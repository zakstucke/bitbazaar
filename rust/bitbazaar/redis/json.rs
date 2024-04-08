use redis::{FromRedisValue, ToRedisArgs};

/// A wrapper on an arbitrary json object to allow reading and writing to redis.
/// Use [`RedisJsonConsume::consume`] to extract the inner json object.
#[derive(Debug)]
pub struct RedisJson<T: serde::Serialize + for<'a> serde::Deserialize<'a>>(pub T);

/// If the object is comparable, make the wrapper too:
impl<T: serde::Serialize + for<'a> serde::Deserialize<'a> + PartialEq> PartialEq for RedisJson<T> {
    fn eq(&self, other: &Self) -> bool {
        self.0 == other.0
    }
}
/// If the object is comparable, make the wrapper too:
impl<T: serde::Serialize + for<'a> serde::Deserialize<'a> + Eq> Eq for RedisJson<T> {}

/// A trait to allow consuming a redis wrapper and returning the inner json object.
pub trait RedisJsonConsume<T> {
    /// Extract the inner json object from the redis wrapper.
    fn consume(self) -> T;
}

impl<T: serde::Serialize + for<'a> serde::Deserialize<'a>> RedisJsonConsume<T> for RedisJson<T> {
    fn consume(self) -> T {
        self.0
    }
}

impl<T: serde::Serialize + for<'a> serde::Deserialize<'a>> RedisJsonConsume<Option<T>>
    for Option<RedisJson<T>>
{
    fn consume(self) -> Option<T> {
        self.map(|x| x.0)
    }
}

impl<T> FromRedisValue for RedisJson<T>
where
    T: serde::Serialize + for<'a> serde::Deserialize<'a>,
{
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

impl<T> ToRedisArgs for RedisJson<T>
where
    T: serde::Serialize + for<'a> serde::Deserialize<'a>,
{
    fn write_redis_args<W>(&self, out: &mut W)
    where
        W: ?Sized + redis::RedisWrite,
    {
        let data = serde_json::to_vec(&self.0).unwrap();
        out.write_arg(&data)
    }
}
