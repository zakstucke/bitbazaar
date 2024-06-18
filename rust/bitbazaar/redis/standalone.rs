use std::time::Instant;

use crate::log::record_exception;
use crate::prelude::*;

use super::Redis;

/// Standalone redis client, using a unique free port.
/// Useful for testing.
pub struct RedisStandalone {
    /// The port the redis server is running on.
    pub port: u16,
    child: std::process::Child,
}

impl RedisStandalone {
    /// Start a standalone redis server process on an unused port.
    /// This process will be killed on drop.
    pub async fn new() -> RResult<Self, AnyErr> {
        let port = portpicker::pick_unused_port()
            .ok_or_else(|| anyerr!("Could not find a free port to run RedisStandalone on."))?;

        let child = std::process::Command::new("redis-server")
            .arg("--port")
            .arg(port.to_string())
            .spawn()
            .change_context(AnyErr)?;

        // Wait for redis to come up, raising if waited for 10 seconds.
        let mut up = false;
        let elapsed = Instant::now();
        while !up && elapsed.elapsed() < std::time::Duration::from_secs(10) {
            up = Redis::new(
                format!("redis://localhost:{}", port),
                uuid::Uuid::new_v4().to_string(),
            )?
            .conn()
            .ping()
            .await
        }
        if up {
            Ok(Self { child, port })
        } else {
            Err(anyerr!("RedisStandalone process not ready in 10 seconds."))
        }
    }

    /// Get a new [`Redis`] instance connected to this standalone redis server.
    pub fn instance(&self) -> RResult<Redis, AnyErr> {
        Redis::new(
            format!("redis://localhost:{}", self.port),
            uuid::Uuid::new_v4().to_string(),
        )
    }
}

impl Drop for RedisStandalone {
    fn drop(&mut self) {
        match self.child.kill() {
            Ok(_) => {}
            Err(e) => record_exception("Could not kill child process.", format!("{:?}", e)),
        }
    }
}
