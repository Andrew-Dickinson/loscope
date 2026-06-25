use std::sync::Arc;
use std::time::Duration;
use tokio::time::sleep;
use crate::providers::backends::distributed_mutex::{AcquiredLock, DistributedMutexManager, LockError};

const MUTEX_TTL: Duration = Duration::from_secs(60);
pub(crate) const MAX_LOCK_AQUIRE_ATTEMPTS: usize = 5;
pub(crate) const LOCK_AQUIRE_SLEEP_WAIT: Duration = Duration::from_secs(5);

pub struct LocalPathLock<'a> {
    local_cache_id: &'a str,
    intra_cache_path: &'a str,
    lock_manager: Arc<dyn DistributedMutexManager>,
    lock: Option<Box<dyn AcquiredLock>>,
}

impl<'a> LocalPathLock<'a> {
    pub fn new(intra_cache_path: &'a str, local_cache_id: &'a str, lock_manager: Arc<dyn DistributedMutexManager>) -> Self {
        LocalPathLock {
            intra_cache_path,
            local_cache_id,
            lock_manager,
            lock: None,
        }
    }

    pub async fn aquire(&mut self) -> Result<(), LockError> {
        self.lock = Some(
            self.lock_manager.lock(
                format!("{}_{}", self.local_cache_id, self.intra_cache_path).as_str(),
                MUTEX_TTL,
            ).await?
        );
        Ok(())
    }

    pub async fn aquire_wait(&mut self) -> Result<(), LockError> {
        let mut last_result: Option<LockError> = None;
        assert!(MAX_LOCK_AQUIRE_ATTEMPTS > 0);
        for i in 0..MAX_LOCK_AQUIRE_ATTEMPTS {
            if i > 0 {
                println!("Lock on {} unavailable, trying again...", self.intra_cache_path);
                sleep(LOCK_AQUIRE_SLEEP_WAIT).await;
            }
            match self.aquire().await {
                Ok(_) => return Ok(()),
                Err(e) => { last_result = Some(e); },
            }
        }

        // Safety: last_result is always initialized because MAX_LOCK_AQUIRE_ATTEMPTS > 0
        // and all other paths return before this point
        Err(LockError::AcquisitionTimeout(
            format!(
                "Timeout exceeded after {} attempts, error: {}",
                MAX_LOCK_AQUIRE_ATTEMPTS,
                last_result.unwrap()
            )
        ))
    }
}

impl<'a> Drop for LocalPathLock<'a> {
    fn drop(&mut self) {
        if let Some(lock) = self.lock.take() {
            let lock_manager = Arc::clone(&self.lock_manager);
            tokio::spawn(async move {
                lock_manager.unlock(lock).await;
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::providers::backends::distributed_mutex::{InMemoryMutexManager, LockError};
    use std::any::Any;
    use std::sync::Mutex;

    fn mgr() -> Arc<dyn DistributedMutexManager> {
        Arc::new(InMemoryMutexManager::default())
    }

    // A mutex manager that always returns ResourceBusy.
    struct AlwaysBusyMutexManager;

    #[async_trait]
    impl DistributedMutexManager for AlwaysBusyMutexManager {
        async fn lock(&self, _: &str, _: Duration) -> Result<Box<dyn AcquiredLock>, LockError> {
            Err(LockError::ResourceBusy)
        }
        async fn unlock(&self, _: Box<dyn AcquiredLock>) {}
    }

    // A mutex manager that fails the first `fails` calls then succeeds.
    struct CountdownMutexManager {
        fails_remaining: Mutex<usize>,
    }

    impl CountdownMutexManager {
        fn new(fails: usize) -> Self {
            CountdownMutexManager { fails_remaining: Mutex::new(fails) }
        }
    }

    struct NoopLock;
    impl AcquiredLock for NoopLock {
        fn as_any(self: Box<Self>) -> Box<dyn Any + Send> { self }
    }

    #[async_trait]
    impl DistributedMutexManager for CountdownMutexManager {
        async fn lock(&self, _: &str, _: Duration) -> Result<Box<dyn AcquiredLock>, LockError> {
            let mut remaining = self.fails_remaining.lock().unwrap();
            if *remaining > 0 {
                *remaining -= 1;
                Err(LockError::ResourceBusy)
            } else {
                Ok(Box::new(NoopLock))
            }
        }
        async fn unlock(&self, _: Box<dyn AcquiredLock>) {}
    }

    #[tokio::test]
    async fn acquire_succeeds_on_free_key() {
        let mut lock = LocalPathLock::new("path/to/asset", "cache-id", mgr());
        assert!(lock.aquire().await.is_ok());
    }

    #[tokio::test]
    async fn acquire_fails_when_key_is_held() {
        let manager = mgr();
        let mut lock1 = LocalPathLock::new("path/to/asset", "cache-id", Arc::clone(&manager));
        lock1.aquire().await.unwrap();

        let mut lock2 = LocalPathLock::new("path/to/asset", "cache-id", Arc::clone(&manager));
        assert!(matches!(lock2.aquire().await, Err(LockError::ResourceBusy)));
    }

    #[tokio::test]
    async fn different_paths_do_not_conflict() {
        let manager = mgr();
        let mut lock1 = LocalPathLock::new("path/a", "cache-id", Arc::clone(&manager));
        let mut lock2 = LocalPathLock::new("path/b", "cache-id", Arc::clone(&manager));
        assert!(lock1.aquire().await.is_ok());
        assert!(lock2.aquire().await.is_ok());
    }

    #[tokio::test]
    async fn drop_releases_lock() {
        let manager = mgr();
        {
            let mut lock = LocalPathLock::new("path/to/asset", "cache-id", Arc::clone(&manager));
            lock.aquire().await.unwrap();
        }
        // Yield so the spawned unlock task runs.
        tokio::task::yield_now().await;

        let mut relock = LocalPathLock::new("path/to/asset", "cache-id", Arc::clone(&manager));
        assert!(relock.aquire().await.is_ok());
    }

    #[tokio::test]
    async fn acquire_wait_succeeds_immediately_when_free() {
        let mut lock = LocalPathLock::new("path/to/asset", "cache-id", mgr());
        assert!(lock.aquire_wait().await.is_ok());
    }

    #[tokio::test(start_paused = true)]
    async fn acquire_wait_retries_and_eventually_succeeds() {
        // Fails twice then succeeds on the third attempt.
        let manager = Arc::new(CountdownMutexManager::new(2));
        let handle = tokio::spawn(async move {
            let mut lock = LocalPathLock::new("p", "c", Arc::new(CountdownMutexManager::new(2)));
            lock.aquire_wait().await
        });

        tokio::time::advance(LOCK_AQUIRE_SLEEP_WAIT * 2).await;
        assert!(handle.await.unwrap().is_ok());
    }

    #[tokio::test(start_paused = true)]
    async fn acquire_wait_returns_timeout_after_all_attempts_fail() {
        let handle = tokio::spawn(async {
            let mut lock = LocalPathLock::new("p", "c", Arc::new(AlwaysBusyMutexManager));
            lock.aquire_wait().await
        });

        tokio::time::advance(LOCK_AQUIRE_SLEEP_WAIT * MAX_LOCK_AQUIRE_ATTEMPTS as u32).await;
        assert!(matches!(
            handle.await.unwrap(),
            Err(LockError::AcquisitionTimeout(_))
        ));
    }
}