use deadpool_redis::redis::{FromRedisValue, Script, ScriptInvocation};

use super::RedisConn;
use crate::errors::TracedResult;

/// A wrapper around a redis script.
pub struct RedisScript {
    script: Script,
}

impl RedisScript {
    /// Create a new redis script from the given static string. This object should be reused.
    pub fn new(script: &'static str) -> Self {
        Self {
            script: Script::new(script),
        }
    }

    /// Run the script with the given keys and args.
    pub async fn run<'a, 'b, ReturnType>(
        &self,
        conn: &mut RedisConn<'a>,
        cb: impl FnOnce(&mut ScriptInvocation<'_>),
    ) -> TracedResult<ReturnType>
    where
        ReturnType: FromRedisValue,
    {
        let mut scr = self.script.prepare_invoke();
        cb(&mut scr);
        Ok(scr.invoke_async(conn.get_conn().await?).await?)
    }
}
