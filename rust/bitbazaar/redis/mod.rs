mod batch;
mod conn;
mod dlock;
mod fuzzy;
mod json;
mod pubsub;
mod redis_retry;
mod script;
mod temp_list;
mod wrapper;

mod standalone_redis;

pub use standalone_redis::*;

pub use batch::{RedisBatch, RedisBatchFire, RedisBatchReturningOps};
pub use conn::{RedisConn, RedisConnLike};
pub use dlock::{RedisLock, RedisLockErr};
pub use json::{RedisJson, RedisJsonBorrowed};
pub use pubsub::RedisChannelListener;
// Re-exporting redis and deadpool_redis to be used outside if needed:
pub use deadpool_redis;
pub use redis;
pub use script::{RedisScript, RedisScriptInvoker};
pub use temp_list::{
    RedisTempList, RedisTempListChangeEvent, RedisTempListItem, RedisTempListItemWithConn,
};
pub use wrapper::Redis;

// Redis server can't be run on windows:
#[cfg(not(target_os = "windows"))]
#[cfg(test)]
mod tests {
    use std::{
        collections::HashMap,
        sync::{atomic::AtomicU8, Arc},
    };

    use chrono::{TimeDelta, Utc};

    use super::*;
    use crate::{errors::prelude::*, test::prelude::*};

    #[derive(PartialEq, Debug, serde::Serialize, serde::Deserialize)]
    struct ExampleJson {
        ree: String,
    }

    async fn setup_conns() -> RResult<(RedisStandalone, Redis, Redis), AnyErr> {
        let server = RedisStandalone::new_no_persistence().await?;
        let work_r = Redis::new(server.client_conn_str(), uuid::Uuid::new_v4())?;
        // Also create a fake version on a random port, this will be used to check failure cases.
        let fail_r = Redis::new(
            "redis://FAKKEEEE:6372",
            format!("test_{}", uuid::Uuid::new_v4()),
        )?;
        Ok((server, work_r, fail_r))
    }

    #[rstest]
    #[tokio::test]
    async fn test_redis_ping(#[allow(unused_variables)] logging: ()) -> RResult<(), AnyErr> {
        let (_server, work_r, fail_r) = setup_conns().await?;
        let work_conn = work_r.conn();
        let fail_conn = fail_r.conn();

        assert!(work_conn.ping().await);
        assert!(!(fail_conn.ping().await));

        Ok(())
    }

    #[rstest]
    #[tokio::test]
    async fn test_redis_scripts(#[allow(unused_variables)] logging: ()) -> RResult<(), AnyErr> {
        let (_server, work_r, fail_r) = setup_conns().await?;
        let mut work_conn = work_r.conn();
        let mut fail_conn = fail_r.conn();

        // Confirm simple true/false scripts works for both the fuzzy and hard versions:
        // (this was a bug where the falsey value failed in fuzzy version)
        for expected in [true, false] {
            let script = RedisScript::new(&format!("return {};", expected));
            assert_eq!(
                work_conn
                    .batch()
                    .script_no_decode_protection::<bool>(script.invoker())
                    .fire()
                    .await,
                Some(expected)
            );
            assert_eq!(
                work_conn.batch().script(script.invoker()).fire().await,
                Some(Some(expected))
            );
        }

        let add_script = RedisScript::new(
            r#"
                return tonumber(ARGV[1]) + tonumber(ARGV[2]);
            "#,
        );

        for (conn, exp) in [(&mut work_conn, Some(3)), (&mut fail_conn, None)] {
            assert_eq!(
                conn.batch()
                    .script(add_script.invoker().arg(1).arg(2))
                    .fire()
                    .await
                    .flatten(),
                exp
            );
        }
        assert_eq!(
            work_conn
                .batch()
                .script(add_script.invoker().arg(1).arg(2))
                .script(add_script.invoker().arg(2).arg(5))
                .script(add_script.invoker().arg(9).arg(1))
                .fire()
                .await,
            Some((Some(3), Some(7), Some(10)))
        );

        // Make sure script_no_decode_protection ones work too (not wrapped in fuzzy):
        assert_eq!(
            work_conn
                .batch()
                .script_no_decode_protection::<i64>(add_script.invoker().arg(1).arg(2))
                .script(add_script.invoker().arg(2).arg(5))
                .script_no_decode_protection::<i64>(add_script.invoker().arg(9).arg(1))
                .fire()
                .await,
            Some((3, Some(7), 10))
        );

        Ok(())
    }

    /// Test functionality working as it should when redis up and running fine.
    #[rstest]
    #[tokio::test]
    async fn test_redis_misc(#[allow(unused_variables)] logging: ()) -> RResult<(), AnyErr> {
        let (_server, work_r, fail_r) = setup_conns().await?;
        let mut work_conn = work_r.conn();
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
            (
                &mut work_conn,
                Some((vec![None, Some("bar".to_string())], vec![None])),
            ),
            (&mut fail_conn, None),
        ] {
            assert_eq!(
                conn.batch()
                    .mget("", vec!["I don't exist", "foo"])
                    // Single nonexistent can sometimes cause problems:
                    .mget::<String>("", vec!["nonexistent"])
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
            (
                &mut work_conn,
                Some((true, false, vec![], vec![true, true, false])),
            ),
            (&mut fail_conn, None),
        ] {
            assert_eq!(
                conn.batch()
                    .exists("", "bar")
                    .exists("", "madup")
                    .mexists("", std::iter::empty::<String>())
                    .mexists("", vec!["bar", "foo", "madup"])
                    .fire()
                    .await,
                exp
            );
        }

        // <--- sadd/srem/smembers/smismember:
        for (conn, exp) in [
            (
                &mut work_conn,
                Some((
                    vec![],
                    vec![false, false],
                    vec!["foo".to_string(), "baz".to_string(), "bash".to_string()],
                    vec![true, false, true],
                )),
            ),
            (&mut fail_conn, None),
        ] {
            assert_eq!(
                conn.batch()
                    .smismember("", "myset", std::iter::empty::<String>())
                    .smismember("", "myset", ["foo", "bar"])
                    .sadd(
                        "",
                        "myset",
                        None,
                        vec![
                            "foo".to_string(),
                            "bar".to_string(),
                            "baz".to_string(),
                            "raz".to_string()
                        ]
                    )
                    .sadd("", "myset", None, vec!["bash".to_string()])
                    .srem("", "myset", vec!["bar".to_string(), "raz".to_string()])
                    .smembers("", "myset")
                    .smismember("", "myset", ["foo", "fake", "bash"])
                    .fire()
                    .await,
                exp
            );
        }

        // <--- hset/hmget:
        let now = Utc::now();
        for (conn, exp) in [
            (
                &mut work_conn,
                Some((
                    vec![],
                    vec![Some("foo".to_string()), Some("bar".to_string()), None],
                    vec![Some(0)],
                    vec![Some(RedisJson(now))],
                    vec![None::<String>],
                )),
            ),
            (&mut fail_conn, None),
        ] {
            assert_eq!(
                conn.batch()
                    .hset("", "myhash", None, [("foo", "foo"), ("bar", "bar")])
                    .hset("", "myhash2", None, std::iter::empty::<(String, String)>())
                    .hset("", "myhash2", None, [("ree", 0u64)])
                    .hset("", "myhash2", None, [("roo", RedisJson(now))])
                    .hmget::<String>("", "myhash", std::iter::empty::<String>())
                    .hmget("", "myhash", ["foo", "bar", "baz"])
                    .hmget("", "myhash2", ["ree"])
                    .hmget("", "myhash2", ["roo"])
                    // Single nonexistent can sometimes cause problems:
                    .hmget("", "myhash2", ["nonexistent"])
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
                        RedisJson(ExampleJson {
                            ree: "roo".to_string()
                        }),
                        None
                    )
                    .get::<RedisJson<ExampleJson>>("", "foo")
                    .fire()
                    .await
                    .flatten()
                    .map(|r| r.0),
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
                        Ok::<_, Report<AnyErr>>(ExampleJson {
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
                            Ok::<_, Report<AnyErr>>(ExampleJson {
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

        Ok(())
    }
    #[rstest]
    #[tokio::test]
    async fn test_redis_strange_value_handling(
        #[allow(unused_variables)] logging: (),
    ) -> RResult<(), AnyErr> {
        let server = RedisStandalone::new_no_persistence().await?;
        let r = Redis::new(server.client_conn_str(), uuid::Uuid::new_v4())?;
        let rconn = r.conn();

        rconn
            .batch()
            .set("n1", "none", None::<String>, None)
            .set("n1", "empty_vec", Vec::<String>::new(), None)
            .set("n1", "empty_str", "", None)
            .set("n1", "false", false, None)
            .fire()
            .await;

        // Check both get and mget as they're decoded slightly differently.
        assert_eq!(
            rconn
                .batch()
                .get::<Option<String>>("n1", "none")
                .get::<Vec<String>>("n1", "empty_vec")
                .get::<String>("n1", "empty_str")
                .get::<bool>("n1", "false")
                .fire()
                .await,
            Some((Some(None), Some(vec![]), Some("".to_string()), Some(false)))
        );
        assert_eq!(
            rconn
                .batch()
                .mget::<Option<String>>("n1", vec!["none"])
                .mget::<Vec<String>>("n1", vec!["empty_vec"])
                .mget::<String>("n1", vec!["empty_str"])
                .mget::<bool>("n1", vec!["false"])
                .fire()
                .await,
            Some((
                // TODO: annoyingly comes out as None instead of Some(None):
                vec![None],
                // TODO: annoyingly comes out as None instead of Some(vec![]):
                vec![None],
                vec![Some("".to_string())],
                vec![Some(false)]
            ))
        );

        // If something's of the incorrect type in a get as part of a batch, it shouldn't break the batch:
        assert_eq!(
            rconn
                .batch()
                .set(
                    "z1",
                    "corrupt1",
                    "bazboo",
                    Some(TimeDelta::milliseconds(30))
                )
                .set("z1", "valid", "str", Some(TimeDelta::milliseconds(30)))
                .set(
                    "z1",
                    "corrupt2",
                    "foobar",
                    Some(TimeDelta::milliseconds(30))
                )
                .get::<Vec<HashMap::<String, String>>>("z1", "corrupt1")
                .get::<String>("z1", "valid")
                .get::<RedisJson<ExampleJson>>("z1", "corrupt2")
                .fire()
                .await,
            Some((None, Some("str".to_string()), None))
        );

        // Mget should also be able to handle individual keys being corrupt:
        assert_eq!(
            rconn
                .batch()
                .set("z1", "valid", 1, None)
                .mget::<usize>("z1", vec!["corrupt1", "valid", "corrupt2"])
                .fire()
                .await,
            Some(vec![None, Some(1), None])
        );

        // RedisJson should also be fine with it:
        assert_eq!(
            rconn
                .batch()
                .set(
                    "z1",
                    "valid",
                    RedisJson(ExampleJson {
                        ree: "roo".to_string()
                    }),
                    None
                )
                .set("z1", "corrupt", "foobar", None)
                .get::<RedisJson<ExampleJson>>("z1", "valid")
                .get::<RedisJson<ExampleJson>>("z1", "corrupt")
                .fire()
                .await,
            Some((
                Some(RedisJson(ExampleJson {
                    ree: "roo".to_string()
                })),
                None
            ))
        );

        Ok(())
    }

    #[rstest]
    #[tokio::test]
    async fn test_redis_backoff(#[allow(unused_variables)] logging: ()) -> RResult<(), AnyErr> {
        let server = RedisStandalone::new_no_persistence().await?;
        let r = Redis::new(server.client_conn_str(), uuid::Uuid::new_v4())?;
        let rconn = r.conn();

        macro_rules! call {
            () => {
                rconn
                    .rate_limiter("n1", "caller1", 2, chrono::Duration::milliseconds(100), 1.5)
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
