use std::{
    collections::HashMap,
    fmt::Debug,
    sync::{atomic::AtomicBool, Arc},
};

use chrono::TimeDelta;
use dashmap::DashMap;
use redis::{aio::MultiplexedConnection, from_owned_redis_value, FromRedisValue, ToRedisArgs};

use crate::{
    log::record_exception,
    misc::{random_u64_rolling, Retry},
    prelude::*,
    redis::redis_retry::redis_retry_config,
};

use super::RedisChannelListener;

/// The lazy pubsub manager.
pub struct RedisPubSubGlobal {
    client: redis::Client,
    config: redis::AsyncConnectionConfig,
    /// Unlike deadpool these aren't pooled, so definitely need to store and reuse until it becomes invalid, only then get a new one.
    active_conn: tokio::sync::RwLock<Option<MultiplexedConnection>>,
    /// The downstream configured listeners for different channels, messages will be pushed to all active listeners.
    /// Putting in a nested hashmap for easy cleanup when listeners are dropped.
    pub(crate) listeners:
        DashMap<String, HashMap<u64, tokio::sync::mpsc::UnboundedSender<redis::Value>>>,

    /// The global receiver of messages hooked directly into the redis connection.
    /// This will be taken when the main listener is spawned.
    rx: tokio::sync::Mutex<Option<tokio::sync::mpsc::UnboundedReceiver<redis::PushInfo>>>,
    /// Below used to trigger unsubscriptions and listeners dashmap cleanup when listeners are dropped.
    /// (The tx is called when a listener is dropped, and the spawned process listens for these and does the cleanup.)
    listener_drop_tx: Arc<tokio::sync::mpsc::UnboundedSender<(String, u64)>>,
    listener_drop_rx:
        tokio::sync::Mutex<Option<tokio::sync::mpsc::UnboundedReceiver<(String, u64)>>>,
    spawned: AtomicBool,
}

impl Debug for RedisPubSubGlobal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RedisPubSubGlobal")
            .field("client", &self.client)
            // .field("config", &self.config)
            .field("active_conn", &self.active_conn)
            .field("listeners", &self.listeners)
            .field("rx", &self.rx)
            .field("spawned", &self.spawned)
            .finish()
    }
}

impl RedisPubSubGlobal {
    pub(crate) fn new(redis_conn_str: impl Into<String>) -> RResult<Self, AnyErr> {
        let client = redis::Client::open(format!("{}?protocol=resp3", redis_conn_str.into()))
            .change_context(AnyErr)?;
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        let (listener_drop_tx, listener_drop_rx) = tokio::sync::mpsc::unbounded_channel();
        let config = redis::AsyncConnectionConfig::new().set_push_sender(tx);
        Ok(Self {
            client,
            config,
            active_conn: tokio::sync::RwLock::new(None),
            listeners: DashMap::new(),
            rx: tokio::sync::Mutex::new(Some(rx)),
            listener_drop_tx: Arc::new(listener_drop_tx),
            listener_drop_rx: tokio::sync::Mutex::new(Some(listener_drop_rx)),
            spawned: AtomicBool::new(false),
        })
    }

    pub(crate) async fn unsubscribe(&self, channel: impl Into<String>) {
        let channel = channel.into();
        self.listeners.remove(&channel);

        let force_new_connection = AtomicBool::new(false);
        match redis_retry_config()
            .call(|| async {
                if let Some(mut conn) = self
                    .get_conn(
                        // Means on second attempt onwards, will always get new connections.
                        force_new_connection.swap(true, std::sync::atomic::Ordering::Relaxed),
                    )
                    .await
                {
                    conn.unsubscribe(&channel).await
                } else {
                    // Doing nothing when None as that'll have been logged lower down.
                    Ok(())
                }
            })
            .await
        {
            Ok(()) => {}
            Err(e) => {
                record_exception(
                    "Pubsub: failed to unsubscribe from channel.",
                    format!("{:?}", e),
                );
            }
        }
    }

    /// Returns None when redis down/acting up and couldn't get over a few seconds.
    pub(crate) async fn subscribe<T: ToRedisArgs + FromRedisValue>(
        self: &Arc<Self>,
        channel: impl Into<String>,
    ) -> Option<RedisChannelListener<T>> {
        let channel = channel.into();

        let force_new_connection = AtomicBool::new(false);
        match redis_retry_config()
            .call(|| async {
                if let Some(mut conn) = self
                    .get_conn(
                        // Means on second attempt onwards, will always get new connections.
                        force_new_connection.swap(true, std::sync::atomic::Ordering::Relaxed),
                    )
                    .await
                {
                    conn.subscribe(&channel).await
                } else {
                    // Doing nothing when None as that'll have been logged lower down.
                    Err(redis::RedisError::from(std::io::Error::new(
                        std::io::ErrorKind::Other,
                        "Couldn't get a connection to redis.",
                    )))
                }
            })
            .await
        {
            Ok(()) => {}
            Err(e) => {
                record_exception(
                    "Pubsub: failed to subscribe to channel.",
                    format!("{:?}", e),
                );
                return None;
            }
        }

        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();

        let listener_key = random_u64_rolling();
        self.listeners
            .entry(channel.clone())
            .or_default()
            .insert(listener_key, tx);

        if !self
            .spawned
            .swap(true, std::sync::atomic::Ordering::Relaxed)
        {
            let arc_self = self.clone();
            let mut rx = self
                .rx
                .lock()
                .await
                .take()
                .expect("rx should only be taken once");
            let mut listener_drop_rx = self
                .listener_drop_rx
                .lock()
                .await
                .take()
                .expect("listener_drop_rx should only be taken once");

            tokio::spawn(async move {
                loop {
                    tokio::select! {
                        // Adding this means the listener fut will always be polled first, i.e. has higher priority.
                        // This is what we want as it cleans up dead listeners, so avoids the second fut ideally hitting any dead listeners.
                        biased;

                        result = listener_drop_rx.recv() => {
                            arc_self.spawned_handle_listener_dropped(result).await;
                        }
                        result = rx.recv() => {
                            arc_self.spawned_handle_message(result).await;
                        }
                    }
                }
            });
        }

        Some(RedisChannelListener {
            key: listener_key,
            on_drop_tx: self.listener_drop_tx.clone(),
            channel,
            rx,
            _t: std::marker::PhantomData,
        })
    }

    /// None returned when redis seemingly down/erroring and can't get a connection.
    async fn get_conn(&self, force_new_connection: bool) -> Option<MultiplexedConnection> {
        // Inside rwlock so read only if already in there and not forcing new, to avoid getting a write lock when not needed:
        if !force_new_connection {
            if let Some(conn) = self.active_conn.read().await.as_ref() {
                return Some(conn.clone());
            }
        }

        // If couldn't return above, we need a new conn:
        let mut maybe_conn = self.active_conn.write().await;
        match redis_retry_config()
            .call(move || {
                // WARNING: unlike deadpool for the rest of redis, this is very heavy as it's not pooled.
                self.client
                    .get_multiplexed_async_connection_with_config(&self.config)
            })
            .await
        {
            Ok(mut conn) => {
                // Need to re-subscribe to all actively listened to channels for the new connection:
                for entry in self.listeners.iter() {
                    let channel = entry.key();
                    match conn.subscribe(channel).await {
                        Ok(()) => {}
                        Err(e) => {
                            record_exception(
                                format!("Pubsub: failed to re-subscribe to channel '{}' with newly acquired connection, discarding.", channel),
                                format!("{:?}", e),
                            );
                            *maybe_conn = None;
                            return None;
                        }
                    }
                }
                *maybe_conn = Some(conn);
            }
            Err(e) => {
                record_exception(
                    "Pubsub: creation of a new Redis connection failed.",
                    format!("{:?}", e),
                );
                *maybe_conn = None;
                return None;
            }
        }

        let conn = maybe_conn
            .as_ref()
            .expect("conn should be Some given just created if needed.");

        Some(conn.clone())
    }

    /// Handle cleaning up the listeners dashmap, and calling redis's unsubscribe method when no more listeners for a given channel.
    /// The cleanup channel gets called in the drop fn of each [`RedisChannelListener`].
    async fn spawned_handle_listener_dropped(
        self: &Arc<Self>,
        channel_and_key: Option<(String, u64)>,
    ) {
        match channel_and_key {
            Some((channel, key)) => {
                let unsub = if let Some(mut listeners) = self.listeners.get_mut(&channel) {
                    listeners.remove(&key);
                    listeners.is_empty()
                } else {
                    true
                };
                // Need to come after otherwise dashmap could deadlock.
                if unsub {
                    self.unsubscribe(&channel).await;
                }
            }
            None => {
                record_exception(
                    "Pubsub: redis cleanup channel died. Tx sender supposedly dropped.",
                    "",
                );
            }
        }
    }

    /// Handle redis pubsub messages coming into subscriptions.
    async fn spawned_handle_message(self: &Arc<Self>, message: Option<redis::PushInfo>) {
        match message {
            Some(push_info) => {
                match push_info.kind.clone() {
                    redis::PushKind::Subscribe => {
                        // Example received:
                        // PushInfo { kind: Subscribe, data: [bulk-string('"foo"'), int(1)] }

                        // Don't actually need to do anything for these methods:

                        // match from_owned_redis_value::<(String, i64)>(
                        //     redis::Value::Array(push_info.data),
                        // ) {
                        //     Ok((channel, sub_count)) => {
                        //         tracing::info!(
                        //             "Subscribed to channel: '{}', sub_count: {}",
                        //             channel,
                        //             sub_count
                        //         );
                        //     }
                        //     Err(e) => {
                        //         record_exception(
                        //             "Pubsub: failed to decode redis::PushKind::Subscribe.",
                        //             format!("{:?}", e),
                        //         );
                        //     }
                        // }
                    }
                    redis::PushKind::Unsubscribe => {
                        // Example received:
                        // PushInfo { kind: Unsubscribe, data: [bulk-string('"49878c28-c7ef-4f4c-b196-9956942bbe95:n1:foo"'), int(1)] }

                        // Don't actually need to do anything for these methods:

                        // match from_owned_redis_value::<(String, i64)>(
                        //     redis::Value::Array(push_info.data),
                        // ) {
                        //     Ok((client_and_channel, sub_count)) => {
                        //         tracing::info!(
                        //             "Client unsubscribed from channel: '{}', sub_count: {}",
                        //             client_and_channel,
                        //             sub_count
                        //         );
                        //     }
                        //     Err(e) => {
                        //         record_exception(
                        //             "Pubsub: failed to decode redis::PushKind::Unsubscribe.",
                        //             format!("{:?}", e),
                        //         );
                        //     }
                        // }
                    }
                    redis::PushKind::Disconnection => {
                        tracing::warn!(
                            "Pubsub: redis disconnected, attempting to get new connection, retrying every 100ms until success..."
                        );
                        let result = Retry::fixed(TimeDelta::milliseconds(100))
                            .until_forever()
                            .call(|| async {
                                match self.get_conn(true).await {
                                    Some(_) => {
                                        tracing::info!("Pubsub: redis reconnected.");
                                        Ok(())
                                    }
                                    None => Err(()),
                                }
                            })
                            .await;
                        if result.is_err() {
                            panic!("Should be impossible, above retry loop should go infinitely until success");
                        }
                    }
                    redis::PushKind::Message => {
                        // Example received:
                        // PushInfo { kind: Message, data: [bulk-string('"foo"'), bulk-string('"bar"')] }

                        match from_owned_redis_value::<(String, redis::Value)>(redis::Value::Array(
                            push_info.data,
                        )) {
                            Ok((channel, msg)) => {
                                if let Some(listeners) = self.listeners.get(&channel) {
                                    for tx in listeners.values() {
                                        // Given we have a separate future for cleaning up,
                                        // this shouldn't be a big issue if this ever errors with dead listeners,
                                        // as they should immediately be cleaned up by the cleanup future.
                                        let _ = tx.send(msg.clone());
                                    }
                                }
                            }
                            Err(e) => {
                                record_exception(
                                    "Pubsub: failed to decode redis::PushKind::Message.",
                                    format!("{:?}", e),
                                );
                            }
                        }
                    }
                    _ => {
                        record_exception(
                            "Pubsub: unsupported/unexpected message received by global listener.",
                            format!("{:?}", push_info),
                        );
                    }
                }
            }
            None => {
                record_exception(
                    "Pubsub: redis listener channel died. Tx sender supposedly dropped.",
                    "",
                );
            }
        }
    }
}

// TESTS:
// - redis prefix still used.
// - DONE: sub to same channel twice with same name but 2 different fns, each should be called once, not first twice, second twice, first dropped etc.
// - Redis down then backup:
//    - If just during listening, msgs, shld come in after back up.
//    - if happened before subscribe, subscribe still recorded and applied after redis back up
// - After lots of random channels, lots of random listeners, once all dropped the hashmap should be empty.

// Redis server can't be run on windows:
#[cfg(not(target_os = "windows"))]
#[cfg(test)]
mod tests {

    use chrono::TimeDelta;

    use crate::misc::with_timeout;
    use crate::redis::{Redis, RedisBatchFire, RedisConnLike, RedisStandalone};
    use crate::testing::prelude::*;

    use super::*;

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
