use std::{sync::atomic::AtomicI64, time::Duration};

use super::{batch::*, RedisJson};
#[cfg(test)]
use crate::prelude::*;

/// Connect up to a magic redis list that:
/// - Has an expiry on the list itself, resetting on each read or write. (each change lives again for `expire_after` time)
/// - Each item in the list has it's own expiry, so the list is always clean of old items.
/// - Each item has a generated unique key, this key can be used to update or delete specific items directly.
/// - Returned items are returned newest to oldest (for all intents and purposes, slightly more complicated).
/// This makes this distributed data structure perfect for e.g.:
/// - recent/temporary logs/events of any sort.
/// - pending actions, that can be updated in-place by the creator, but read as part of a list by a viewer etc.
pub struct RedisTempList<'a> {
    redis: &'a super::Redis,
    /// The namespace of the list in redis
    pub namespace: &'static str,
    /// The key of the list in redis
    pub key: String,
    /// If the list hasn't been read or written to in this time, it will be expired.
    pub expire_after: Duration,

    /// Used to prevent overlap between push() and extend() calls using the same ts by accident.
    last_extension_ts_millis: AtomicI64,
}

/// A managed list entry in redis that will:
/// - Auto expire on inactivity
/// - Return items newest to oldest
/// - Items are set with their own ttl, so the members themselves expire separate from the full list
impl<'a> RedisTempList<'a> {
    pub(crate) fn new(
        redis: &'a super::Redis,
        namespace: &'static str,
        key: String,
        expire_after: Duration,
    ) -> Self {
        Self {
            redis,
            namespace,
            key,
            expire_after,
            last_extension_ts_millis: AtomicI64::new(0),
        }
    }

    /// The score should be the utc timestamp to expire:
    async fn extend_inner<T: serde::Serialize + for<'b> serde::Deserialize<'b>>(
        &self,
        items: impl IntoIterator<Item = T>,
        ttl: Duration,
    ) -> Option<Vec<String>> {
        let score = (chrono::Utc::now() + ttl).timestamp_millis();

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

        let result: Option<()> = self
            .redis
            .conn()
            .batch()
            // Add the uids to the main set, the command will auto update the set's ttl (self.expire_after) given it's been updated.
            .zadd_multi(
                self.namespace,
                &self.key,
                Some(self.expire_after), // This will auto reset the expire time of the list as a whole
                items_with_uids
                    .iter()
                    .map(|(uid, _)| (score, uid))
                    .collect::<Vec<_>>()
                    .as_slice(),
            )
            // Now store the values themselves as normal redis keys with the same ttl: (these are normal ttls that auto clean up)
            .mset(
                self.namespace,
                items_with_uids
                    .into_iter()
                    .map(|(uid, item)| (uid, RedisJson(item))),
                Some(ttl),
            )
            // Cleanup old members that have now expired:
            // (set member expiry is a logical process, not currently part of redis but could be soon)
            // https://github.com/redis/redis/issues/135#issuecomment-2361996
            // https://github.com/redis/redis/pull/13172
            .zremrangebyscore(
                self.namespace,
                &self.key,
                i64::MIN,
                chrono::Utc::now().timestamp_millis(),
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
    /// - Autoreset list's expire time to self.expire_after from now
    /// - Clean up expired list items
    ///
    /// Returns:
    /// - Some(String): The uid of the item added, this can be used to update or delete the item directly.
    /// - None: Something went wrong and the item wasn't added correctly.
    pub async fn push<T: serde::Serialize + for<'b> serde::Deserialize<'b>>(
        &self,
        item: T,
        ttl: Duration,
    ) -> Option<String> {
        let uids = self.extend_inner(std::iter::once(item), ttl).await;
        if let Some(uids) = uids {
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
        }
    }

    /// Add multiple items to the sorted list.
    /// Purposely using one ttl for all, should think about why if you're needing to set different ttls to items you're adding together!
    ///
    /// This will also:
    /// - Autoreset list's expire time to self.expire_after from now
    /// - Clean up expired list items
    ///
    /// Returns:
    /// - Some(Vec<String>): The uids of the items added, these can be used to update or delete the items directly.
    /// - None: Something went wrong and the items weren't added correctly.
    pub async fn extend<T: serde::Serialize + for<'b> serde::Deserialize<'b>>(
        &self,
        items: impl IntoIterator<Item = T>,
        ttl: Duration,
    ) -> Option<Vec<String>> {
        self.extend_inner(items, ttl).await
    }

    /// Underlying of [`RedisTempList::read_multi`], but returns the (i64: ttl, String: item key, T: item) rather than just the T.
    /// Use this when you want to be able to update/delete individual items with the result.
    pub async fn read_multi_with_info<T: serde::Serialize + for<'b> serde::Deserialize<'b>>(
        &self,
        limit: Option<isize>,
    ) -> Vec<(i64, String, T)> {
        let mut conn = self.redis.conn();

        // NOTE: because of the separation between the root list, and the values themselves as a separate redis keys, 2 calls are needed.
        // 1. Get the uids from the list
        // 2. Get the values from the uids
        // This cannot be done without a round-trip through a script because redis requires all keys used in scripts to be known ahead of time (using KEYS), so can't use that.

        let item_info: Option<Vec<(Option<String>, i64)>> = conn
            .batch()
            // NOTE: cleaning up first as don't want these to be included in the read.
            // Cleanup old members that have now expired:
            // (member expiry is a logical process, not currently part of redis but could be soon)
            // https://github.com/redis/redis/issues/135#issuecomment-2361996
            // https://github.com/redis/redis/pull/13172
            .zremrangebyscore(
                self.namespace,
                &self.key,
                i64::MIN,
                chrono::Utc::now().timestamp_millis(),
            )
            .zrangebyscore::<String>(self.namespace, &self.key, i64::MIN, i64::MAX, limit)
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
                let items: Option<Vec<Option<RedisJson<T>>>> = conn
                    .batch()
                    .mget(
                        self.namespace,
                        &item_info.iter().map(|(uid, _)| uid).collect::<Vec<_>>(),
                    )
                    // Unlike our zadd during setting, need to manually refresh the expire time of the list here:
                    .expire(self.namespace, &self.key, self.expire_after)
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

    /// Read multiple items from the list, ordered from latest ttl to oldest.
    /// When the ttl is the same, the items are returned newest to oldest.
    ///
    /// This will also:
    /// - Autoreset list's expire time to self.expire_after from now
    /// - Clean up expired list items
    ///
    /// Returns:
    /// - Vec<T>: The items in the list from newest to oldest up to the provided limit (if any).
    pub async fn read_multi<T: serde::Serialize + for<'b> serde::Deserialize<'b>>(
        &self,
        limit: Option<isize>,
    ) -> Vec<T> {
        self.read_multi_with_info(limit)
            .await
            .into_iter()
            .map(|(_, _, item)| item)
            .collect()
    }

    /// Read a specific item given it's uid.
    ///
    /// This will also:
    /// - Autoreset list's expire time to self.expire_after from now
    /// - Clean up expired list items
    ///
    /// Returns:
    /// - Some(T): The item found with the uid.
    /// - None: The item wasn't found, or something went wrong during the read.
    pub async fn read<T: serde::Serialize + for<'b> serde::Deserialize<'b>>(
        &self,
        uid: &str,
    ) -> Option<T> {
        let item: Option<Option<RedisJson<T>>> = self
            .redis
            .conn()
            .batch()
            // NOTE: cleaning up first as don't want these to be included in the read.
            // Cleanup old members that have now expired:
            // (member expiry is a logical process, not currently part of redis but could be soon)
            // https://github.com/redis/redis/issues/135#issuecomment-2361996
            // https://github.com/redis/redis/pull/13172
            .zremrangebyscore(
                self.namespace,
                &self.key,
                i64::MIN,
                chrono::Utc::now().timestamp_millis(),
            )
            .get(self.namespace, uid)
            // Unlike our zadd during setting, need to manually refresh the expire time of the list here:
            .expire(self.namespace, &self.key, self.expire_after)
            .fire()
            .await;
        if let Some(item) = item.flatten() {
            Some(item.0)
        } else {
            None
        }
    }

    /// Delete a specific item given it's uid.
    ///
    /// This will also:
    /// - Autoreset list's expire time to self.expire_after from now
    /// - Clean up expired list items
    pub async fn delete(&self, uid: &str) {
        self.redis
            .conn()
            .batch()
            // NOTE: cleaning up first as don't want these to be included in the read.
            // Cleanup old members that have now expired:
            // (member expiry is a logical process, not currently part of redis but could be soon)
            // https://github.com/redis/redis/issues/135#issuecomment-2361996
            // https://github.com/redis/redis/pull/13172
            .zremrangebyscore(
                self.namespace,
                &self.key,
                i64::MIN,
                chrono::Utc::now().timestamp_millis(),
            )
            .zrem(self.namespace, &self.key, uid)
            // Unlike our zadd during setting, need to manually refresh the expire time of the list here:
            .expire(self.namespace, &self.key, self.expire_after)
            .fire()
            .await;
    }

    /// Update a specific item given it's uid.
    ///
    /// This will also:
    /// - Autoreset list's expire time to self.expire_after from now
    /// - Clean up expired list items
    pub async fn update<T: serde::Serialize + for<'b> serde::Deserialize<'b>>(
        &self,
        uid: &str,
        item: T,
        ttl: Duration,
    ) {
        let new_score = (chrono::Utc::now() + ttl).timestamp_millis();

        self.redis
            .conn()
            .batch()
            // This will update the uid's score/ttl, redis will automatically see it already existed (if it hadn't already expired) and update it.
            // It will also implicitly renew the list's ttl.
            .zadd(
                self.namespace,
                &self.key,
                Some(self.expire_after),
                new_score,
                uid,
            )
            // Update the value itself, which is stored under the uid as a normal redis value:
            .set(self.namespace, uid, RedisJson(item), Some(ttl))
            // Cleanup old members that have now expired:
            // (member expiry is a logical process, not currently part of redis but could be soon)
            // https://github.com/redis/redis/issues/135#issuecomment-2361996
            // https://github.com/redis/redis/pull/13172
            .zremrangebyscore(
                self.namespace,
                &self.key,
                i64::MIN,
                chrono::Utc::now().timestamp_millis(),
            )
            .fire()
            .await;
    }

    /// Clear all the items in the list.
    /// (by just deleting the list itself, stored values will still live until their ttl but used a random uid so no conflicts)
    pub async fn clear(&self) {
        self.redis
            .conn()
            .batch()
            .clear(self.namespace, std::iter::once(self.key.as_str()))
            .fire()
            .await;
    }
}

#[cfg(test)]
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
pub struct ExampleObject {
    pub a: i32,
    pub b: String,
}

/// Run by the main tester that spawns up a redis process.
#[cfg(test)]
pub async fn redis_temp_list_tests(r: &super::Redis) -> Result<(), AnyErr> {
    // Just checking the object is normal: (from upstream)
    fn is_normal<T: Sized + Send + Sync + Unpin>() {}
    is_normal::<RedisTempList>();

    static NS: &str = "templist_tests";

    let li1 = r.templist(NS, "t1", Duration::from_millis(100));
    li1.extend(
        vec!["i1".to_string(), "i2".to_string(), "i3".to_string()],
        Duration::from_millis(30),
    )
    .await;
    li1.push("i4".to_string(), Duration::from_millis(10)).await;
    li1.push("i5".to_string(), Duration::from_millis(50)).await;
    // Keys are ordered from latest ttl to oldest, then reverse lexicographically when the ttl is the same:
    assert_eq!(
        li1.read_multi::<String>(None).await,
        vec!["i5", "i3", "i2", "i1", "i4"]
    );
    // Read with limit, should be keeping newest and dropping oldest:
    assert_eq!(
        li1.read_multi::<String>(Some(3)).await,
        vec!["i5", "i3", "i2"]
    );
    tokio::time::sleep(Duration::from_millis(20)).await;
    // i4 should be expired, it had a ttl of 10ms:
    assert_eq!(
        li1.read_multi::<String>(None).await,
        vec!["i5", "i3", "i2", "i1"]
    );
    tokio::time::sleep(Duration::from_millis(20)).await;
    // i1,2,3 should be expired, they had a ttl of 30ms:
    assert_eq!(li1.read_multi::<String>(None).await, vec!["i5"]);
    tokio::time::sleep(Duration::from_millis(20)).await;
    // i5 should be expired, it had a ttl of 50ms:
    assert_eq!(li1.read_multi::<String>(None).await, Vec::<String>::new());

    // Put a new item in there with a nice long ttl:
    let uid = li1
        .push("i6".to_string(), Duration::from_millis(1000))
        .await
        .unwrap();
    // Li should still be there after 80ms, as the last push should have updated the list's ttl:
    tokio::time::sleep(Duration::from_millis(80)).await;
    assert_eq!(li1.read_multi::<String>(None).await, vec!["i6"]);
    // Above read_multi should have also updated the list's ttl, so should be there after another 80ms:
    tokio::time::sleep(Duration::from_millis(80)).await;
    assert_eq!(li1.read::<String>(&uid).await, Some("i6".to_string()));
    // Above direct read should have also updated the list's ttl, so should be there after another 80ms:
    tokio::time::sleep(Duration::from_millis(80)).await;
    assert_eq!(li1.read::<String>(&uid).await, Some("i6".to_string()));
    let uid_i7 = li1
        .push("i7".to_string(), Duration::from_millis(1000))
        .await
        .unwrap();
    tokio::time::sleep(Duration::from_millis(80)).await;
    li1.delete(&uid).await;
    // Above delete should have updated the list's ttl, so should be there after another 80ms:
    tokio::time::sleep(Duration::from_millis(80)).await;
    assert_eq!(li1.read_multi::<String>(None).await, vec!["i7"]);
    tokio::time::sleep(Duration::from_millis(80)).await;
    li1.update(
        &uid_i7,
        "i7-updated".to_string(),
        Duration::from_millis(1000),
    )
    .await;
    // Above update should have updated the list's ttl, so should be there after another 80ms:
    tokio::time::sleep(Duration::from_millis(80)).await;
    assert_eq!(
        li1.read::<String>(&uid_i7).await,
        Some("i7-updated".to_string())
    );
    // When no reads or writes, the list should expire after 100ms:
    tokio::time::sleep(Duration::from_millis(110)).await;
    assert_eq!(li1.read_multi::<String>(None).await, Vec::<String>::new());

    // Make sure manual clear() works on a new list too:
    let li2 = r.templist(NS, "t2", Duration::from_millis(100));
    li2.extend(
        vec!["i1".to_string(), "i2".to_string(), "i3".to_string()],
        Duration::from_millis(30),
    )
    .await;
    assert_eq!(li2.read_multi::<String>(None).await, vec!["i3", "i2", "i1"]);
    li2.clear().await;
    assert_eq!(li2.read_multi::<String>(None).await, Vec::<String>::new());

    // Try with arb value, e.g. vec of (i32, String):
    let li3 = r.templist(NS, "t3", Duration::from_millis(100));
    li3.push((1, "a".to_string()), Duration::from_millis(30))
        .await;
    li3.push((2, "b".to_string()), Duration::from_millis(30))
        .await;
    assert_eq!(
        li3.read_multi::<(i32, String)>(None).await,
        vec![(2, "b".to_string()), (1, "a".to_string())]
    );

    // Try with json value:
    let li4 = r.templist(NS, "t4", Duration::from_millis(100));
    li4.extend(
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
        Duration::from_millis(30),
    )
    .await;
    assert_eq!(
        li4.read_multi::<ExampleObject>(None).await,
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
    let li5 = r.templist(NS, "t5", Duration::from_millis(100));
    li5.extend(
        vec![
            "i1".to_string(),
            "i2".to_string(),
            "i1".to_string(),
            "i3".to_string(),
        ],
        Duration::from_millis(30),
    )
    .await;
    assert_eq!(
        li5.read_multi::<String>(None).await,
        vec!["i3", "i1", "i2", "i1"]
    );

    Ok(())
}
