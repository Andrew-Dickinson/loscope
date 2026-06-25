use std::any::Any;
use std::collections::HashMap;
use std::sync::Mutex;
use std::time::Duration;
use chrono::{DateTime, Utc};
use derive_more::Display;
use derive_new::new;
use rslock::LockManager;

pub trait AcquiredLock: Send + Sync + 'static {
    fn as_any(self: Box<Self>) -> Box<dyn Any + Send>;
}

#[async_trait]
pub trait DistributedMutexManager: Send + Sync {
    async fn lock(&self, key: &str, ttl: Duration) -> Result<Box<dyn AcquiredLock>, LockError>;
    async fn unlock(&self, lock: Box<dyn AcquiredLock>);
}

#[derive(Debug, Display)]
pub enum LockError {
    ResourceBusy,
    AcquisitionTimeout(String),
    UpstreamError(String),
}

impl From<rslock::LockError> for LockError {
    fn from(e: rslock::LockError) -> Self {
        match e {
            rslock::LockError::Unavailable => LockError::ResourceBusy,
            _ => LockError::UpstreamError(e.to_string()),
        }
    }
}

struct RsLock(rslock::Lock);

impl AcquiredLock for RsLock {
    fn as_any(self: Box<Self>) -> Box<dyn Any + Send> { self }
}

#[derive(new)]
pub struct RedisDistributedMutexManager {
    lock_manager: LockManager,
}

#[async_trait]
impl DistributedMutexManager for RedisDistributedMutexManager {
    async fn lock(&self, key: &str, ttl: Duration) -> Result<Box<dyn AcquiredLock>, LockError> {
        Ok(Box::new(RsLock(self.lock_manager.lock(key, ttl).await?)))
    }

    async fn unlock(&self, lock: Box<dyn AcquiredLock>) {
        if let Ok(rs_lock) = lock.as_any().downcast::<RsLock>() {
            self.lock_manager.unlock(&rs_lock.0).await;
        } else {
            panic!("Invalid type passed to unlock, expected RsLock");
        }
    }
}

#[derive(Clone)]
pub struct InMemoryLock {
    key: String,
    ttl: DateTime<Utc>,
    salt: u32,
}

impl AcquiredLock for InMemoryLock {
    fn as_any(self: Box<Self>) -> Box<dyn Any + Send> { self }
}

pub struct InMemoryMutexManager {
    locks: Mutex<HashMap<String, InMemoryLock>>,
}

impl Default for InMemoryMutexManager {
    fn default() -> Self {
        InMemoryMutexManager {
            locks: Mutex::new(HashMap::new()),
        }
    }
}

#[async_trait]
impl DistributedMutexManager for InMemoryMutexManager {
    async fn lock(&self, key: &str, ttl: Duration) -> Result<Box<dyn AcquiredLock>, LockError> {
        let mut locks_ref = self.locks.lock().unwrap();

        let existing_lock = locks_ref.get(key);
        if let Some(existing_lock) = existing_lock {
            if existing_lock.ttl >= Utc::now() {
                return Err(LockError::ResourceBusy);
            }
        }

        let new_lock = InMemoryLock {
            key: key.to_string(),
            ttl: Utc::now() + ttl,
            salt: 0,
        };
        locks_ref.insert(key.to_string(), new_lock.clone());
        Ok(Box::new(new_lock))
    }

    async fn unlock(&self, lock: Box<dyn AcquiredLock>) {
        if let Ok(lock) = lock.as_any().downcast::<InMemoryLock>() {
            let mut locks_ref = self.locks.lock().unwrap();
            if let Some(existing) = locks_ref.get(&lock.key) {
                if existing.ttl == lock.ttl && existing.salt == lock.salt {
                    locks_ref.remove(&lock.key);
                }
            }
        } else {
            panic!("Invalid type passed to unlock, expected InMemoryLock");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn lock_succeeds() {
        let mgr = InMemoryMutexManager::default();
        let result = mgr.lock("key", Duration::from_secs(60)).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn lock_same_key_twice_returns_resource_busy() {
        let mgr = InMemoryMutexManager::default();
        let _first = mgr.lock("key", Duration::from_secs(60)).await.unwrap();
        let second = mgr.lock("key", Duration::from_secs(60)).await;
        assert!(matches!(second, Err(LockError::ResourceBusy)));
    }

    #[tokio::test]
    async fn lock_different_keys_both_succeed() {
        let mgr = InMemoryMutexManager::default();
        let r1 = mgr.lock("key-a", Duration::from_secs(60)).await;
        let r2 = mgr.lock("key-b", Duration::from_secs(60)).await;
        assert!(r1.is_ok());
        assert!(r2.is_ok());
    }

    #[tokio::test]
    async fn unlock_allows_relock() {
        let mgr = InMemoryMutexManager::default();
        let lock = mgr.lock("key", Duration::from_secs(60)).await.unwrap();
        mgr.unlock(lock).await;
        let relock = mgr.lock("key", Duration::from_secs(60)).await;
        assert!(relock.is_ok());
    }

    #[tokio::test]
    async fn unlock_only_removes_matching_lock() {
        let mgr = InMemoryMutexManager::default();

        // Hold key-a and key-b, then unlock key-b only.
        // key-a should remain locked.
        let lock_a = mgr.lock("key-a", Duration::from_secs(60)).await.unwrap();
        let lock_b = mgr.lock("key-b", Duration::from_secs(60)).await.unwrap();

        mgr.unlock(lock_b).await;

        assert!(matches!(
            mgr.lock("key-a", Duration::from_secs(60)).await,
            Err(LockError::ResourceBusy)
        ));
        assert!(mgr.lock("key-b", Duration::from_secs(60)).await.is_ok());

        mgr.unlock(lock_a).await;
    }

    #[tokio::test]
    async fn expired_lock_can_be_reacquired() {
        let mgr = InMemoryMutexManager::default();
        let _lock = mgr.lock("key", Duration::from_millis(1)).await.unwrap();

        tokio::time::sleep(Duration::from_millis(5)).await;

        let relock = mgr.lock("key", Duration::from_secs(60)).await;
        assert!(relock.is_ok());
    }

    #[tokio::test]
    async fn stale_token_unlock_does_not_remove_new_lock() {
        // Acquire with 1ms TTL, let it expire, re-acquire with a fresh token,
        // then unlock with the stale token. The fresh lock must not be removed.
        let mgr = InMemoryMutexManager::default();
        let stale = mgr.lock("key", Duration::from_millis(1)).await.unwrap();

        tokio::time::sleep(Duration::from_millis(5)).await;

        let _fresh = mgr.lock("key", Duration::from_secs(60)).await.unwrap();
        mgr.unlock(stale).await;

        assert!(matches!(
            mgr.lock("key", Duration::from_secs(60)).await,
            Err(LockError::ResourceBusy)
        ));
    }
}