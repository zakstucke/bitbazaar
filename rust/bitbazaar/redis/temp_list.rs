use std::{
    borrow::Cow,
    sync::{atomic::AtomicI64, Arc},
};

use chrono::TimeDelta;
use futures::future::BoxFuture;
use tokio::sync::{MappedMutexGuard, Mutex, MutexGuard};

use super::{batch::*, conn::RedisConnOwned, RedisChannelListener, RedisConnLike, RedisJson};
#[cfg(test)]
use crate::prelude::*;
use crate::{
    misc::{FlexiLog, FlexiLogFromRedis, FlexiLogPhase, FlexiLogWriter},
    redis::RedisJsonBorrowed,
};

/// A wrapped item, with a connection too, preventing need to pass 2 things around if useful for certain interfaces.
#[derive(Debug)]
pub struct RedisTempListItemWithConn<T> {
    // Mutability kept internal.
    item: Arc<Mutex<RedisTempListItem<T>>>,
    conn: RedisConnOwned,
}

impl<
        T: FlexiLogWriter + Send + Sync + 'static + serde::Serialize + for<'d> serde::Deserialize<'d>,
    > FlexiLog for RedisTempListItemWithConn<T>
{
    type Writer = T;

    fn batch(&self, cb: impl FnOnce(&mut Self::Writer) + Send + 'static) {
        // Sync function but async internals, so have to spawn:
        let conn = self.conn.clone();
        let item = self.item.clone();
        tokio::spawn(async move {
            let mut locked = item.lock().await;
            locked
                .update(&conn, move |updater| {
                    cb(updater);
                })
                .await;
        });
    }

    async fn phase(&self) -> FlexiLogPhase {
        let maybe_item = self.item().await;
        if let Some(inner) = maybe_item.as_ref() {
            inner.phase()
        } else {
            crate::misc::FlexiLogPhase::Pending
        }
    }

    async fn progress(&self) -> f64 {
        let maybe_item = self.item().await;
        if let Some(inner) = maybe_item.as_ref() {
            inner.progress()
        } else {
            0.0
        }
    }
}

impl<
        T: FlexiLogWriter
            + Send
            + Sync
            + 'static
            + serde::Serialize
            + for<'de> serde::Deserialize<'de>,
    > FlexiLogFromRedis for RedisTempListItem<T>
{
    type FlexiLogger = RedisTempListItemWithConn<T>;

    fn into_flexi_log(self, redis: &super::Redis) -> Self::FlexiLogger {
        self.into_with_conn(redis.conn())
    }
}

impl<T: serde::Serialize + for<'de> serde::Deserialize<'de>> RedisTempListItemWithConn<T> {
    /// See [`RedisTempListItem::update`]
    pub async fn update(&self, updater: impl FnOnce(&mut T)) {
        self.item.lock().await.update(&self.conn, updater).await;
    }

    /// See [`RedisTempListItem::update_async`]
    pub async fn update_async(&self, updater: impl for<'e> FnOnce(&'e mut T) -> BoxFuture<'e, ()>) {
        self.item
            .lock()
            .await
            .update_async(&self.conn, updater)
            .await;
    }

    /// See [`RedisTempListItem::replace`]
    pub async fn replace(&mut self, replacer: impl FnOnce() -> T) {
        self.item.lock().await.replace(&self.conn, replacer).await;
    }

    /// See [`RedisTempListItem::uid`]
    pub async fn uid(&self) -> MappedMutexGuard<Option<String>> {
        MutexGuard::map(self.item.lock().await, |item| &mut item.maybe_uid)
    }

    /// See [`RedisTempListItem::item`]
    pub async fn item(&self) -> MappedMutexGuard<Option<T>> {
        MutexGuard::map(self.item.lock().await, |item| &mut item.maybe_item)
    }
}

/// A user friendly interface around a redis list item, allowing for easy updates and replacements.
/// This encapsulates when items aren't available, or the user doesn't actually want to use an item in some cases, but doesn't want to pass Options<> around.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct RedisTempListItem<T> {
    // By making all this optional, it encapsulates a lot of user logic & redis errors:
    // - redis failure
    // - key expiries
    // - list expiries
    // - the user just not wanting to use a list in a case, but still wants to use a fn that might take a list.

    // The arc is used to prevent a need to copy for each time, but can serialize with it if needed.
    maybe_tmp_list: Option<Arc<RedisTempList>>,

    maybe_uid: Option<String>,
    maybe_item: Option<T>,
}

impl<T: serde::Serialize + for<'de> serde::Deserialize<'de>> RedisTempListItem<T> {
    /// Useful helper utility to just get a vec of valid items from a vec of holders.
    /// E.g. `RedisTempListItem::vec_items(vec![item1, item2, item3])`
    pub fn vec_items(items: Vec<Self>) -> Vec<T> {
        items.into_iter().filter_map(|x| x.into_item()).collect()
    }

    /// Useful for combining a connection with an item, to prevent needing to pass both around.
    pub fn into_with_conn(self, conn: impl RedisConnLike) -> RedisTempListItemWithConn<T> {
        RedisTempListItemWithConn {
            item: Arc::new(Mutex::new(self)),
            conn: conn.to_conn_owned(),
        }
    }

    /// Create a new holder for a redis list item. All optional to encapsulate error paths if needed.
    pub fn new(
        uid: Option<String>,
        item: Option<T>,
        tmp_list: Option<&Arc<RedisTempList>>, // Forcing entry as ref and will clone the arc in here, to make it clear what's going on.
    ) -> Self {
        Self {
            maybe_tmp_list: tmp_list.cloned(),
            maybe_uid: uid,
            maybe_item: item,
        }
    }

    /// Create a dummy holder for a redis list item, useful when the list isn't available but you still want to use the holder elsewhere.
    pub fn new_dummy() -> Self {
        Self {
            maybe_tmp_list: None,
            maybe_uid: None,
            maybe_item: None,
        }
    }

    /// Fully manage the update of an item back to redis.
    /// This interface is designed to encapsulate all failure logic,
    /// and not run the callback if the item wasn't needed, didn't exist etc.
    pub async fn update(&mut self, conn: &impl RedisConnLike, updater: impl FnOnce(&mut T)) {
        let mut try_push = false;
        if let Some(tmp_list) = &self.maybe_tmp_list {
            if let Some(item) = self.maybe_item.as_mut() {
                // Run the user's callback to make all changes.
                updater(item);
                if let Some(uid) = &self.maybe_uid {
                    // Sync those changes back to the list/redis:
                    tmp_list.update(conn, uid, item).await;
                } else {
                    // Given we have the list but not the uid, we can just push it onto the list instead:
                    try_push = true;
                }
            }
        }

        // For borrowing reasons doing separately to above:
        if try_push {
            if let Some(tmp_list) = self.maybe_tmp_list.clone() {
                if let Some(item) = self.maybe_item.take() {
                    *self = tmp_list.push(conn, item).await;
                }
            }
        }
    }

    /// Fully manage the update of an item back to redis. (Async callback version)
    /// This interface is designed to encapsulate all failure logic,
    /// and not run the callback if the item wasn't needed, didn't exist etc.
    pub async fn update_async(
        &mut self,
        conn: &impl RedisConnLike,
        updater: impl for<'e> FnOnce(&'e mut T) -> BoxFuture<'e, ()>,
    ) {
        let mut try_push = false;
        if let Some(tmp_list) = &self.maybe_tmp_list {
            if let Some(item) = self.maybe_item.as_mut() {
                // Run the user's callback to make all changes.
                updater(item).await;
                if let Some(uid) = &self.maybe_uid {
                    // Sync those changes back to the list/redis:
                    tmp_list.update(conn, uid, item).await;
                } else {
                    // Given we have the list but not the uid, we can just push it onto the list instead:
                    try_push = true;
                }
            }
        }

        // For borrowing reasons doing separately to above:
        if try_push {
            if let Some(tmp_list) = self.maybe_tmp_list.clone() {
                if let Some(item) = self.maybe_item.take() {
                    *self = tmp_list.push(conn, item).await;
                }
            }
        }
    }

    /// Replace the contents of an item in the redis list with new, discarding any previous.
    /// This encapsulates the logic of no list being available, the callback will then never run.
    pub async fn replace(&mut self, conn: &impl RedisConnLike, replacer: impl FnOnce() -> T) {
        let mut try_push_with_next_item = (false, None);
        if let Some(tmp_list) = &self.maybe_tmp_list {
            let next_item = replacer();
            if let Some(uid) = &self.maybe_uid {
                // Sync those changes back to the list/redis:
                tmp_list.update(conn, uid, &next_item).await;
                self.maybe_item = Some(next_item);
            } else {
                // Given we have the list but not the uid, we can just push it onto the list instead:
                try_push_with_next_item = (true, Some(next_item));
            }
        }
        // For borrowing reasons doing separately to above:
        if let Some(tmp_list) = self.maybe_tmp_list.clone() {
            if try_push_with_next_item.0 {
                if let Some(next_item) = try_push_with_next_item.1 {
                    *self = tmp_list.push(conn, next_item).await;
                }
            }
        }
    }

    /// Delete the item from the list.
    /// Will be a no-op of the item wrapper is actually empty for some reason.
    pub async fn delete(self, conn: &impl RedisConnLike) {
        if let Some(tmp_list) = self.maybe_tmp_list {
            if let Some(uid) = self.maybe_uid {
                tmp_list.delete(conn, &uid).await;
            }
        }
    }

    /// Access the underlying item's uid, if it exists.
    pub fn uid(&self) -> Option<&str> {
        self.maybe_uid.as_deref()
    }

    /// Access the underlying item, if it exists.
    pub fn item(&self) -> Option<&T> {
        self.maybe_item.as_ref()
    }

    /// Consume the holder, returning the item, if it exists.
    pub fn into_item(self) -> Option<T> {
        self.maybe_item
    }
}

/// Change events sent through a redis pubsub channel.
#[derive(Debug, Clone, serde::Deserialize, serde::Serialize, PartialEq, Eq)]
pub enum RedisTempListChangeEvent {
    /// New items added with these uids.
    Extended(Vec<String>),
    /// Item updated with this uid.
    Updated(String),
    /// Items removed with these uids.
    Removed(Vec<String>),
    /// All items cleared.
    Clear,
}

/// Connect up to a magic redis list that:
/// - Has pubsub enabled on all modifications.
/// - Has an expiry on the list itself, resetting on each read or write. (each change lives again for `list_inactive_ttl` time)
/// - Each item in the list has it's own expiry, so the list is always clean of old items.
/// - Each item has a generated unique key, this key can be used to update or delete specific items directly.
/// - Returned items are returned newest/last-updated to oldest
///
/// This makes this distributed data structure perfect for stuff like:
/// - recent/temporary logs/events of any sort.
/// - pending actions, that can be updated in-place by the creator, but read as part of a list by a viewer etc.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct RedisTempList {
    /// The namespace of the list in redis (should come in as static, but Cow needed because of deserialization)
    namespace: Cow<'static, str>,

    /// The key of the list in redis
    key: String,

    /// If the list hasn't been read or written to in this time, it will be expired.
    list_inactive_ttl_ms: i64,

    /// If an item hasn't been read or written to in this time, it will be expired.
    item_inactive_ttl_ms: i64,

    /// Used to prevent overlap between push() and extend() calls using the same ts by accident.
    #[serde(skip)]
    last_extension_ts_millis: Arc<AtomicI64>,
}

impl RedisTempList {
    /// Create/connect to a managed list entry in redis that will:
    /// - Auto expire on inactivity
    /// - Return items newest to oldest
    /// - Items are set with their own ttl, so the members themselves expire separate from the full list
    /// - Optionally prevents duplicate values. When enabled, if a duplicate is added, the item will be bumped to the front & old discarded.
    pub fn new(
        namespace: &'static str,
        key: impl Into<String>,
        list_inactive_ttl: TimeDelta,
        item_inactive_ttl: TimeDelta,
    ) -> Arc<Self> {
        Arc::new(Self {
            namespace: Cow::Borrowed(namespace),
            key: key.into(),
            list_inactive_ttl_ms: list_inactive_ttl.num_milliseconds(),
            item_inactive_ttl_ms: item_inactive_ttl.num_milliseconds(),
            last_extension_ts_millis: Arc::new(AtomicI64::new(0)),
        })
    }

    /// Subscribe via redis pubsub to changes to the list.
    pub async fn subscribe_to_changes(
        &self,
        conn: &impl RedisConnLike,
    ) -> Option<RedisChannelListener<RedisTempListChangeEvent>> {
        conn.subscribe(&self.namespace, &self.key).await
    }

    /// The score should be the utc timestamp to expire:
    async fn extend_inner<'a, T>(
        &self,
        conn: &impl RedisConnLike,
        items: impl IntoIterator<Item = &'a T>,
    ) -> Option<Vec<String>>
    where
        T: 'a + serde::Deserialize<'a>,
        &'a T: serde::Serialize,
    {
        let score = chrono::Utc::now().timestamp_millis() + self.item_inactive_ttl_ms;

        // Get the current millis, but actually increment the value if they're the same, protects against accidentally using the same ts on separate quickfire calls.
        let last_ts_millis = self
            .last_extension_ts_millis
            .load(std::sync::atomic::Ordering::Relaxed);
        let mut current_ts_millis = chrono::Utc::now().timestamp_millis();
        if current_ts_millis <= last_ts_millis {
            current_ts_millis = last_ts_millis + 1;
        }
        self.last_extension_ts_millis
            .store(current_ts_millis, std::sync::atomic::Ordering::Relaxed);

        fn generate_id(index: usize, current_ts_millis: &i64) -> String {
            // Why am I adding the ts millis and index?
            // When tts is the same, keys are returned reverse lexographically.
            // We want the latest added to be first, hence the millis and index. So the last added will have the highest millis and index.
            // This also means stable ordering whatever the generated key, useful for testing.
            format!("{}-{}-{}", current_ts_millis, index, uuid::Uuid::new_v4())
        }

        let items_with_uids = items
            .into_iter()
            .enumerate()
            .map(|(index, item)| (generate_id(index, &current_ts_millis), item))
            .collect::<Vec<_>>();

        let uids = items_with_uids
            .iter()
            .map(|(uid, _)| uid.to_string())
            .collect::<Vec<_>>();

        let result = conn
            .batch()
            // Add the uids to the main set, the command will auto update the set's ttl (self.list_inactive_ttl) given it's been updated.
            .zadd_multi(
                &self.namespace,
                &self.key,
                Some(self.list_inactive_ttl()), // This will auto reset the expire time of the list as a whole
                items_with_uids
                    .iter()
                    .map(|(uid, _)| (score, uid))
                    .collect::<Vec<_>>()
                    .as_slice(),
            )
            // Now store the values themselves as normal redis keys with the same ttl: (these are normal ttls that auto clean up)
            .mset(
                &self.namespace,
                items_with_uids
                    .into_iter()
                    .map(|(uid, item)| (uid, RedisJsonBorrowed(item))),
                Some(self.item_inactive_ttl()),
            )
            // Cleanup old members that have now expired:
            .zremrangebyscore(
                &self.namespace,
                &self.key,
                i64::MIN,
                chrono::Utc::now().timestamp_millis(),
            )
            // Notify any subs given we're updating the list:
            .publish(
                &self.namespace,
                &self.key,
                RedisTempListChangeEvent::Extended(uids.clone()),
            )
            .fire()
            .await;

        // Even though the result is empty, if result is None then something went wrong, so keep sending None outwards.
        if result.is_some() {
            Some(uids)
        } else {
            None
        }
    }

    /// Add a new item to the sorted list.
    ///
    /// This will also:
    /// - Autoreset list's expire time to self.list_inactive_ttl from now
    /// - Clean up expired list items
    ///
    /// Returns:
    /// `RedisTempListItem<T>`: The resulting item holder which can be used to further manipulate the items.
    pub async fn push<'a, T: serde::Serialize + for<'b> serde::Deserialize<'b>>(
        self: &'a Arc<Self>, // Using arc to be cloning references into the list items rather than the full list object each time.
        conn: &impl RedisConnLike,
        item: T,
    ) -> RedisTempListItem<T> {
        let uids = self.extend_inner(conn, std::iter::once(&item)).await;
        let uid = if let Some(uids) = uids {
            if uids.len() != 1 {
                tracing::error!(
                    "Expected 1 uid to be returned during temp list push, got: {:?}",
                    uids
                );
                None
            } else {
                Some(uids.into_iter().next().unwrap())
            }
        } else {
            None
        };

        if let Some(uid) = uid {
            RedisTempListItem::new(Some(uid), Some(item), Some(self))
        } else {
            RedisTempListItem::new_dummy()
        }
    }

    /// Add multiple items to the sorted list.
    /// Purposely using one ttl for all, should think about why if you're needing to set different ttls to items you're adding together!
    ///
    /// This will also:
    /// - Autoreset list's expire time to self.list_inactive_ttl from now
    /// - Clean up expired list items
    ///
    /// Returns:
    /// - `Some(Vec<RedisTempListItem<T>>)`: The resulting item holders for each of the items added, these can be used to further manipulate the items.
    /// - `None`: Something went wrong and the items weren't added correctly.
    pub async fn extend<'a, T: serde::Serialize + for<'b> serde::Deserialize<'b>>(
        self: &'a Arc<Self>, // Using arc to be cloning references into the list items rather than the full list object each time.
        conn: &impl RedisConnLike,
        items: impl IntoIterator<Item = T>,
    ) -> Vec<RedisTempListItem<T>> {
        let items = items.into_iter().collect::<Vec<_>>();
        let uids = self.extend_inner(conn, &items).await;
        if uids.is_some() {
            let uids = uids.unwrap();
            uids.into_iter()
                .zip(items.into_iter())
                .map(|(uid, item)| RedisTempListItem::new(Some(uid), Some(item), Some(self)))
                .collect()
        } else {
            uids.into_iter()
                .map(|_| RedisTempListItem::new_dummy())
                .collect()
        }
    }

    /// Underlying of [`RedisTempList::read_recent`], but returns the `(i64: ttl, String: item key, T: item)` rather than `RedisTempList<T>` that makes working with items easier.
    pub async fn read_recent_raw<T: serde::Serialize + for<'a> serde::Deserialize<'a>>(
        &self,
        conn: &impl RedisConnLike,
        limit: Option<isize>,
    ) -> Vec<(i64, String, T)> {
        // NOTE: because of the separation between the root list, and the values themselves as a separate redis keys, 2 calls are needed.
        // 1. Get the uids from the list
        // 2. Get the values from the uids
        // This cannot be done without a round-trip through a script because redis requires all keys used in scripts to be known ahead of time (using KEYS), so can't use that.

        let item_info = conn
            .batch()
            // Cleanup old members that have now expired first because don't want these to be included in the read:
            .zremrangebyscore(
                &self.namespace,
                &self.key,
                i64::MIN,
                chrono::Utc::now().timestamp_millis(),
            )
            .zrangebyscore_high_to_low::<String>(
                &self.namespace,
                &self.key,
                i64::MIN,
                i64::MAX,
                limit,
            )
            .fire()
            .await;

        // Only continuing if succeeded to get:
        if let Some(item_info) = item_info {
            // Filter out an uids that failed to decode (should never happen)
            let item_info = item_info
                .into_iter()
                .filter_map(|(uid, score)| uid.map(|uid| (uid, score)))
                .collect::<Vec<_>>();

            // Don't continue if no items successfully decoded:
            if !item_info.is_empty() {
                // Pull the items using the retrieved uids:
                let items = conn
                    .batch()
                    .mget::<RedisJson<T>>(
                        &self.namespace,
                        &item_info.iter().map(|(uid, _)| uid).collect::<Vec<_>>(),
                    )
                    // Unlike our zadd during setting, need to manually refresh the expire time of the list here:
                    .expire(&self.namespace, &self.key, self.list_inactive_ttl())
                    .fire()
                    .await;

                // Only continuing if succeeded to get:
                if let Some(items) = items {
                    // - Exclude None items, ones which couldn't be deserialized to RedisJson<T>
                    // - Consume the RedisJson<<T> to get the inner T
                    // - Combine with the score and uid
                    return items
                        .into_iter()
                        .zip(item_info.into_iter())
                        .filter_map(|(item, (uid, score))| {
                            if let Some(item) = item {
                                Some((score, uid, item.0))
                            } else {
                                None
                            }
                        })
                        .collect();
                }
            }
        }

        // if anything went wrong, list empty etc, return empty vec:
        vec![]
    }

    /// Read multiple items from the list, ordered from last updated to least (newest to oldest).
    ///
    /// This will also:
    /// - Autoreset list's expire time to self.list_inactive_ttl from now
    /// - Clean up expired list items
    ///
    /// Returns:
    /// - `Vec<RedisTempListItem<T>`: The wrapped items in the list from newest to oldest up to the provided limit (if any).
    pub async fn read_recent<T: serde::Serialize + for<'a> serde::Deserialize<'a>>(
        self: &Arc<Self>, // Using arc to be cloning references into the list items rather than the full list object each time.
        conn: &impl RedisConnLike,
        limit: Option<isize>,
    ) -> Vec<RedisTempListItem<T>> {
        self.read_recent_raw::<T>(conn, limit)
            .await
            .into_iter()
            .map(|(_score, uid, item)| RedisTempListItem::new(Some(uid), Some(item), Some(self)))
            .collect()
    }

    /// Read a specific items given their uids.
    ///
    /// This will also:
    /// - Autoreset list's expire time to self.list_inactive_ttl from now
    /// - Clean up expired list items
    ///
    /// Returns:
    /// - `Vec<RedisTempListItem<T>>`: The item holder that encapsulates any error logic.
    pub async fn read_multi<S: AsRef<str>, T: serde::Serialize + for<'a> serde::Deserialize<'a>>(
        self: &Arc<Self>, // Using arc to be cloning references into the list items rather than the full list object each time.
        conn: &impl RedisConnLike,
        uids: impl IntoIterator<Item = S>,
    ) -> Vec<RedisTempListItem<T>> {
        let uids = uids
            .into_iter()
            .map(|x| x.as_ref().to_string())
            .collect::<Vec<_>>();
        conn.batch()
            // Cleanup old members that have now expired first because don't want these to be included in the read:
            .zremrangebyscore(
                &self.namespace,
                &self.key,
                i64::MIN,
                chrono::Utc::now().timestamp_millis(),
            )
            .mget::<RedisJson<T>>(&self.namespace, &uids)
            // Unlike our zadd during setting, need to manually refresh the expire time of the list here:
            .expire(&self.namespace, &self.key, self.list_inactive_ttl())
            .fire()
            .await
            .unwrap_or_default()
            .into_iter()
            .zip(uids.into_iter())
            .filter_map(|(item, uid)| {
                if let Some(item) = item {
                    Some(RedisTempListItem::new(Some(uid), Some(item.0), Some(self)))
                } else {
                    None
                }
            })
            .collect()
    }

    /// Read a specific item given it's uid.
    ///
    /// This will also:
    /// - Autoreset list's expire time to self.list_inactive_ttl from now
    /// - Clean up expired list items
    ///
    /// Returns:
    /// - `RedisTempListItem<T>`: The item holder that encapsulates any error logic.
    pub async fn read<T: serde::Serialize + for<'a> serde::Deserialize<'a>>(
        self: &Arc<Self>, // Using arc to be cloning references into the list items rather than the full list object each time.
        conn: &impl RedisConnLike,
        uid: &str,
    ) -> RedisTempListItem<T> {
        if let Some(item) = self.read_multi(conn, std::iter::once(uid)).await.pop() {
            item
        } else {
            RedisTempListItem::new_dummy()
        }
    }

    /// Delete a specific item given it's uid.
    ///
    /// This will also:
    /// - Autoreset list's expire time to self.list_inactive_ttl from now
    /// - Clean up expired list items
    pub async fn delete(&self, conn: &impl RedisConnLike, uid: &str) {
        self.delete_multi(conn, std::iter::once(uid)).await;
    }

    /// Delete multiple items via their ids.
    ///
    /// This will also:
    /// - Autoreset list's expire time to self.list_inactive_ttl from now
    /// - Clean up expired list items
    pub async fn delete_multi<S: Into<String>>(
        &self,
        conn: &impl RedisConnLike,
        uids: impl IntoIterator<Item = S>,
    ) {
        let uids = uids.into_iter().map(Into::into).collect::<Vec<String>>();
        conn.batch()
            // Cleanup old members that have now expired first because don't want these to be included in the read:
            .zremrangebyscore(
                &self.namespace,
                &self.key,
                i64::MIN,
                chrono::Utc::now().timestamp_millis(),
            )
            .zrem(&self.namespace, &self.key, uids.as_slice())
            // Unlike our zadd during setting, need to manually refresh the expire time of the list here:
            .expire(&self.namespace, &self.key, self.list_inactive_ttl())
            .publish(
                &self.namespace,
                &self.key,
                RedisTempListChangeEvent::Removed(uids),
            )
            .fire()
            .await;
    }

    /// Update a specific item given it's uid.
    ///
    /// This will also:
    /// - Autoreset list's expire time to self.list_inactive_ttl from now
    /// - Clean up expired list items
    /// - Reset the item's expiry time, given its been updated
    ///
    pub async fn update<'a, T>(&self, conn: &impl RedisConnLike, uid: &str, item: &'a T)
    where
        T: 'a + serde::Deserialize<'a>,
        &'a T: serde::Serialize,
    {
        let new_score = chrono::Utc::now().timestamp_millis() + self.item_inactive_ttl_ms;

        conn.batch()
            // This will update the uid's score/ttl, redis will automatically see it already existed (if it hadn't already expired) and update it.
            // It will also implicitly renew the list's ttl.
            .zadd(
                &self.namespace,
                &self.key,
                Some(self.list_inactive_ttl()),
                new_score,
                uid,
            )
            // Update the value itself, which is stored under the uid as a normal redis value:
            .set(
                &self.namespace,
                uid,
                RedisJsonBorrowed(item),
                Some(self.item_inactive_ttl()),
            )
            // Cleanup old members that have now expired:
            .zremrangebyscore(
                &self.namespace,
                &self.key,
                i64::MIN,
                chrono::Utc::now().timestamp_millis(),
            )
            // Notify any subs given we're updating the list:
            .publish(
                &self.namespace,
                &self.key,
                RedisTempListChangeEvent::Updated(uid.to_string()),
            )
            .fire()
            .await;
    }

    /// Clear all the items in the list.
    /// (by just deleting the list itself, stored values will still live until their ttl but used a random uid so no conflicts)
    pub async fn clear(&self, conn: &impl RedisConnLike) {
        conn.batch()
            .clear(&self.namespace, std::iter::once(self.key.as_str()))
            // Notify any subs given we're updating the list:
            .publish(&self.namespace, &self.key, RedisTempListChangeEvent::Clear)
            .fire()
            .await;
    }

    fn list_inactive_ttl(&self) -> TimeDelta {
        TimeDelta::milliseconds(self.list_inactive_ttl_ms)
    }

    fn item_inactive_ttl(&self) -> TimeDelta {
        TimeDelta::milliseconds(self.item_inactive_ttl_ms)
    }
}

// Redis server can't be run on windows:
#[cfg(not(target_os = "windows"))]
#[cfg(test)]
mod tests {

    use chrono::TimeDelta;

    use super::*;
    use crate::misc::with_timeout;
    use crate::redis::{Redis, RedisStandalone};
    use crate::test::prelude::*;

    #[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
    pub struct ExampleObject {
        pub a: i32,
        pub b: String,
    }

    #[rstest]
    #[tokio::test]
    async fn test_redis_temp_list_subscription(
        #[allow(unused_variables)] logging: (),
    ) -> RResult<(), AnyErr> {
        let server = RedisStandalone::new_no_persistence().await?;
        let r = Redis::new(server.client_conn_str(), uuid::Uuid::new_v4())?;
        let conn = r.conn();

        let li = RedisTempList::new(
            "templist_tests",
            "n1",
            TimeDelta::milliseconds(1000),
            TimeDelta::milliseconds(1000),
        );
        let mut rx = li.subscribe_to_changes(&conn).await.unwrap();

        let mut items = li
            .extend(
                &conn,
                vec!["A".to_string(), "B".to_string(), "C".to_string()],
            )
            .await;
        let mut c_item = items.pop().unwrap();
        let b_item = items.pop().unwrap();
        let b_uid = b_item.uid().unwrap().to_string();
        let a_item = items.pop().unwrap();
        b_item.delete(&conn).await;
        c_item.update(&conn, |x| *x = "C_UPDATED".to_string()).await;
        li.clear(&conn).await;

        with_timeout(
            TimeDelta::seconds(3),
            || {
                panic!("Timeout waiting for pubsub message");
            },
            async move {
                assert_eq!(
                    rx.recv().await.unwrap(),
                    RedisTempListChangeEvent::Extended(vec![
                        a_item.uid().unwrap().to_string(),
                        b_uid.clone(),
                        c_item.uid().unwrap().to_string()
                    ])
                );
                assert_eq!(
                    rx.recv().await.unwrap(),
                    RedisTempListChangeEvent::Removed(vec![b_uid])
                );
                assert_eq!(
                    rx.recv().await.unwrap(),
                    RedisTempListChangeEvent::Updated(c_item.uid().unwrap().to_string())
                );
                assert_eq!(rx.recv().await.unwrap(), RedisTempListChangeEvent::Clear);
                // Shouldn't receive any more messages:
                with_timeout(
                    TimeDelta::seconds(1),
                    || Ok::<_, Report<AnyErr>>(()),
                    async move {
                        let resp = rx.recv().await;
                        panic!("Unexpected message: {:?}", resp);
                    },
                )
                .await?;
                Ok::<_, Report<AnyErr>>(())
            },
        )
        .await?;

        Ok(())
    }

    #[rstest]
    #[tokio::test]
    async fn test_redis_temp_list(#[allow(unused_variables)] logging: ()) -> RResult<(), AnyErr> {
        let server = RedisStandalone::new_no_persistence().await?;
        let r = Redis::new(server.client_conn_str(), uuid::Uuid::new_v4())?;
        // Just checking the object is normal: (from upstream)
        fn is_normal<T: Sized + Send + Sync + Unpin>() {}
        is_normal::<RedisTempList>();

        let conn = r.conn();

        static NS: &str = "templist_tests";

        let li1 = RedisTempList::new(
            NS,
            "t1",
            TimeDelta::milliseconds(100),
            TimeDelta::milliseconds(60),
        );
        li1.extend(
            &conn,
            vec!["i1".to_string(), "i2".to_string(), "i3".to_string()],
        )
        .await;
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        li1.push(&conn, "i4".to_string()).await;
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        li1.push(&conn, "i5".to_string()).await;
        // Keys are ordered from recent to old:
        assert_eq!(
            RedisTempListItem::vec_items(li1.read_recent::<String>(&conn, None).await),
            vec!["i5", "i4", "i3", "i2", "i1"]
        );
        // Read with limit, should be keeping newest and dropping oldest:
        assert_eq!(
            RedisTempListItem::vec_items(li1.read_recent::<String>(&conn, Some(3)).await),
            vec!["i5", "i4", "i3"]
        );
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        // First batch should have expired now (3 20ms waits)
        // i4 should be expired, it had a ttl of 20ms:
        assert_eq!(
            RedisTempListItem::vec_items(li1.read_recent::<String>(&conn, None).await),
            vec!["i5", "i4"]
        );
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        // i4 should be gone now:
        assert_eq!(
            RedisTempListItem::vec_items(li1.read_recent::<String>(&conn, None).await),
            vec!["i5"]
        );
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        // i5 should be gone now:
        assert_eq!(
            RedisTempListItem::vec_items(li1.read_recent::<String>(&conn, None).await),
            Vec::<String>::new()
        );

        // Let's create a strange list, one with a short list lifetime but effectively infinite item lifetime:
        let li2 = RedisTempList::new(
            NS,
            "t2",
            TimeDelta::milliseconds(50),
            TimeDelta::milliseconds(1000),
        );
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        let i1 = li2.push(&conn, "i1".to_string()).await;
        // Li should still be there after another 40ms, as the last push should have updated the list's ttl:
        tokio::time::sleep(std::time::Duration::from_millis(40)).await;
        assert_eq!(
            RedisTempListItem::vec_items(li2.read_recent::<String>(&conn, None).await),
            vec!["i1"]
        );
        tokio::time::sleep(std::time::Duration::from_millis(40)).await;
        li2.extend(
            &conn,
            vec!["i2".to_string(), "i3".to_string(), "i4".to_string()],
        )
        .await;
        // Li should still be there after another 40ms, as the last push should have updated the list's ttl:
        tokio::time::sleep(std::time::Duration::from_millis(40)).await;
        assert_eq!(
            RedisTempListItem::vec_items(li2.read_recent::<String>(&conn, None).await),
            vec!["i4", "i3", "i2", "i1"]
        );
        // Above read_recent should have also updated the list's ttl, so should be there after another 40ms:
        tokio::time::sleep(std::time::Duration::from_millis(40)).await;
        assert_eq!(
            li2.read::<String>(&conn, i1.uid().unwrap())
                .await
                .into_item(),
            Some("i1".to_string())
        );
        // Above direct read should have also updated the list's ttl, so should be there after another 40ms:
        tokio::time::sleep(std::time::Duration::from_millis(40)).await;
        assert_eq!(
            li2.read::<String>(&conn, i1.uid().unwrap())
                .await
                .into_item(),
            Some("i1".to_string())
        );
        let i5 = li2.push(&conn, "i5".to_string()).await;
        tokio::time::sleep(std::time::Duration::from_millis(40)).await;
        li2.delete(&conn, i1.uid().unwrap()).await;
        // Above delete should have updated the list's ttl, so should be there after another 40ms:
        tokio::time::sleep(std::time::Duration::from_millis(40)).await;
        assert_eq!(
            RedisTempListItem::vec_items(li2.read_recent::<String>(&conn, None).await),
            vec!["i5", "i4", "i3", "i2"]
        );
        tokio::time::sleep(std::time::Duration::from_millis(40)).await;
        li2.update(&conn, i5.uid().unwrap(), &"i5-updated").await;
        // Above update should have updated the list's ttl, so should be there after another 40ms:
        tokio::time::sleep(std::time::Duration::from_millis(40)).await;
        assert_eq!(
            li2.read::<String>(&conn, i5.uid().unwrap())
                .await
                .into_item(),
            Some("i5-updated".to_string())
        );
        // When no reads or writes, the list should expire after 50ms:
        tokio::time::sleep(std::time::Duration::from_millis(60)).await;
        assert_eq!(
            RedisTempListItem::vec_items(li2.read_recent::<String>(&conn, None).await),
            Vec::<String>::new()
        );

        // Make sure manual clear() works on a new list too:
        let li3 = RedisTempList::new(
            NS,
            "t3",
            TimeDelta::milliseconds(100),
            TimeDelta::milliseconds(30),
        );
        li3.extend(
            &conn,
            vec!["i1".to_string(), "i2".to_string(), "i3".to_string()],
        )
        .await;
        assert_eq!(
            RedisTempListItem::vec_items(li3.read_recent::<String>(&conn, None).await),
            vec!["i3", "i2", "i1"]
        );
        li3.clear(&conn).await;
        assert_eq!(
            RedisTempListItem::vec_items(li3.read_recent::<String>(&conn, None).await),
            Vec::<String>::new()
        );

        // Try with arb value, e.g. vec of (i32, String):
        let li4 = RedisTempList::new(
            NS,
            "t4",
            TimeDelta::milliseconds(100),
            TimeDelta::milliseconds(30),
        );
        li4.push(&conn, (1, "a".to_string())).await;
        li4.push(&conn, (2, "b".to_string())).await;
        assert_eq!(
            RedisTempListItem::vec_items(li4.read_recent::<(i32, String)>(&conn, None).await),
            vec![(2, "b".to_string()), (1, "a".to_string())]
        );
        // Confirm delete() with the items themselves works:
        let li_items = li4.read_recent::<(i32, String)>(&conn, None).await;
        assert_eq!(li_items.len(), 2);
        for item in li_items {
            item.delete(&conn).await;
        }
        assert_eq!(li4.read_recent::<(i32, String)>(&conn, None).await.len(), 0);

        // Try with json value:
        let li5 = RedisTempList::new(
            NS,
            "t5",
            TimeDelta::milliseconds(100),
            TimeDelta::milliseconds(30),
        );
        li5.extend(
            &conn,
            vec![
                ExampleObject {
                    a: 1,
                    b: "a".to_string(),
                },
                ExampleObject {
                    a: 2,
                    b: "b".to_string(),
                },
            ],
        )
        .await;
        assert_eq!(
            RedisTempListItem::vec_items(li5.read_recent::<ExampleObject>(&conn, None).await),
            vec![
                ExampleObject {
                    a: 2,
                    b: "b".to_string()
                },
                ExampleObject {
                    a: 1,
                    b: "a".to_string()
                }
            ]
        );

        // Make sure duplicate values don't break the list and are still kept:
        let li6 = RedisTempList::new(
            NS,
            "t6",
            TimeDelta::milliseconds(100),
            TimeDelta::milliseconds(30),
        );
        li6.extend(
            &conn,
            vec![
                "i1".to_string(),
                "i2".to_string(),
                "i1".to_string(),
                "i3".to_string(),
            ],
        )
        .await;
        assert_eq!(
            RedisTempListItem::vec_items(li6.read_recent::<String>(&conn, None).await),
            vec!["i3", "i1", "i2", "i1"]
        );

        Ok(())
    }
}
