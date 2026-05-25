use crate::types::errors::AssetErr;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use uuid::Uuid;

pub trait ValueStore {
    fn put(&self, key: String, value: Vec<u8>) -> Result<(), AssetErr>;
    fn get(&self, key: String) -> Result<Vec<u8>, AssetErr>;
}

// TODO: Implement redis-based shared cache for cross-container state

pub struct InMemoryValueStore {
    map: Mutex<HashMap<String, Vec<u8>>>,
}

impl InMemoryValueStore {
    pub fn new() -> Self {
        Self {
            map: Mutex::new(HashMap::new()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn get_after_put_returns_value() {
        let store = InMemoryValueStore::new();
        store.put("k".into(), b"hello".to_vec()).unwrap();
        assert_eq!(store.get("k".into()).unwrap(), b"hello");
    }

    #[test]
    fn get_missing_key_returns_not_found() {
        let store = InMemoryValueStore::new();
        assert!(matches!(
            store.get("missing".into()),
            Err(AssetErr::AssetNotFound(_))
        ));
    }

    #[test]
    fn put_overwrites_existing_key() {
        let store = InMemoryValueStore::new();
        store.put("k".into(), b"first".to_vec()).unwrap();
        store.put("k".into(), b"second".to_vec()).unwrap();
        assert_eq!(store.get("k".into()).unwrap(), b"second");
    }

    #[test]
    fn keys_are_independent() {
        let store = InMemoryValueStore::new();
        store.put("a".into(), b"1".to_vec()).unwrap();
        store.put("b".into(), b"2".to_vec()).unwrap();
        assert_eq!(store.get("a".into()).unwrap(), b"1");
        assert_eq!(store.get("b".into()).unwrap(), b"2");
    }

    #[test]
    fn get_returns_cloned_value() {
        let store = InMemoryValueStore::new();
        store.put("k".into(), b"data".to_vec()).unwrap();
        let v1 = store.get("k".into()).unwrap();
        let v2 = store.get("k".into()).unwrap();
        assert_eq!(v1, v2);
    }
}

impl ValueStore for InMemoryValueStore {
    fn put(&self, key: String, value: Vec<u8>) -> Result<(), AssetErr> {
        self.map
            .lock()
            .or_else(|e| {
                Err(AssetErr::AssetDownloadError(format!(
                    "Failed to aquire lock putting {key}: {:?}",
                    e
                )))
            })?
            .insert(key, value);
        Ok(())
    }

    fn get(&self, key: String) -> Result<Vec<u8>, AssetErr> {
        self.map
            .lock()
            .or_else(|e| {
                Err(AssetErr::AssetDownloadError(format!(
                    "Failed to aquire lock putting {key}: {:?}",
                    e
                )))
            })?
            .get(&key)
            .ok_or(AssetErr::AssetNotFound(format!("Key {} not found", key)))
            .cloned()
    }
}
