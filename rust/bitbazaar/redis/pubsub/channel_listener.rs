use std::sync::Arc;

use redis::{from_owned_redis_value, FromRedisValue, ToRedisArgs};

use crate::log::record_exception;

use super::pubsub_global::ChannelSubscription;

/// A listener to receive messages from a redis channel via pubsub.
pub struct RedisChannelListener<T> {
    pub(crate) on_drop_tx: Arc<tokio::sync::mpsc::UnboundedSender<(ChannelSubscription, u64)>>,
    pub(crate) key: u64,
    pub(crate) channel_sub: ChannelSubscription,
    pub(crate) rx: tokio::sync::mpsc::UnboundedReceiver<redis::Value>,
    pub(crate) _t: std::marker::PhantomData<T>,
}

impl<T: ToRedisArgs + FromRedisValue> RedisChannelListener<T> {
    /// Get a new message from the channel.
    /// The outer None indicates the channel has been closed erroneously, or the internal data could not be coerced to the target type.
    /// In either case, something's gone wrong, an exception will probably have been recorded too.
    pub async fn recv(&mut self) -> Option<T> {
        if let Some(v) = self.rx.recv().await {
            match from_owned_redis_value(v) {
                Ok(v) => Some(v),
                Err(e) => {
                    record_exception(
                        format!(
                            "Failed to convert redis value to target type '{}'",
                            std::any::type_name::<T>()
                        ),
                        format!("{:?}", e),
                    );
                    None
                }
            }
        } else {
            None
        }
    }
}

/// Tell the global pubsub manager this listener is being dropped.
impl<T> Drop for RedisChannelListener<T> {
    fn drop(&mut self) {
        let _ = self.on_drop_tx.send((self.channel_sub.clone(), self.key));
    }
}
