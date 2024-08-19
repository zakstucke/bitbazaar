mod channel_listener;
pub(crate) mod pubsub_global;

pub use channel_listener::*;

// Redis server can't be run on windows:
#[cfg(not(target_os = "windows"))]
#[cfg(test)]
mod tests {

    use chrono::TimeDelta;

    use crate::misc::with_timeout;
    use crate::redis::{Redis, RedisBatchFire, RedisConnLike, RedisStandalone};
    use crate::testing::prelude::*;

    async fn setup_conns() -> RResult<(RedisStandalone, Redis, Redis), AnyErr> {
        let server = RedisStandalone::new_no_persistence().await?;
        let work_r = Redis::new(server.client_conn_str(), uuid::Uuid::new_v4())?;
        // Also create a fake version on a random port, this will be used to check failure cases.
        let fail_r = Redis::new(
            "redis://FAKE:6372",
            format!("test_{}", uuid::Uuid::new_v4()),
        )?;
        Ok((server, work_r, fail_r))
    }

    // The basics:
    // - Listeners receive messages.
    // - Listeners receive only their own messages.
    // - Listeners clean themselves up.
    #[rstest]
    #[tokio::test]
    async fn test_redis_pubsub_simple(
        #[allow(unused_variables)] logging: (),
    ) -> RResult<(), AnyErr> {
        let (_server, work_r, _fail_r) = setup_conns().await?;
        let work_conn = work_r.conn();

        for (mut rx, namespace) in [
            (
                work_conn.subscribe::<String>("n1", "foo").await.unwrap(),
                "n1",
            ),
            (
                work_conn.subscribe::<String>("n2", "foo").await.unwrap(),
                "n2",
            ),
        ] {
            assert!(work_conn
                .batch()
                .publish(namespace, "foo", format!("{}_first_msg", namespace))
                .publish(namespace, "foo", format!("{}_second_msg", namespace))
                .fire()
                .await
                .is_some());
            with_timeout(
                TimeDelta::seconds(3),
                || {
                    panic!("Timeout waiting for pubsub message");
                },
                async move {
                    assert_eq!(Some(format!("{}_first_msg", namespace)), rx.recv().await);
                    assert_eq!(Some(format!("{}_second_msg", namespace)), rx.recv().await);
                    with_timeout(
                        TimeDelta::milliseconds(100),
                        || Ok::<_, Report<AnyErr>>(()),
                        async {
                            let msg = rx.recv().await;
                            panic!("Shouldn't have received any more messages, got: {:?}", msg);
                        },
                    )
                    .await?;
                    Ok::<_, Report<AnyErr>>(())
                },
            )
            .await?;
        }

        // Given everything's dropped now we're out of the loop, internals should've been cleaned up after a short delay:
        tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;
        assert_eq!(work_r.pubsub_listener.listeners.len(), 0);

        Ok(())
    }

    // The redis prefix should be respected for channels
    #[rstest]
    #[tokio::test]
    async fn test_redis_pubsub_prefix_respected(
        #[allow(unused_variables)] logging: (),
    ) -> RResult<(), AnyErr> {
        let server = RedisStandalone::new_no_persistence().await?;
        let work_r_1 = Redis::new(server.client_conn_str(), uuid::Uuid::new_v4())?;
        let work_r_2 = Redis::new(server.client_conn_str(), uuid::Uuid::new_v4())?;
        let work_conn_1 = work_r_1.conn();
        let work_conn_2 = work_r_2.conn();

        // Given we're using 2 different prefixes, each should not be impacted by the other:
        let mut rx1 = work_conn_1.subscribe::<String>("", "foo").await.unwrap();
        let mut rx2 = work_conn_2.subscribe::<String>("", "foo").await.unwrap();

        assert!(work_conn_1
            .batch()
            .publish("", "foo", "conn_1_msg")
            .fire()
            .await
            .is_some());
        assert!(work_conn_2
            .batch()
            .publish("", "foo", "conn_2_msg")
            .fire()
            .await
            .is_some());

        with_timeout(
            TimeDelta::seconds(3),
            || {
                panic!("Timeout waiting for pubsub message");
            },
            async move {
                assert_eq!(Some("conn_1_msg".to_string()), rx1.recv().await);
                with_timeout(
                    TimeDelta::milliseconds(100),
                    || Ok::<_, Report<AnyErr>>(()),
                    async {
                        let msg = rx1.recv().await;
                        panic!("Shouldn't have received any more messages, got: {:?}", msg);
                    },
                )
                .await?;
                assert_eq!(Some("conn_2_msg".to_string()), rx2.recv().await);
                with_timeout(
                    TimeDelta::milliseconds(100),
                    || Ok::<_, Report<AnyErr>>(()),
                    async {
                        let msg = rx2.recv().await;
                        panic!("Shouldn't have received any more messages, got: {:?}", msg);
                    },
                )
                .await?;
                Ok::<_, Report<AnyErr>>(())
            },
        )
        .await?;
        Ok(())
    }

    // Multiple listeners on the same channel:
    // - Each gets data
    // - Each gets data only once
    #[rstest]
    #[tokio::test]
    async fn test_redis_pubsub_single_channel_multiple_listeners(
        #[allow(unused_variables)] logging: (),
    ) -> RResult<(), AnyErr> {
        let (_server, work_r, _fail_r) = setup_conns().await?;
        let work_conn = work_r.conn();

        let rx1 = work_conn.subscribe::<String>("n1", "foo").await.unwrap();
        let rx2 = work_conn.subscribe::<String>("n1", "foo").await.unwrap();
        let rx3 = work_conn.subscribe::<String>("n1", "foo").await.unwrap();

        // All 3 receivers should receive these messages:
        assert!(work_conn
            .batch()
            .publish("n1", "foo", "first_msg")
            .publish("n1", "foo", "second_msg")
            .fire()
            .await
            .is_some());

        for mut rx in [rx1, rx2, rx3] {
            with_timeout(
                TimeDelta::seconds(3),
                || {
                    panic!("Timeout waiting for pubsub message");
                },
                async move {
                    assert_eq!(Some("first_msg".to_string()), rx.recv().await);
                    assert_eq!(Some("second_msg".to_string()), rx.recv().await);
                    with_timeout(
                        TimeDelta::milliseconds(100),
                        || Ok::<_, Report<AnyErr>>(()),
                        async {
                            let msg = rx.recv().await;
                            panic!("Shouldn't have received any more messages, got: {:?}", msg);
                        },
                    )
                    .await?;
                    Ok::<_, Report<AnyErr>>(())
                },
            )
            .await?;
        }

        // Given everything's dropped now we're out of the loop, internals should've been cleaned up after a short delay:
        tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;
        assert_eq!(work_r.pubsub_listener.listeners.len(), 0);

        Ok(())
    }

    /// - pubsub should be able to continue after redis goes down and back up.
    /// - subscribe() and publish() should work even if redis literally just coming back alive.
    /// - subscriptions should automatically resubscribe when the connection has to be restarted on new redis.
    #[rstest]
    #[tokio::test]
    async fn test_redis_pubsub_redis_sketchiness(
        #[allow(unused_variables)] logging: (),
    ) -> RResult<(), AnyErr> {
        // Start a server to get a static port then instantly shutdown but keep the redis instance (client):
        let (client, port) = {
            let server = RedisStandalone::new_no_persistence().await?;
            let client = Redis::new(server.client_conn_str(), uuid::Uuid::new_v4().to_string())?;
            (client, server.port)
        };

        let restart_server = move || {
            // Same as new_no_persistence, but have to use underlying for port:
            RedisStandalone::new_with_opts(port, Some(&["--appendonly", "no", "--save", "\"\""]))
        };

        // subscribe() should work even if redis is justttt coming back up, i.e. it should wait around for a connection.
        let mut rx = {
            let _server = restart_server().await?;
            client
                .conn()
                .subscribe::<String>("n1", "foo")
                .await
                .unwrap()
        };

        // publish() should work even if redis is justttt coming back up, i.e. it should wait around for a connection.
        let _server = restart_server().await?;
        // This is separate, just confirming publish works straight away,
        // slight delay needed for actual publish as redis needs time to resubscribe to the channel on the new connection,
        // otherwise won't see the published event.
        assert!(client
            .conn()
            .batch()
            .publish("lah", "loo", "baz")
            .fire()
            .await
            .is_some());

        // Short delay, see above comment for redis to resubscribe before proper publish to check:
        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
        assert!(client
            .conn()
            .batch()
            .publish("n1", "foo", "first_msg")
            .publish("n1", "foo", "second_msg")
            .fire()
            .await
            .is_some());

        // Despite all the madness messages should come through:
        with_timeout(
            TimeDelta::seconds(3),
            || {
                panic!("Timeout waiting for pubsub message");
            },
            async move {
                assert_eq!(Some("first_msg".to_string()), rx.recv().await);
                assert_eq!(Some("second_msg".to_string()), rx.recv().await);
                with_timeout(
                    TimeDelta::milliseconds(100),
                    || Ok::<_, Report<AnyErr>>(()),
                    async {
                        let msg = rx.recv().await;
                        panic!("Shouldn't have received any more messages, got: {:?}", msg);
                    },
                )
                .await?;
                Ok::<_, Report<AnyErr>>(())
            },
        )
        .await?;

        Ok(())
    }

    // Patterns should work with conn.psubscribe(), confirm patterns match correctly, but don't if pattern passed through normal subscribe.
    #[rstest]
    #[tokio::test]
    async fn test_redis_pubsub_pattern(
        #[allow(unused_variables)] logging: (),
    ) -> RResult<(), AnyErr> {
        let (_server, work_r, _fail_r) = setup_conns().await?;
        let work_conn = work_r.conn();

        let mut rx_normal = work_conn.subscribe::<String>("n1", "f*o").await.unwrap();
        let mut rx_pattern = work_conn.psubscribe::<String>("n1", "f*o").await.unwrap();

        assert!(work_conn
            .batch()
            .publish("n1", "foo", "only_pattern")
            .publish("n1", "f*o", "both")
            .fire()
            .await
            .is_some());
        with_timeout(
            TimeDelta::seconds(3),
            || {
                panic!("Timeout waiting for pubsub message");
            },
            async move {
                assert_eq!(Some("both".to_string()), rx_normal.recv().await);
                with_timeout(
                    TimeDelta::milliseconds(100),
                    || Ok::<_, Report<AnyErr>>(()),
                    async {
                        let msg = rx_normal.recv().await;
                        panic!("Shouldn't have received any more messages, got: {:?}", msg);
                    },
                )
                .await?;
                assert_eq!(Some("only_pattern".to_string()), rx_pattern.recv().await);
                assert_eq!(Some("both".to_string()), rx_pattern.recv().await);
                with_timeout(
                    TimeDelta::milliseconds(100),
                    || Ok::<_, Report<AnyErr>>(()),
                    async {
                        let msg = rx_pattern.recv().await;
                        panic!("Shouldn't have received any more messages, got: {:?}", msg);
                    },
                )
                .await?;
                Ok::<_, Report<AnyErr>>(())
            },
        )
        .await?;

        Ok(())
    }

    // Nothing should break when no ones subscribed to a channel when a message is published.
    #[rstest]
    #[tokio::test]
    async fn test_redis_pubsub_no_listener(
        #[allow(unused_variables)] logging: (),
    ) -> RResult<(), AnyErr> {
        let (_server, work_r, _fail_r) = setup_conns().await?;
        let work_conn = work_r.conn();

        assert!(work_conn
            .batch()
            .publish("n1", "foo", "first_msg")
            .publish("n1", "foo", "second_msg")
            .fire()
            .await
            .is_some());

        Ok(())
    }
}
