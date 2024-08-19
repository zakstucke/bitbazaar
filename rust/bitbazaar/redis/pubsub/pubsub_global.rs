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
    misc::{random_u64_rolling, IterWithCloneLazy, Retry},
    prelude::*,
    redis::redis_retry::redis_retry_config,
};

use super::RedisChannelListener;

#[derive(Debug, Clone, Hash, PartialEq, Eq)]
pub(crate) enum ChannelSubscription {
    Concrete(String),
    Pattern(String),
}

impl ChannelSubscription {
    fn is_pattern(&self) -> bool {
        match self {
            ChannelSubscription::Concrete(_) => false,
            ChannelSubscription::Pattern(_) => true,
        }
    }

    fn as_str(&self) -> &str {
        match self {
            ChannelSubscription::Concrete(s) => s,
            ChannelSubscription::Pattern(s) => s,
        }
    }
}

/// The lazy pubsub manager.
pub struct RedisPubSubGlobal {
    client: redis::Client,
    config: redis::AsyncConnectionConfig,
    /// Unlike deadpool these aren't pooled, so definitely need to store and reuse until it becomes invalid, only then get a new one.
    active_conn: tokio::sync::RwLock<Option<MultiplexedConnection>>,
    /// The downstream configured listeners for different channels, messages will be pushed to all active listeners.
    /// Putting in a nested hashmap for easy cleanup when listeners are dropped.
    pub(crate) listeners: DashMap<
        ChannelSubscription,
        HashMap<u64, tokio::sync::mpsc::UnboundedSender<redis::Value>>,
    >,

    /// Below used to trigger unsubscriptions and listeners dashmap cleanup when listeners are dropped.
    /// (The tx is called when a listener is dropped, and the spawned process listens for these and does the cleanup.)
    listener_drop_tx: Arc<tokio::sync::mpsc::UnboundedSender<(ChannelSubscription, u64)>>,

    /// Will be taken when the listener is lazily spawned.
    spawn_init: tokio::sync::Mutex<Option<SpawnInit>>,
    spawned: AtomicBool,

    /// Will be sent on Redis drop to kill the spawned listener.
    on_drop_tx: Option<tokio::sync::oneshot::Sender<()>>,
}

impl Drop for RedisPubSubGlobal {
    fn drop(&mut self) {
        // This will kill the spawned listener, which will in turn kill the spawned process.
        if let Some(on_drop_tx) = self.on_drop_tx.take() {
            let _ = on_drop_tx.send(());
        };
    }
}

#[derive(Debug)]
struct SpawnInit {
    /// The global receiver of messages hooked directly into the redis connection.
    /// This will be taken when the main listener is spawned.
    rx: tokio::sync::mpsc::UnboundedReceiver<redis::PushInfo>,

    // Will receive whenever a listener is dropped:
    listener_drop_rx: tokio::sync::mpsc::UnboundedReceiver<(ChannelSubscription, u64)>,

    // Received when the redis instance dropped, meaning the spawned listener should shutdown.
    on_drop_rx: tokio::sync::oneshot::Receiver<()>,
}

impl Debug for RedisPubSubGlobal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RedisPubSubGlobal")
            .field("client", &self.client)
            // .field("config", &self.config)
            .field("active_conn", &self.active_conn)
            .field("listeners", &self.listeners)
            .field("listener_drop_tx", &self.listener_drop_tx)
            .field("spawn_init", &self.spawn_init)
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
        let (on_drop_tx, on_drop_rx) = tokio::sync::oneshot::channel();
        Ok(Self {
            client,
            config,
            active_conn: tokio::sync::RwLock::new(None),
            listeners: DashMap::new(),
            listener_drop_tx: Arc::new(listener_drop_tx),
            spawn_init: tokio::sync::Mutex::new(Some(SpawnInit {
                rx,
                listener_drop_rx,
                on_drop_rx,
            })),
            spawned: AtomicBool::new(false),
            on_drop_tx: Some(on_drop_tx),
        })
    }

    pub(crate) async fn unsubscribe(&self, channel_sub: &ChannelSubscription) {
        self.listeners.remove(channel_sub);

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
                    match &channel_sub {
                        ChannelSubscription::Concrete(channel) => conn.unsubscribe(&channel).await,
                        ChannelSubscription::Pattern(channel_pattern) => {
                            conn.punsubscribe(&channel_pattern).await
                        }
                    }
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
        self._subscribe_inner(ChannelSubscription::Concrete(channel.into()))
            .await
    }

    /// Returns None when redis down/acting up and couldn't get over a few seconds.
    pub(crate) async fn psubscribe<T: ToRedisArgs + FromRedisValue>(
        self: &Arc<Self>,
        channel_pattern: impl Into<String>,
    ) -> Option<RedisChannelListener<T>> {
        self._subscribe_inner(ChannelSubscription::Pattern(channel_pattern.into()))
            .await
    }

    /// Returns None when redis down/acting up and couldn't get over a few seconds.
    pub(crate) async fn _subscribe_inner<T: ToRedisArgs + FromRedisValue>(
        self: &Arc<Self>,
        channel_sub: ChannelSubscription,
    ) -> Option<RedisChannelListener<T>> {
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
                    match &channel_sub {
                        ChannelSubscription::Concrete(channel) => conn.subscribe(channel).await,
                        ChannelSubscription::Pattern(channel_pattern) => {
                            conn.psubscribe(channel_pattern).await
                        }
                    }
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
                    format!(
                        "Pubsub: failed to subscribe to channel {}.",
                        if channel_sub.is_pattern() {
                            format!("pattern '{}'", channel_sub.as_str())
                        } else {
                            format!("'{}'", channel_sub.as_str())
                        }
                    ),
                    format!("{:?}", e),
                );
                return None;
            }
        }

        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();

        let listener_key = random_u64_rolling();
        self.listeners
            .entry(channel_sub.clone())
            .or_default()
            .insert(listener_key, tx);

        if !self
            .spawned
            .swap(true, std::sync::atomic::Ordering::Relaxed)
        {
            let arc_self = self.clone();
            let mut init = self
                .spawn_init
                .lock()
                .await
                .take()
                .expect("init should only be taken once");

            tokio::spawn(async move {
                // Spawned task will exit only when the on_drop_rx is sent, i.e. when the redis instance is dropped.
                tokio::select! {
                    _ = init.on_drop_rx => {}
                    _ = async {
                        loop {
                            tokio::select! {
                                // Adding this means the listener fut will always be polled first, i.e. has higher priority.
                                // This is what we want as it cleans up dead listeners, so avoids the second fut ideally hitting any dead listeners.
                                biased;
                                result = init.listener_drop_rx.recv() => {
                                    arc_self.spawned_handle_listener_dropped(result).await;
                                }
                                result = init.rx.recv() => {
                                    arc_self.spawned_handle_message(result).await;
                                }
                            }
                        }
                    } => {}
                }
            });
        }

        Some(RedisChannelListener {
            key: listener_key,
            on_drop_tx: self.listener_drop_tx.clone(),
            channel_sub,
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
                    let channel_sub = entry.key();
                    let sub_result = match channel_sub {
                        ChannelSubscription::Concrete(channel) => conn.subscribe(channel).await,
                        ChannelSubscription::Pattern(channel_pattern) => {
                            conn.psubscribe(channel_pattern).await
                        }
                    };
                    match sub_result {
                        Ok(()) => {}
                        Err(e) => {
                            record_exception(
                                format!("Pubsub: failed to re-subscribe to channel {} with newly acquired connection, discarding.", if channel_sub.is_pattern() {
                                    format!("pattern '{}'", channel_sub.as_str())
                                } else {
                                    format!("'{}'", channel_sub.as_str())
                                }),
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
        channel_sub_and_key: Option<(ChannelSubscription, u64)>,
    ) {
        match channel_sub_and_key {
            Some((channel_sub, key)) => {
                let unsub = if let Some(mut listeners) = self.listeners.get_mut(&channel_sub) {
                    listeners.remove(&key);
                    listeners.is_empty()
                } else {
                    true
                };
                // Need to come after otherwise dashmap could deadlock.
                if unsub {
                    self.unsubscribe(&channel_sub).await;
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
                    redis::PushKind::PSubscribe | redis::PushKind::PUnsubscribe => {}
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
                                self.handle_msg(ChannelSubscription::Concrete(channel), msg)
                                    .await;
                            }
                            Err(e) => {
                                record_exception(
                                    "Pubsub: failed to decode redis::PushKind::Message.",
                                    format!("{:?}", e),
                                );
                            }
                        }
                    }
                    // Patterns come in separately.
                    redis::PushKind::PMessage => {
                        // Example received:
                        // PushInfo { kind: PMessage, data: [bulk-string('"f*o"'), bulk-string('"foo"'), bulk-string('"only_pattern"')] }

                        match from_owned_redis_value::<(String, redis::Value, redis::Value)>(
                            redis::Value::Array(push_info.data),
                        ) {
                            Ok((channel_pattern, _concrete_channel, msg)) => {
                                self.handle_msg(ChannelSubscription::Pattern(channel_pattern), msg)
                                    .await;
                            }
                            Err(e) => {
                                record_exception(
                                    "Pubsub: failed to decode redis::PushKind::PMessage.",
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

    async fn handle_msg(&self, channel_sub: ChannelSubscription, msg: redis::Value) {
        if let Some(listeners) = self.listeners.get(&channel_sub) {
            for (tx, msg) in listeners.values().with_clone_lazy(msg) {
                // Given we have a separate future for cleaning up,
                // this shouldn't be a big issue if this ever errors with dead listeners,
                // as they should immediately be cleaned up by the cleanup future.
                let _ = tx.send(msg);
            }
        }
    }
}
