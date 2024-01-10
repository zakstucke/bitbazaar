mod batch;
mod conn;
mod script;
mod wrapper;

pub use batch::RedisBatch;
pub use conn::RedisConn;
pub use script::RedisScript;
pub use wrapper::Redis;

#[cfg(test)]
mod tests {
    use std::process::{Child, Command};

    use portpicker::is_free;
    use rstest::*;

    use super::*;
    use crate::{errors::TracedErr, misc::in_ci};

    struct ChildGuard(Child);

    impl Drop for ChildGuard {
        fn drop(&mut self) {
            match self.0.kill() {
                Err(e) => println!("Could not kill child process: {}", e),
                Ok(_) => println!("Successfully killed child process"),
            }
        }
    }

    /// Make sure redis is running and return the redis wrapper.
    /// If in CI, this will return None meaning skip the test, don't want to install redis in CI.
    #[fixture]
    async fn rwrapper() -> (Option<Redis>, Option<ChildGuard>) {
        // Don't want to install redis in ci, just run this test locally:
        if in_ci() {
            return (None, None);
        }

        // Make sure redis is running on port 6379, starting it otherwise. (this means you must have redis installed)
        let mut _redis_guard: Option<ChildGuard> = None;
        if is_free(6379) {
            _redis_guard = Some(ChildGuard(
                Command::new("redis-server")
                    .arg("--port")
                    .arg("6379")
                    .spawn()
                    .unwrap(),
            ));
            // sleep for 50ms to give redis time to start:
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        }

        (
            Some(Redis::new("redis://localhost:6379", uuid::Uuid::new_v4().to_string()).unwrap()),
            _redis_guard,
        )
    }

    #[rstest]
    #[tokio::test]
    async fn test_redis(
        #[future] rwrapper: (Option<Redis>, Option<ChildGuard>),
    ) -> Result<(), TracedErr> {
        if let Some(r) = rwrapper.await.0 {
            let mut conn = r.conn();

            // Shouldn't exist yet:
            assert_eq!(conn.batch().get::<String, _>("foo").fire().await?, None);

            // Set so should now exist:
            conn.batch().set("foo", "bar").fire().await?;

            // Should be passed back successfully:
            assert_eq!(
                conn.batch().get("foo").fire().await?,
                Some("bar".to_string())
            );

            // Multiple should come back as tuple:
            assert_eq!(
                conn.batch()
                    .get::<String, _>("I don't exist")
                    .get("foo")
                    .fire()
                    .await?,
                (None, Some("bar".to_string()))
            );

            // Mget, first should fail as not set, second should succeed:
            assert_eq!(
                conn.batch()
                    .mget(vec!["I don't exist", "foo"])
                    .fire()
                    .await?,
                vec![None, Some("bar".to_string())]
            );

            // <--- Scripts:
            let script = RedisScript::new(
                r#"
                return tonumber(ARGV[1]) + tonumber(ARGV[2]);
            "#,
            );

            assert_eq!(
                script
                    .run::<usize>(&mut conn, |scr| {
                        scr.arg(1).arg(2);
                    })
                    .await
                    .unwrap(),
                3
            );
        }
        Ok(())
    }
}
