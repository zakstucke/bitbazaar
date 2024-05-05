// Derived from https://github.com/hexcowboy/rslock

// Copyright (c) 2014-2021, Jan-Erik Rediger

// Redistribution and use in source and binary forms, with or without modification, are permitted provided that the following conditions are met:

// * Redistributions of source code must retain the above copyright notice, this list of conditions and the following disclaimer.
// * Redistributions in binary form must reproduce the above copyright notice, this list of conditions and the following disclaimer in the documentation and/or other materials provided with the distribution.
// * Neither the name of Redis nor the names of its contributors may be used to endorse or promote products derived from this software without specific prior written permission.

// THIS SOFTWARE IS PROVIDED BY THE COPYRIGHT HOLDERS AND CONTRIBUTORS "AS IS" AND ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE ARE DISCLAIMED. IN NO EVENT SHALL THE COPYRIGHT OWNER OR CONTRIBUTORS BE LIABLE FOR ANY DIRECT, INDIRECT, INCIDENTAL, SPECIAL, EXEMPLARY, OR CONSEQUENTIAL DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS OR SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS INTERRUPTION) HOWEVER CAUSED AND ON ANY THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY OUT OF THE USE OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF SUCH DAMAGE.

use std::time::{Duration, Instant};

use error_stack::Context;
use futures::{future::join_all, Future};
use futures_timer::Delay;
use once_cell::sync::Lazy;
use rand::{thread_rng, Rng, RngCore};
use redis::{RedisResult, Value};

use super::{RedisBatchFire, RedisBatchReturningOps, RedisConn, RedisScript};
use crate::prelude::*;

const RETRY_DELAY: u32 = 200;
const CLOCK_DRIFT_FACTOR: f32 = 0.01;

const UNLOCK_LUA: &str = r#"
if redis.call("GET", KEYS[1]) == ARGV[1] then
  return redis.call("DEL", KEYS[1])
else
  return 0
end
"#;
const EXTEND_LUA: &str = r#"
if redis.call("get", KEYS[1]) ~= ARGV[1] then
  return 0
else
  if redis.call("set", KEYS[1], ARGV[1], "PX", ARGV[2]) ~= nil then
    return 1
  else
    return 0
  end
end
"#;

static UNLOCK_SCRIPT: Lazy<RedisScript> = Lazy::new(|| RedisScript::new(UNLOCK_LUA));
static EXTEND_SCRIPT: Lazy<RedisScript> = Lazy::new(|| RedisScript::new(EXTEND_LUA));

/// Errors that can occur when trying to lock a resource.
#[derive(Debug)]
pub enum RedisLockErr {
    /// When the lock is held by someone else.
    Unavailable,
    /// When the user has done something wrong.
    UserErr,
}

impl std::fmt::Display for RedisLockErr {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RedisLockErr::UserErr => write!(f, "User error"),
            RedisLockErr::Unavailable => write!(f, "Lock unavailable"),
        }
    }
}

impl Context for RedisLockErr {}

/// A distributed lock for Redis.
pub struct RedisLock<'a> {
    redis: &'a super::Redis,
    /// The resource to lock. Will be used as the key in Redis.
    pub lock_id: Vec<u8>,
    /// The value for this lock.
    pub val: Vec<u8>,
    /// How long to wait before giving up trying to get the lock.
    pub wait_up_to: Option<Duration>,
}

impl<'a> RedisLock<'a> {
    /// Creates a new lock, use [`super::Redis::dlock`] instead.
    pub(crate) async fn new(
        redis: &'a super::Redis,
        lock_id: &str,
        ttl: Duration,
        wait_up_to: Option<Duration>,
    ) -> Result<RedisLock<'a>, RedisLockErr> {
        if ttl < Duration::from_millis(100) {
            return Err(err!(
                RedisLockErr::UserErr,
                "Do not set time to live to less than 100 milliseconds."
            ));
        }

        let mut lock = RedisLock {
            redis,
            lock_id: lock_id.as_bytes().to_vec(),
            val: get_unique_lock_id(),
            wait_up_to,
        };

        // Need to actually lock for the first time:
        let lock_id = lock.lock_id.clone();
        let val = lock.val.clone();
        lock.exec_or_retry(ttl, move |mut conn| {
            let lock_id = lock_id.clone();
            let val = val.clone();
            async move {
                if let Some(conn) = conn.get_inner_conn().await {
                    let result: RedisResult<Value> = redis::cmd("SET")
                        .arg(lock_id)
                        .arg(val)
                        .arg("NX")
                        .arg("PX")
                        .arg(ttl.as_millis() as usize)
                        .query_async(conn)
                        .await;

                    match result {
                        Ok(Value::Okay) => true,
                        Ok(_) | Err(_) => false,
                    }
                } else {
                    false
                }
            }
        })
        .await?;

        Ok(lock)
    }

    /// Extend the lifetime of the lock from the previous ttl.
    /// Note this will be the new ttl from this point, meaning if this is called with 10 seconds, the lock will be killed after 10 seconds, not the prior remaining plus 10 seconds.
    ///
    /// Returns:
    /// true: the lock was successfully extended.
    /// false: the lock could not be extended for some reason.
    pub async fn extend(&mut self, new_ttl: Duration) -> Result<bool, RedisLockErr> {
        if new_ttl < Duration::from_millis(100) {
            return Err(err!(
                RedisLockErr::UserErr,
                "Do not set time to live to less than 100 milliseconds."
            ));
        }

        let lock_id = self.lock_id.clone();
        let val = self.val.clone();
        self.exec_or_retry(new_ttl, move |mut conn| {
            let lock_id = lock_id.clone();
            let val = val.clone();
            async move {
                // TODONOW script types should be as generic args.
                let result: Option<i32> = conn
                    .batch()
                    .script(
                        EXTEND_SCRIPT
                            .invoker()
                            .key(lock_id)
                            .arg(val)
                            .arg(new_ttl.as_millis() as usize),
                    )
                    .fire()
                    .await;

                match result {
                    Some(val) => val == 1,
                    None => false,
                }
            }
        })
        .await
    }

    /// Unlock the lock manually.
    /// Not necessarily needed, the lock will expire automatically after the TTL.
    ///
    /// Returns:
    /// true: the lock was successfully unlocked.
    /// false: the lock could not be unlocked for some reason.
    pub async fn unlock(&mut self) -> bool {
        let result = join_all(
            self.redis
                .get_conn_to_each_server()
                .into_iter()
                .map(|mut conn| {
                    let lock_id = self.lock_id.clone();
                    let val = self.val.clone();
                    async move {
                        let result: Option<i32> = conn
                            .batch()
                            .script(UNLOCK_SCRIPT.invoker().key(lock_id).arg(val))
                            .fire()
                            .await;

                        match result {
                            Some(val) => val == 1,
                            _ => false,
                        }
                    }
                }),
        )
        .await;
        result.into_iter().all(|unlocked| unlocked)
    }

    // Error handling and retrying for a locking operation (lock/extend).
    async fn exec_or_retry<F, Fut>(&mut self, ttl: Duration, cb: F) -> Result<bool, RedisLockErr>
    where
        F: Fn(RedisConn<'a>) -> Fut,
        Fut: Future<Output = bool>,
    {
        let ttl = ttl.as_millis() as usize;

        let attempt_beginning = Instant::now();
        let wait_up_to = self.wait_up_to.unwrap_or(Duration::from_secs(0));
        let mut first_run = true;
        while first_run || wait_up_to > attempt_beginning.elapsed() {
            first_run = false;

            let start_time = Instant::now();
            let conns = self.redis.get_conn_to_each_server();
            // Quorum is defined to be N/2+1, with N being the number of given Redis instances.
            let quorum = (conns.len() as u32) / 2 + 1;

            let n = join_all(conns.into_iter().map(&cb))
                .await
                .into_iter()
                .fold(0, |count, locked| if locked { count + 1 } else { count });

            let drift = (ttl as f32 * CLOCK_DRIFT_FACTOR) as usize + 2;
            let elapsed = start_time.elapsed();
            let elapsed_ms =
                elapsed.as_secs() as usize * 1000 + elapsed.subsec_nanos() as usize / 1_000_000;
            if ttl <= drift + elapsed_ms {
                return Err(err!(RedisLockErr::Unavailable).attach_printable(format!(
                    "Ttl expired during locking, ttl millis: {}, potential_drift: {}, elapsed_ms: {}. Try increasing the lock's ttl.",
                    ttl, drift, elapsed_ms
                )));
            }
            let validity_time = ttl
                - drift
                - elapsed.as_secs() as usize * 1000
                - elapsed.subsec_nanos() as usize / 1_000_000;

            // If met the quorum and ttl still holds, succeed, otherwise just unlock.
            if n >= quorum && validity_time > 0 {
                return Ok(true);
            } else {
                self.unlock().await;
            }

            let n = thread_rng().gen_range(0..RETRY_DELAY);
            Delay::new(Duration::from_millis(n as u64)).await;
        }

        Err(err!(RedisLockErr::Unavailable)).attach_printable(format!(
            "Lock, unavailable, {}",
            if let Some(wait_up_to) = self.wait_up_to {
                format!("waited for: {:?}.", wait_up_to)
            } else {
                "user configured to not wait all.".to_string()
            }
        ))
    }
}

/// Get 20 random bytes from the pseudorandom interface.
fn get_unique_lock_id() -> Vec<u8> {
    let mut buf = [0u8; 20];
    thread_rng().fill_bytes(&mut buf);
    buf.to_vec()
}

/// Run by the main tester that spawns up a redis process.
#[cfg(test)]
pub async fn redis_dlock_tests(r: &super::Redis) -> Result<(), AnyErr> {
    // Just checking the object is normal: (from upstream)
    fn is_normal<T: Sized + Send + Sync + Unpin>() {}
    is_normal::<RedisLock>();

    assert_eq!(get_unique_lock_id().len(), 20);
    let id1 = get_unique_lock_id();
    let id2 = get_unique_lock_id();
    assert_eq!(20, id1.len());
    assert_eq!(20, id2.len());
    assert_ne!(id1, id2);

    macro_rules! check_lockable {
        ($name:expr) => {{
            let mut lock = r
                .dlock($name, Duration::from_secs(1), None)
                .await
                .change_context(AnyErr)?;
            lock.unlock().await;
        }};
    }

    macro_rules! check_not_lockable {
        ($name:expr) => {{
            if (r.dlock($name, Duration::from_secs(1), None).await).is_ok() {
                return Err(anyerr!("Lock acquired, even though it should be locked"));
            }
        }};
    }

    // Manual unlock should work:
    let mut lock = r
        .dlock("test_lock_lock_unlock", Duration::from_secs(1), None)
        .await
        .change_context(AnyErr)?;
    // Should fail as instantly locked:
    check_not_lockable!("test_lock_lock_unlock");
    check_not_lockable!("test_lock_lock_unlock"); // Purposely checking twice
    tokio::time::sleep(Duration::from_millis(30)).await;
    // Should still be locked after 30ms: (ttl is 1s)
    check_not_lockable!("test_lock_lock_unlock");
    // Manual unlock should instantly allow relocking:
    lock.unlock().await;
    check_lockable!("test_lock_lock_unlock");

    // Make lock live for 100ms, after 50ms should fail, after 110ms should succeed with no manual unlock:
    let _ = r
        .dlock("test_lock_autoexpire", Duration::from_millis(100), None)
        .await
        .change_context(AnyErr)?;
    // 50ms shouldn't be enough to unlock:
    tokio::time::sleep(Duration::from_millis(50)).await;
    check_not_lockable!("test_lock_autoexpire");
    // another 50msms should be enough to unlock:
    tokio::time::sleep(Duration::from_millis(60)).await;
    check_lockable!("test_lock_autoexpire");

    // New test, confirm extend does extend by expected amount:
    let mut lock = r
        .dlock("test_lock_extend", Duration::from_millis(100), None)
        .await
        .change_context(AnyErr)?;
    tokio::time::sleep(Duration::from_millis(50)).await;
    // This means should be valid for another 100ms:
    lock.extend(Duration::from_millis(100))
        .await
        .change_context(AnyErr)?;
    // Sleep for 60, would have expired original, but new will still be valid for another 40:
    tokio::time::sleep(Duration::from_millis(60)).await;
    check_not_lockable!("test_lock_extend");
    // Should now go over extension, should be relockable:
    tokio::time::sleep(Duration::from_millis(50)).await;
    check_lockable!("test_lock_extend");

    // Confirm retries would work to wait for a lock:
    let _ = r
        .dlock("test_lock_retry", Duration::from_millis(300), None)
        .await
        .change_context(AnyErr)?;
    // This will fail as no wait:
    check_not_lockable!("test_lock_retry");
    // This will fail as only waiting 100ms:
    if r.dlock(
        "test_lock_retry",
        Duration::from_millis(100),
        Some(Duration::from_millis(100)),
    )
    .await
    .is_ok()
    {
        return Err(anyerr!("Lock acquired, even though it should be locked"));
    }
    // This will succeed as waiting for another 250ms, which should easily hit the 300ms ttl:
    r.dlock(
        "test_lock_retry",
        Duration::from_millis(100),
        Some(Duration::from_millis(250)),
    )
    .await
    .change_context(AnyErr)?;

    Ok(())
}
