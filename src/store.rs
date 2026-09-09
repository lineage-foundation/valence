// store.rs
//
// Storage layer for mailboxes: each mailbox holds a set of opaque JSON blobs
// keyed by a caller-supplied `id` (e.g. a DRUID), with a per-mailbox TTL.
//
// `KvStore` is the storage-agnostic trait handlers depend on. `RedisStore` is
// the production implementation (a Redis hash per mailbox); `MemStore` is an
// in-memory implementation used in unit tests so they don't need a running
// Redis.

use std::collections::{BTreeMap, HashMap};
use std::sync::Mutex;

use async_trait::async_trait;
use redis::aio::ConnectionManager;
use redis::AsyncCommands;
use serde_json::Value;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StoreError {
    /// The underlying store (e.g. Redis) failed.
    Backend(String),
    /// A stored value could not be deserialized back into JSON.
    Corrupt(String),
}

impl std::fmt::Display for StoreError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            StoreError::Backend(msg) => write!(f, "storage backend error: {msg}"),
            StoreError::Corrupt(msg) => write!(f, "stored data is corrupt: {msg}"),
        }
    }
}

impl std::error::Error for StoreError {}

#[async_trait]
pub trait KvStore {
    /// Stores `data` under `id` in `mailbox`, refreshing the mailbox's TTL.
    async fn set(
        &self,
        mailbox: &str,
        id: &str,
        data: &Value,
        ttl_secs: u64,
    ) -> Result<(), StoreError>;

    /// Returns every entry in `mailbox`, keyed by id.
    async fn get_all(&self, mailbox: &str) -> Result<BTreeMap<String, Value>, StoreError>;

    /// Returns a single entry from `mailbox`, if present.
    async fn get_one(&self, mailbox: &str, id: &str) -> Result<Option<Value>, StoreError>;

    /// Deletes a single entry from `mailbox`.
    async fn delete_one(&self, mailbox: &str, id: &str) -> Result<(), StoreError>;

    /// Deletes every entry in `mailbox`.
    async fn delete_all(&self, mailbox: &str) -> Result<(), StoreError>;
}

fn mailbox_key(mailbox: &str) -> String {
    format!("valence:{mailbox}")
}

/// Redis-backed `KvStore`. Each mailbox is a hash at `valence:{mailbox}`,
/// with `id` fields mapping to JSON-encoded values. The whole hash key gets
/// an `EXPIRE` refresh on every `set`, so a mailbox with no writes for
/// `ttl_secs` disappears.
#[derive(Clone)]
pub struct RedisStore {
    conn: ConnectionManager,
}

impl RedisStore {
    pub async fn connect(url: &str) -> Result<Self, StoreError> {
        let client = redis::Client::open(url).map_err(|e| StoreError::Backend(e.to_string()))?;
        let conn = client
            .get_connection_manager()
            .await
            .map_err(|e| StoreError::Backend(e.to_string()))?;
        Ok(RedisStore { conn })
    }
}

#[async_trait]
impl KvStore for RedisStore {
    async fn set(
        &self,
        mailbox: &str,
        id: &str,
        data: &Value,
        ttl_secs: u64,
    ) -> Result<(), StoreError> {
        let key = mailbox_key(mailbox);
        let payload = serde_json::to_string(data).map_err(|e| StoreError::Corrupt(e.to_string()))?;

        let mut conn = self.conn.clone();
        let _: () = conn
            .hset(&key, id, payload)
            .await
            .map_err(|e| StoreError::Backend(e.to_string()))?;
        let _: () = conn
            .expire(&key, ttl_secs as i64)
            .await
            .map_err(|e| StoreError::Backend(e.to_string()))?;
        Ok(())
    }

    async fn get_all(&self, mailbox: &str) -> Result<BTreeMap<String, Value>, StoreError> {
        let key = mailbox_key(mailbox);
        let mut conn = self.conn.clone();
        let raw: HashMap<String, String> = conn
            .hgetall(&key)
            .await
            .map_err(|e| StoreError::Backend(e.to_string()))?;

        let mut out = BTreeMap::new();
        for (id, payload) in raw {
            let value = serde_json::from_str(&payload)
                .map_err(|e| StoreError::Corrupt(format!("id {id}: {e}")))?;
            out.insert(id, value);
        }
        Ok(out)
    }

    async fn get_one(&self, mailbox: &str, id: &str) -> Result<Option<Value>, StoreError> {
        let key = mailbox_key(mailbox);
        let mut conn = self.conn.clone();
        let raw: Option<String> = conn
            .hget(&key, id)
            .await
            .map_err(|e| StoreError::Backend(e.to_string()))?;

        match raw {
            Some(payload) => {
                let value = serde_json::from_str(&payload)
                    .map_err(|e| StoreError::Corrupt(e.to_string()))?;
                Ok(Some(value))
            }
            None => Ok(None),
        }
    }

    async fn delete_one(&self, mailbox: &str, id: &str) -> Result<(), StoreError> {
        let key = mailbox_key(mailbox);
        let mut conn = self.conn.clone();
        let _: () = conn
            .hdel(&key, id)
            .await
            .map_err(|e| StoreError::Backend(e.to_string()))?;
        Ok(())
    }

    async fn delete_all(&self, mailbox: &str) -> Result<(), StoreError> {
        let key = mailbox_key(mailbox);
        let mut conn = self.conn.clone();
        let _: () = conn
            .del(&key)
            .await
            .map_err(|e| StoreError::Backend(e.to_string()))?;
        Ok(())
    }
}

/// In-memory `KvStore`, for unit tests that don't need a running Redis.
/// TTLs are accepted but not enforced.
#[derive(Default)]
pub struct MemStore {
    mailboxes: Mutex<HashMap<String, HashMap<String, Value>>>,
}

impl MemStore {
    pub fn new() -> Self {
        MemStore::default()
    }
}

#[async_trait]
impl KvStore for MemStore {
    async fn set(
        &self,
        mailbox: &str,
        id: &str,
        data: &Value,
        _ttl_secs: u64,
    ) -> Result<(), StoreError> {
        let mut mailboxes = self
            .mailboxes
            .lock()
            .map_err(|_| StoreError::Backend("mutex poisoned".to_string()))?;
        mailboxes
            .entry(mailbox.to_string())
            .or_default()
            .insert(id.to_string(), data.clone());
        Ok(())
    }

    async fn get_all(&self, mailbox: &str) -> Result<BTreeMap<String, Value>, StoreError> {
        let mailboxes = self
            .mailboxes
            .lock()
            .map_err(|_| StoreError::Backend("mutex poisoned".to_string()))?;
        let out = mailboxes
            .get(mailbox)
            .map(|entries| entries.iter().map(|(k, v)| (k.clone(), v.clone())).collect())
            .unwrap_or_default();
        Ok(out)
    }

    async fn get_one(&self, mailbox: &str, id: &str) -> Result<Option<Value>, StoreError> {
        let mailboxes = self
            .mailboxes
            .lock()
            .map_err(|_| StoreError::Backend("mutex poisoned".to_string()))?;
        Ok(mailboxes.get(mailbox).and_then(|entries| entries.get(id)).cloned())
    }

    async fn delete_one(&self, mailbox: &str, id: &str) -> Result<(), StoreError> {
        let mut mailboxes = self
            .mailboxes
            .lock()
            .map_err(|_| StoreError::Backend("mutex poisoned".to_string()))?;
        if let Some(entries) = mailboxes.get_mut(mailbox) {
            entries.remove(id);
        }
        Ok(())
    }

    async fn delete_all(&self, mailbox: &str) -> Result<(), StoreError> {
        let mut mailboxes = self
            .mailboxes
            .lock()
            .map_err(|_| StoreError::Backend("mutex poisoned".to_string()))?;
        mailboxes.remove(mailbox);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[tokio::test]
    async fn mem_store_set_and_get_one() {
        let store = MemStore::new();
        store.set("mbox", "id1", &json!({"a": 1}), 60).await.unwrap();

        let got = store.get_one("mbox", "id1").await.unwrap();
        assert_eq!(got, Some(json!({"a": 1})));
    }

    #[tokio::test]
    async fn mem_store_get_one_missing() {
        let store = MemStore::new();
        let got = store.get_one("mbox", "missing").await.unwrap();
        assert_eq!(got, None);
    }

    #[tokio::test]
    async fn mem_store_get_all_empty() {
        let store = MemStore::new();
        let got = store.get_all("mbox").await.unwrap();
        assert!(got.is_empty());
    }

    #[tokio::test]
    async fn mem_store_delete_one() {
        let store = MemStore::new();
        store.set("mbox", "id1", &json!(1), 60).await.unwrap();
        store.set("mbox", "id2", &json!(2), 60).await.unwrap();

        store.delete_one("mbox", "id1").await.unwrap();

        let all = store.get_all("mbox").await.unwrap();
        assert_eq!(all.len(), 1);
        assert!(all.contains_key("id2"));
    }

    #[tokio::test]
    async fn mem_store_delete_all() {
        let store = MemStore::new();
        store.set("mbox", "id1", &json!(1), 60).await.unwrap();
        store.set("mbox", "id2", &json!(2), 60).await.unwrap();

        store.delete_all("mbox").await.unwrap();

        let all = store.get_all("mbox").await.unwrap();
        assert!(all.is_empty());
    }
}
