use std::time::Instant;

use crate::log::record_exception;
use crate::prelude::*;

use super::{Redis, RedisConnLike};

/// Standalone redis client, using a unique free port.
/// Useful for testing.
pub struct RedisStandalone {
    /// The port the redis server is running on.
    pub port: u16,
    child: std::process::Child,
}

impl RedisStandalone {
    /// Find an unused port to run the standalone redis server on.
    pub fn find_unused_port() -> RResult<u16, AnyErr> {
        portpicker::pick_unused_port()
            .ok_or_else(|| anyerr!("Could not find a free port to run RedisStandalone on."))
    }

    /// Start a standalone redis server process with the given port and extra arguments.
    /// This process will be killed on drop.
    pub async fn new_with_opts(port: u16, extra_args: Option<&[&str]>) -> RResult<Self, AnyErr> {
        let mut cmd = std::process::Command::new("redis-server");
        cmd.arg("--port").arg(port.to_string());
        if let Some(extra_args) = extra_args {
            for arg in extra_args {
                cmd.arg(arg);
            }
        }
        let child = cmd.spawn().change_context(AnyErr)?;

        // Wait for redis to come up, raising if waited for 10 seconds.
        let client = Redis::new(
            format!("redis://localhost:{}", port),
            uuid::Uuid::new_v4().to_string(),
        )?;
        let mut up = false;
        let elapsed = Instant::now();
        while !up && elapsed.elapsed() < std::time::Duration::from_secs(10) {
            // Using low level check of conn first, as inner will record an exception which we don't really need during this startup check:
            if client.get_inner_pool().get().await.is_ok() {
                up = client.conn().ping().await
            }
        }
        // Final ping as that interface conn() will actually log an error on failure to connect:
        if up || client.conn().ping().await {
            Ok(Self { child, port })
        } else {
            Err(anyerr!("RedisStandalone process not ready in 10 seconds."))
        }
    }

    /// Start a standalone redis server process on an unused port.
    /// This process will be killed on drop.
    pub async fn new() -> RResult<Self, AnyErr> {
        RedisStandalone::new_with_opts(Self::find_unused_port()?, None).await
    }

    /// Start a standalone redis server process on an unused port.
    /// This process will be killed on drop.
    ///
    /// Default config contains persistence, this can be an issue during testing, use this instead.
    pub async fn new_no_persistence() -> RResult<Self, AnyErr> {
        RedisStandalone::new_with_opts(
            Self::find_unused_port()?,
            // Turning off both aof and rdb file saving:
            Some(&["--appendonly", "no", "--save", "\"\""]),
        )
        .await
    }

    /// Get the connection string needed to connect as a client to this locally running redis instance.
    pub fn client_conn_str(&self) -> String {
        format!("redis://localhost:{}", self.port)
    }

    /// Kill the server, will be automatically called when dropped.
    pub fn kill(mut self) {
        self.kill_inner()
    }

    fn kill_inner(&mut self) {
        match self.child.kill() {
            Ok(_) => {}
            Err(e) => record_exception("Could not kill child process.", format!("{:?}", e)),
        }
    }
}

impl Drop for RedisStandalone {
    fn drop(&mut self) {
        self.kill_inner()
    }
}
