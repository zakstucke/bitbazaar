mod batch;
mod conn;
mod dlock;
mod json;
mod script;
mod temp_list;
mod wrapper;

mod standalone;

pub use standalone::*;

pub use batch::{RedisBatch, RedisBatchFire, RedisBatchReturningOps};
pub use conn::RedisConn;
pub use dlock::{RedisLock, RedisLockErr};
pub use json::{RedisJson, RedisJsonBorrowed};
// Re-exporting redis to be used outside: (this must also be in scope for the derive macros to work)
pub use redis;
// Re-exporting the json derive utilities to allow redis to take arbitrary json types without the need for the wrapper.
// Both this and the custom wrapper are exported as latter works better for e.g. the temp list.
pub use redis_macros::{FromRedisValue, ToRedisArgs};
pub use script::{RedisScript, RedisScriptInvoker};
pub use temp_list::{RedisTempList, RedisTempListItem, RedisTempListItemWithConn};
pub use wrapper::Redis;

#[cfg(test)]
mod tests {
    use std::sync::{atomic::AtomicU8, Arc};

    use rstest::*;

    use super::*;
    use crate::{
        chrono::chrono_format_td,
        errors::prelude::*,
        log::GlobalLog,
        redis::{dlock::redis_dlock_tests, temp_list::redis_temp_list_tests},
    };

    #[derive(
        PartialEq, Debug, serde::Serialize, serde::Deserialize, FromRedisValue, ToRedisArgs,
    )]
    struct ExampleJson {
        ree: String,
    }

    struct AddScript {
        script: RedisScript,
    }

    impl Default for AddScript {
        fn default() -> Self {
            Self {
                script: RedisScript::new(
                    r#"
                        return tonumber(ARGV[1]) + tonumber(ARGV[2]);
                    "#,
                ),
            }
        }
    }

    impl AddScript {
        pub fn invoke(&self, a: isize, b: isize) -> RedisScriptInvoker<'_> {
            self.script.invoker().arg(a).arg(b)
        }
    }

    #[fixture]
    fn logging() -> () {
        GlobalLog::setup_quick_stdout_global_logging(tracing::Level::DEBUG).unwrap();
    }

    /// Test functionality working as it should when redis up and running fine.
    #[rstest]
    #[tokio::test]
    async fn test_redis_working(#[allow(unused_variables)] logging: ()) -> RResult<(), AnyErr> {
        // Redis can't be run on windows, skip if so:
        if cfg!(windows) {
            return Ok(());
        }

        let rs = RedisStandalone::new().await?;

        let work_r = rs.instance()?;
        let mut work_conn = work_r.conn();

        // Also create a fake version on a random port, this will be used to check failure cases.
        let fail_r = Redis::new(
            "redis://FAKKEEEE:6372",
            format!("test_{}", uuid::Uuid::new_v4()),
        )?;
        let mut fail_conn = fail_r.conn();

        // <--- get/set:

        // Shouldn't exist yet:
        for (conn, exp) in [(&mut work_conn, Some(None)), (&mut fail_conn, None)] {
            assert_eq!(conn.batch().get::<String>("", "foo").fire().await, exp);
        }

        // Set so should now exist:
        work_conn.batch().set("", "foo", "bar", None).fire().await;

        // Should be passed back successfully:
        for (conn, exp) in [
            (&mut work_conn, Some(Some("bar".to_string()))),
            (&mut fail_conn, None),
        ] {
            assert_eq!(conn.batch().get::<String>("", "foo").fire().await, exp);
        }

        // Multiple should come back as tuple:
        for (conn, exp) in [
            (&mut work_conn, Some((None, Some("bar".to_string())))),
            (&mut fail_conn, None),
        ] {
            assert_eq!(
                conn.batch()
                    .get::<String>("", "I don't exist")
                    .get("", "foo")
                    .fire()
                    .await,
                exp
            );
        }

        // <--- mget/mset:

        // First should fail as not set, second should succeed:
        for (conn, exp) in [
            (&mut work_conn, Some(vec![None, Some("bar".to_string())])),
            (&mut fail_conn, None),
        ] {
            assert_eq!(
                conn.batch()
                    .mget("", vec!["I don't exist", "foo"])
                    .fire()
                    .await,
                exp
            );
        }

        // Set first and update foo together:
        for (conn, exp) in [
            (
                &mut work_conn,
                Some(vec![Some("a".to_string()), Some("b".to_string())]),
            ),
            (&mut fail_conn, None),
        ] {
            assert_eq!(
                conn.batch()
                    .mset("", vec![("bar", "a"), ("foo", "b")], None)
                    .mget("", vec!["bar", "foo"])
                    .fire()
                    .await,
                exp
            );
        }

        // <--- exists/mexists:
        for (conn, exp) in [
            (&mut work_conn, Some((true, false, vec![true, true, false]))),
            (&mut fail_conn, None),
        ] {
            assert_eq!(
                conn.batch()
                    .exists("", "bar")
                    .exists("", "madup")
                    .mexists("", vec!["bar", "foo", "madup"])
                    .fire()
                    .await,
                exp
            );
        }

        // <--- clear/clear_namespace:
        for (conn, exp) in [(&mut work_conn, Some(())), (&mut fail_conn, None)] {
            assert_eq!(
                conn.batch()
                    .clear("", ["bar"])
                    .clear("", ["madup"])
                    .clear_namespace("")
                    .fire()
                    .await,
                exp
            );
        }
        assert_eq!(
            work_conn
                .batch()
                .mset("n1", [("foo", "foo"), ("bar", "bar"), ("baz", "baz")], None)
                .mset("n2", [("foo", "foo"), ("bar", "bar"), ("baz", "baz")], None)
                .mset("n3", [("foo", "foo"), ("bar", "bar"), ("baz", "baz")], None)
                .clear_namespace("n1")
                .clear("n2", ["foo", "baz"])
                .mget::<String>("n1", ["foo", "bar", "baz"])
                .mget("n2", ["foo", "bar", "baz"])
                .mget("n3", ["foo", "bar", "baz"])
                .fire()
                .await,
            Some((
                // n1 should have been completely cleared
                vec![None, None, None],
                // n2 should have had foo and baz cleared
                vec![None, Some("bar".to_string()), None],
                // n3 should've been untouched by it all
                vec![
                    Some("foo".to_string()),
                    Some("bar".to_string()),
                    Some("baz".to_string())
                ]
            ))
        );

        // <--- Scripts:
        let script = AddScript::default();

        for (conn, exp) in [(&mut work_conn, Some(3)), (&mut fail_conn, None)] {
            assert_eq!(
                conn.batch()
                    .script(script.invoke(1, 2))
                    .fire()
                    .await
                    .flatten(),
                exp
            );
        }
        assert_eq!(
            work_conn
                .batch()
                .script(script.invoke(1, 2))
                .script(script.invoke(2, 5))
                .script(script.invoke(9, 1))
                .fire()
                .await,
            Some((Some(3), Some(7), Some(10)))
        );

        // <--- Json:
        for (conn, exp) in [
            (
                &mut work_conn,
                Some(ExampleJson {
                    ree: "roo".to_string(),
                }),
            ),
            (&mut fail_conn, None),
        ] {
            assert_eq!(
                conn.batch()
                    .set(
                        "",
                        "foo",
                        ExampleJson {
                            ree: "roo".to_string()
                        },
                        None
                    )
                    .get::<ExampleJson>("", "foo")
                    .fire()
                    .await
                    .flatten(),
                exp
            );
        }

        // <--- Cached function:
        for (conn, expected_call_times) in [
            (&mut work_conn, 1),
            // When redis fails, should have to keep computing the function, so should be called 5 times:
            (&mut fail_conn, 5),
        ] {
            let called = Arc::new(AtomicU8::new(0));
            for _ in 0..5 {
                assert_eq!(
                    conn.cached_fn("my_fn_group", "foo", None, || async {
                        // Add one to the call count, should only be called once:
                        called.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                        Ok(ExampleJson {
                            ree: "roo".to_string(),
                        })
                    })
                    .await?,
                    ExampleJson {
                        ree: "roo".to_string(),
                    },
                );
            }
            assert_eq!(
                called.load(std::sync::atomic::Ordering::SeqCst),
                expected_call_times
            );
        }

        // <--- Cached function with expiry:
        let called = Arc::new(AtomicU8::new(0));
        for _ in 0..5 {
            assert_eq!(
                work_conn
                    .cached_fn(
                        "my_fn_ex_group",
                        "foo",
                        Some(chrono::Duration::milliseconds(15)),
                        || async {
                            // Add one to the call count, should only be called once:
                            called.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                            Ok(ExampleJson {
                                ree: "roo".to_string(),
                            })
                        }
                    )
                    .await?,
                ExampleJson {
                    ree: "roo".to_string(),
                },
            );
            // Sleep for 5 milliseconds:
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        }
        // The 3rd/4th call will have needed a reload due to the expiry, so should've been called twice:
        assert_eq!(called.load(std::sync::atomic::Ordering::SeqCst), 2);

        // <--- set/mset with expiry:
        work_conn
            .batch()
            .set("e1", "foo", "foo", Some(chrono::Duration::milliseconds(15)))
            .set("e1", "bar", "bar", Some(chrono::Duration::milliseconds(30)))
            .mset(
                "e2",
                [("foo", "foo"), ("bar", "bar")],
                Some(chrono::Duration::milliseconds(15)),
            )
            .mset(
                "e2",
                [("baz", "baz"), ("qux", "qux")],
                Some(chrono::Duration::milliseconds(30)),
            )
            .fire()
            .await;

        // Sleep for 15 milliseconds to let the expiry happen:
        tokio::time::sleep(std::time::Duration::from_millis(15)).await;

        assert_eq!(
            work_conn
                .batch()
                .get::<String>("e1", "foo")
                .get("e1", "bar")
                .mget("e2", ["foo", "bar", "baz", "qux"])
                .fire()
                .await,
            Some((
                // e1: Foo should've expired after 15ms:
                None,
                // e1: Bar should still be there, as it was set to expire after 30ms:
                Some("bar".to_string()),
                // e2: baz and qui should still be there (30), foo and bar should be gone (15):
                vec![None, None, Some("baz".to_string()), Some("qux".to_string())]
            ))
        );

        // Sleep for another 15 milliseconds, the remaining should expire:
        tokio::time::sleep(std::time::Duration::from_millis(15)).await;

        assert_eq!(
            work_conn
                .batch()
                .get::<String>("e1", "foo")
                .get::<String>("e1", "bar")
                .mget::<Vec<String>>("e2", ["foo", "bar", "baz", "qux"])
                .fire()
                .await,
            Some((None, None, vec![None, None, None, None]))
        );

        // zadd/zaddmulti/zrem/zrangebyscore/zremrangebyscore
        assert_eq!(
            work_conn
                .batch()
                .zadd("z1", "myset", None, 3, "foo")
                // By setting an expiry time here, the set itself will now expire after 30ms:
                .zadd(
                    "z1",
                    "myset",
                    Some(chrono::Duration::milliseconds(30)),
                    1,
                    "bar"
                )
                .zadd_multi(
                    "z1",
                    "myset",
                    None,
                    &[(2, "baz"), (5, "qux"), (6, "lah"), (0, "loo"), (4, "quux"),],
                )
                .zrem("z1", "myset", std::iter::once("foo"))
                // Should return vals for scores in 2,3,4 (not 3 because I just removed it)
                .zrangebyscore_high_to_low::<String>("z1", "myset", 2, 4, None)
                // Delete vals with scores in 2,3,4
                .zremrangebyscore("z1", "myset", 2, 4)
                // Means 0-6 now returns only 6,5,1 (not 0 due to limit of 3) as just deleted the rest.
                .zrangebyscore_high_to_low::<String>("z1", "myset", 0, 6, Some(3))
                // Means 0-6 now returns only 0,1,5 (not 6 due to limit of 3) as just deleted the rest.
                .zrangebyscore_low_to_high::<String>("z1", "myset", 0, 6, Some(3))
                .fire()
                .await,
            // Values are ordered from highest score to lowest score:
            Some((
                vec![(Some("quux".into()), 4), (Some("baz".into()), 2),],
                vec![
                    // High to low version
                    (Some("lah".into()), 6),
                    (Some("qux".into()), 5),
                    (Some("bar".into()), 1),
                ],
                vec![
                    // Low to high version
                    (Some("loo".into()), 0),
                    (Some("bar".into()), 1),
                    (Some("qux".into()), 5),
                ],
            ))
        );

        // In 15ms should still exist, in another 20ms should be gone due to the set expire time:
        tokio::time::sleep(std::time::Duration::from_millis(15)).await;
        assert_eq!(
            work_conn.batch().exists("z1", "myset").fire().await,
            Some(true)
        );
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        assert_eq!(
            work_conn.batch().exists("z1", "myset").fire().await,
            Some(false)
        );

        // TODO this isn't possible with redis right now, but looks like its coming very soon.
        // Once this is working, also do test for mget, where some in the mget fail, and also do for the temp list, in case some don't match T they should be ignored but not stop others being pulled.
        // See:
        // https://github.com/redis-rs/redis-rs/issues/746
        // https://github.com/redis-rs/redis-rs/pull/1093
        // https://github.com/redis-rs/redis-rs/pull/1144
        // https://github.com/redis-rs/redis-rs/issues/540
        // https://github.com/redis-rs/redis-rs/pull/813
        // https://github.com/redis-rs/redis-rs/issues/1043

        // // If something's of the incorrect type in a get as part of a batch, it shouldn't break the batch (historic bug the batch would fail and the outer would be none)
        // assert_eq!(
        //     work_conn
        //         .batch()
        //         .set("z1", "corrupt1", "bazboo", Some(Duration::from_millis(30)))
        //         .set("z1", "valid", "str", Some(Duration::from_millis(30)))
        //         .set("z1", "corrupt2", "foobar", Some(Duration::from_millis(30)))
        //         .get::<Vec<HashMap<String, String>>>("z1", "corrupt1")
        //         .get::<String>("z1", "valid")
        //         .get::<RedisJson<ExampleJson>>("z1", "corrupt2")
        //         .fire()
        //         .await,
        //     Some((None, Some("str".to_string()), None))
        // );

        // Run the dlock tests:
        redis_dlock_tests(&work_r).await?;

        // Run the temp_list tests:
        redis_temp_list_tests(&work_r).await?;

        Ok(())
    }

    #[rstest]
    #[tokio::test]
    async fn test_redis_backoff(#[allow(unused_variables)] logging: ()) -> RResult<(), AnyErr> {
        // Redis can't be run on windows, skip if so:
        if cfg!(windows) {
            return Ok(());
        }

        // TODO using now in here and dlock, should be some test utils we can use cross crate.
        macro_rules! assert_td_in_range {
            ($td:expr, $range:expr) => {
                assert!(
                    $td >= $range.start && $td <= $range.end,
                    "Expected '{}' to be in range '{}' - '{}'.",
                    chrono_format_td($td, true),
                    chrono_format_td($range.start, true),
                    chrono_format_td($range.end, true),
                );
            };
        }

        let rs = RedisStandalone::new().await?;

        let r = rs.instance()?;
        let mut rconn = r.conn();

        macro_rules! call {
            () => {
                rconn
                    .backoff_protector("n1", "caller1", 2, chrono::Duration::milliseconds(100), 1.5)
                    .await
            };
        }
        assert_eq!(call!(), None);
        assert_eq!(call!(), None);
        assert_td_in_range!(
            call!().unwrap(),
            chrono::Duration::milliseconds(90)..chrono::Duration::milliseconds(100)
        );
        // Wait to allow call again, but will x1.5 wait for next time:
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        assert_eq!(call!(), None);
        assert_td_in_range!(
            call!().unwrap(),
            chrono::Duration::milliseconds(130)..chrono::Duration::milliseconds(150)
        );
        // Just check double call too:
        assert_td_in_range!(
            call!().unwrap(),
            chrono::Duration::milliseconds(125)..chrono::Duration::milliseconds(150)
        );
        tokio::time::sleep(std::time::Duration::from_millis(150)).await;
        assert_eq!(call!(), None);
        // Should now 1.5x again:
        assert_td_in_range!(
            call!().unwrap(),
            chrono::Duration::milliseconds(190)..chrono::Duration::milliseconds(225)
        );
        // By waiting over 2x the current delay, should all reset:
        tokio::time::sleep(std::time::Duration::from_millis(450)).await;
        assert_eq!(call!(), None);
        assert_eq!(call!(), None);
        assert_td_in_range!(
            call!().unwrap(),
            chrono::Duration::milliseconds(90)..chrono::Duration::milliseconds(100)
        );

        Ok(())
    }
}
