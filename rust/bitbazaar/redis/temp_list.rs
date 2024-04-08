use std::time::Duration;

use redis::{FromRedisValue, ToRedisArgs};

use super::batch::*;
#[cfg(test)]
use crate::prelude::*;

/// A magic redis list that can itself be auto expired if not used in a while, but also each of it's items can be configured to expire on their own intervals.
/// This makes this data structure perfect for e.g. recent/temporary logs/events of any sort.
pub struct RedisTempList<'a> {
    redis: &'a super::Redis,
    /// The namespace of the list in redis
    pub namespace: &'static str,
    /// The key of the list in redis
    pub key: String,
    /// If the list hasn't been read or written to in this time, it will be expired.
    pub expire_after: Duration,
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
        }
    }

    /// The score should be the utc timestamp to expire:
    async fn extend_inner<T: ToRedisArgs>(
        &self,
        items: impl IntoIterator<Item = T>,
        ttl: Duration,
    ) {
        let score = (chrono::Utc::now() + ttl).timestamp_millis();
        // Add the new entries, the command will auto update the sets ttl (self.expire_after) given it's been updated.
        self.redis
            .conn()
            .batch()
            .zadd_multi(
                self.namespace,
                &self.key,
                Some(self.expire_after), // This will auto reset the expire time of the list as a whole
                items
                    .into_iter()
                    .map(|item| (score, item))
                    .collect::<Vec<_>>()
                    .as_slice(),
            )
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

    /// Add a new item to the sorted list.
    ///
    /// This will also:
    /// - Autoreset list's expire time to self.expire_after from now
    /// - Clean up expired list items
    pub async fn push<T: ToRedisArgs>(&self, item: T, ttl: Duration) {
        self.extend_inner(std::iter::once(item), ttl).await;
    }

    /// Add multiple items to the sorted list.
    /// Purposely using one ttl for all, should think about why if you're needing to set different ttls to items you're adding together!
    ///
    /// This will also:
    /// - Autoreset list's expire time to self.expire_after from now
    /// - Clean up expired list items
    pub async fn extend<T: ToRedisArgs>(&self, items: impl IntoIterator<Item = T>, ttl: Duration) {
        self.extend_inner(items, ttl).await;
    }

    /// Read items from the list, ordered from latest ttl to oldest.
    /// When two keys have the same ttl, they are ordered reverse-lexicographically. (side-effect of reversal of ttl ordering)
    ///
    /// This will also:
    /// - Autoreset list's expire time to self.expire_after from now
    /// - Clean up expired list items
    pub async fn read<T: FromRedisValue>(&self, limit: Option<isize>) -> Vec<T> {
        let items: Option<Vec<(Option<T>, i64)>> = self
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
            .zrangebyscore(self.namespace, &self.key, i64::MIN, i64::MAX, limit)
            // Unlike zadd, zrangebyscore doesn't update the expire time of the set automatically:
            .expire(self.namespace, &self.key, self.expire_after)
            .fire()
            .await;

        if let Some(items) = items {
            // Items will be None if couldn't be decoded to T, so filter those out:
            // Also need to remove the scores as they don't matter for this usecase:
            items
                .into_iter()
                .filter_map(|(item, _score)| item)
                .collect()
        } else {
            vec![]
        }
    }

    /// Clear the set, by clearing the set's key.
    pub async fn clear(&self) {
        self.redis
            .conn()
            .batch()
            .clear(self.namespace, std::iter::once(self.key.as_str()))
            .fire()
            .await;
    }
}

/// Run by the main tester that spawns up a redis process.
#[cfg(test)]
pub async fn redis_temp_list_tests(r: &super::Redis) -> Result<(), AnyErr> {
    // Just checking the object is normal: (from upstream)
    fn is_normal<T: Sized + Send + Sync + Unpin>() {}
    is_normal::<RedisTempList>();

    static NS: &str = "templist_tests";

    let li1 = r.templist(NS, "t1", Duration::from_millis(100));
    li1.extend(vec!["i1", "i2", "i3"], Duration::from_millis(30))
        .await;
    li1.push("i4", Duration::from_millis(10)).await;
    li1.push("i5", Duration::from_millis(50)).await;

    // Keys are ordered from latest ttl to oldest, then reverse lexicographically when the ttl is the same:
    assert_eq!(
        li1.read::<String>(None).await,
        vec!["i5", "i3", "i2", "i1", "i4"]
    );
    // Read with limit, should be keeping newest and dropping oldest:
    assert_eq!(li1.read::<String>(Some(3)).await, vec!["i5", "i3", "i2"]);

    tokio::time::sleep(Duration::from_millis(20)).await;
    // i4 should be expired, it had a ttl of 10ms:
    assert_eq!(li1.read::<String>(None).await, vec!["i5", "i3", "i2", "i1"]);
    tokio::time::sleep(Duration::from_millis(20)).await;
    // i1,2,3 should be expired, they had a ttl of 30ms:
    assert_eq!(li1.read::<String>(None).await, vec!["i5"]);
    tokio::time::sleep(Duration::from_millis(20)).await;
    // i5 should be expired, it had a ttl of 50ms:
    assert_eq!(li1.read::<String>(None).await, Vec::<String>::new());

    // Put a new item in there with a nice long ttl:
    li1.push("i6", Duration::from_millis(500)).await;
    // Li should still be there after 80ms, as the last push should have updated the list's ttl:
    tokio::time::sleep(Duration::from_millis(80)).await;
    assert_eq!(li1.read::<String>(None).await, vec!["i6"]);
    // This read should have also updated the list's ttl, so should be there after another 80ms:
    tokio::time::sleep(Duration::from_millis(80)).await;
    assert_eq!(li1.read::<String>(None).await, vec!["i6"]);
    // When no reads or writes, the list should expire after 100ms:
    tokio::time::sleep(Duration::from_millis(110)).await;
    assert_eq!(li1.read::<String>(None).await, Vec::<String>::new());

    // Make sure manual clear() works on a new list too:
    let li2 = r.templist(NS, "t2", Duration::from_millis(100));
    li2.extend(vec!["i1", "i2", "i3"], Duration::from_millis(30))
        .await;
    assert_eq!(li2.read::<String>(None).await, vec!["i3", "i2", "i1"]);
    li2.clear().await;
    assert_eq!(li2.read::<String>(None).await, Vec::<String>::new());

    Ok(())
}
