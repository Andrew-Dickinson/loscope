use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use uuid::Uuid;
use crate::types::errors::AssetErr;

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

impl ValueStore for InMemoryValueStore {
    fn put(&self, key: String, value: Vec<u8>) -> Result<(), AssetErr> {
        self.map.lock()
            .or_else(|e| Err(
                AssetErr::AssetDownloadError(
                    format!("Failed to aquire lock putting {key}: {:?}", e)
                )
            ))?
            .insert(key, value);
        Ok(())
    }

    fn get(&self, key: String) -> Result<Vec<u8>, AssetErr> {
        self.map.lock()
            .or_else(|e| Err(
                AssetErr::AssetDownloadError(
                    format!("Failed to aquire lock putting {key}: {:?}", e)
                )
            ))?
            .get(&key).ok_or(
                AssetErr::AssetNotFound(format!("Key {} not found", key))
            ).cloned()
    }
}