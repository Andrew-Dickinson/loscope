use crate::providers::backends::asset_fetcher::{parse_manifest, AssetFetcher, AssetType, MANIFEST_FILE_NAME};
use crate::types::errors::AssetErr;
use std::fs;
use std::fs::File;
use std::io::{Read, Write};
use std::path::PathBuf;
use std::fs::read_to_string;
use std::sync::Arc;
use tempfile::NamedTempFile;
use typed_path::Utf8UnixPath;
use crate::providers::backends::distributed_mutex::DistributedMutexManager;
use crate::providers::backends::local_path_lock::LocalPathLock;

const CACHE_ID_FILE_NAME: &str = "cache-id";

#[async_trait]
pub trait AssetProvider {
    async fn get_asset(&self, asset_type: AssetType, asset_id: &str) -> Result<File, AssetErr>;
    async fn list_assets_of_type(&self, asset_type: AssetType) -> Result<Vec<String>, AssetErr>;
    fn get_local_asset_path(&self, asset_type: AssetType, asset_id: &str) -> PathBuf;
}

pub struct CachingAssetProvider {
    upstream_fetcher: Box<dyn AssetFetcher + Send + Sync>,
    lock_manager: Arc<dyn DistributedMutexManager>,
    cache_root: PathBuf,
    local_cache_id: String,
}

impl CachingAssetProvider {
    pub fn new(
        upstream_fetcher: Box<dyn AssetFetcher + Send + Sync>,
        lock_manager: Arc<dyn DistributedMutexManager>,
        cache_root: PathBuf,
    ) -> Result<CachingAssetProvider, AssetErr> {
        let local_cache_id = Self::get_local_cache_id(&cache_root)
            .map_err(|err| AssetErr::LocalFileSystemError(
                format!("Failed to get local cache id: {}", err))
            )?;

        Ok(CachingAssetProvider {
            upstream_fetcher,
            lock_manager,
            cache_root,
            local_cache_id
        })
    }

    fn generate_new_local_cache_id(cache_root: &PathBuf) -> Result<String, std::io::Error> {
        let new_cache_id = uuid::Uuid::new_v4().to_string();
        let cache_id_path = cache_root.join(CACHE_ID_FILE_NAME);
        let temp_path_buf = {
            let mut temp = cache_root.join(CACHE_ID_FILE_NAME).clone().into_os_string();
            temp.push(format!("-{:05}.tmp", new_cache_id));
            PathBuf::from(temp)
        };
        let temp_path = temp_path_buf.as_path();

        let mut temp_handle = File::create(temp_path)?;
        temp_handle.write_all(new_cache_id.as_bytes())?;
        temp_handle.flush()?;

        match renamore::rename_exclusive(temp_path, &cache_id_path) {
            Ok(_) => {
                // We won the race (if there was one), other threads will use our ID
                Ok(new_cache_id)
            },
            Err(err) => {
                // Clean up our temp file, if needed
                fs::remove_file(temp_path)?;

                if err.kind() == std::io::ErrorKind::AlreadyExists {
                    // Someone else won the race; throw away our ID in favour of theirs
                    read_to_string(cache_id_path)
                } else {
                    // Other error accessing the filesystem: permissions, etc.
                    Err(err)
                }
            }
        }
    }

    fn get_local_cache_id(cache_root: &PathBuf) -> Result<String, std::io::Error> {
        let cache_id_path = cache_root.join(CACHE_ID_FILE_NAME);

        match read_to_string(cache_id_path) {
            Ok(cache_id) => Ok(cache_id),
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
                Self::generate_new_local_cache_id(cache_root)
            }
            Err(err) => Err(err),
        }
    }
}

#[async_trait]
impl AssetProvider for CachingAssetProvider {
    fn get_local_asset_path(&self, asset_type: AssetType, asset_id: &str) -> PathBuf {
        self.cache_root.join(asset_type.as_ref()).join(asset_id)
    }

    async fn list_assets_of_type(&self, asset_type: AssetType) -> Result<Vec<String>, AssetErr> {
        let manifest_local_path = PathBuf::from(asset_type.as_ref()).join(MANIFEST_FILE_NAME);
        let manifest_local_path_str = manifest_local_path.to_str().ok_or_else(
            || AssetErr::AssetDownloadError(format!("Invalid asset type: {asset_type}"))
        )?;

        let mut path_lock = LocalPathLock::new(
            manifest_local_path_str,
            &self.local_cache_id,
            Arc::clone(&self.lock_manager),
        );

        if let Err(lock_err) = path_lock.aquire_wait().await {
            return Err(AssetErr::LocalFileSystemError(format!(
                "Unable to aquire lock for asset type {} on local shared file system: {}",
                asset_type, lock_err
            )));
        }

        let manifest_full_path = self.cache_root.join(&manifest_local_path);

        if manifest_full_path.exists() {
            let obstruction_index_manifest = fs::read(&manifest_full_path)
                .map_err(|e| AssetErr::LocalFileSystemError(format!(
                    "Error reading local cached obstruction index {}", e
                )))?;

            return parse_manifest(obstruction_index_manifest);
        }

        let assets = self.upstream_fetcher.list_assets(asset_type).await?;
        fs::create_dir_all(manifest_full_path.parent().unwrap()).map_err(|e| {
            AssetErr::LocalFileSystemError(format!("Error creating manifest directory: {e}"))
        })?;
        fs::write(&manifest_full_path, assets.join("\n"))
            .map_err(|e| AssetErr::LocalFileSystemError(format!(
                "Error writing local cached manifest {}", e
            )))?;

        Ok(assets)
    }

    async fn get_asset(&self, asset_type: AssetType, asset_id: &str) -> Result<File, AssetErr> {
        let cache_local_path = PathBuf::from(asset_type.as_ref()).join(asset_id);
        let cache_local_path_str = cache_local_path.to_str().ok_or_else(
            || AssetErr::AssetDownloadError(
                format!("Invalid asset_id or type: {asset_id} of type {asset_type}")
            )
        )?;

        let mut path_lock = LocalPathLock::new(
            cache_local_path_str,
            &self.local_cache_id,
            Arc::clone(&self.lock_manager),
        );

        if let Err(lock_err) = path_lock.aquire_wait().await {
            return Err(AssetErr::LocalFileSystemError(format!(
                "Unable to aquire lock for asset {} of type {} on local shared file system: {}",
                asset_id, asset_type, lock_err
            )));
        }

        let item_path_buf = self.cache_root.join(&cache_local_path);
        let item_path = item_path_buf.as_path();

        // If we have the asset cached on disk already, return it
        match File::open(item_path) {
            Ok(file_handle) => return Ok(file_handle),
            Err(e) => {
                if e.kind() != std::io::ErrorKind::NotFound {
                    return Err(AssetErr::LocalFileSystemError(format!(
                        "Error checking for cached asset at {item_path:?}: {e}"
                    )));
                }
            }
        }

        println!("Calling upstream fetcher for {asset_type:?} {asset_id:?}");

        let asset_type_dir = self.cache_root.join(asset_type.as_ref());
        fs::create_dir_all(&asset_type_dir).map_err(|err| AssetErr::LocalFileSystemError(
            format!("Error creating cache directory {:?}: {}", asset_type_dir, err)
        ))?;

        let temp_file = NamedTempFile::with_prefix_in(asset_id, &asset_type_dir)
            .map_err(|err| AssetErr::LocalFileSystemError(
                format!("Error creating cache file {}", err))
            )?;

        let remote_path = Utf8UnixPath::new(asset_id);
        self.upstream_fetcher
            .fetch_asset(asset_type, remote_path, temp_file.path())
            .await?;

        fs::rename(temp_file.path(), item_path).map_err(|err| {
            AssetErr::LocalFileSystemError(format!(
                "Failed to move temp file {:?} to {item_path:?}: {err}", temp_file.path()
            ))
        })?;

        Ok(File::open(item_path).map_err(|err| AssetErr::LocalFileSystemError(format!(
            "Unable to open fetched file at {item_path:?}: {err}"
        )))?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::providers::backends::distributed_mutex::InMemoryMutexManager;
    use crate::providers::backends::local_path_lock::LOCK_AQUIRE_SLEEP_WAIT;
    use crate::types::errors::AssetErr;
    use std::io::Read;
    use std::path::Path;
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    use std::time::Duration;
    use test_temp_dir::test_temp_dir;
    use typed_path::Utf8UnixPath;

    fn in_memory_lock_manager() -> Arc<dyn crate::providers::backends::distributed_mutex::DistributedMutexManager> {
        Arc::new(InMemoryMutexManager::default())
    }

    struct MockAssetFetcher {
        should_succeed: bool,
    }

    #[async_trait]
    impl AssetFetcher for MockAssetFetcher {
        async fn fetch_asset(
            &self,
            _: AssetType,
            _: &Utf8UnixPath,
            local_path: &Path,
        ) -> Result<(), AssetErr> {
            if self.should_succeed {
                if let Some(parent) = local_path.parent() {
                    std::fs::create_dir_all(parent).unwrap();
                }
                std::fs::write(local_path, b"fetched-content").unwrap();
                Ok(())
            } else {
                Err(AssetErr::AssetNotFound("mock: not found".to_string()))
            }
        }

        async fn list_assets(&self, _asset_type: AssetType) -> Result<Vec<String>, AssetErr> {
            panic!("MockAssetFetcher::list_assets")
        }
    }

    // None => returns Err, Some(v) => returns Ok(v)
    struct ListMockFetcher {
        assets: Option<Vec<String>>,
    }

    #[async_trait]
    impl AssetFetcher for ListMockFetcher {
        async fn fetch_asset(&self, _: AssetType, _: &Utf8UnixPath, _: &Path) -> Result<(), AssetErr> {
            panic!("ListMockFetcher::fetch_asset not expected")
        }
        async fn list_assets(&self, _: AssetType) -> Result<Vec<String>, AssetErr> {
            match &self.assets {
                Some(v) => Ok(v.clone()),
                None => Err(AssetErr::AssetDownloadError("mock upstream error".into())),
            }
        }
    }

    struct DelayedMockFetcher {
        delay_ms: u64,
    }

    #[async_trait]
    impl AssetFetcher for DelayedMockFetcher {
        async fn fetch_asset(
            &self,
            _: AssetType,
            _: &Utf8UnixPath,
            local_path: &Path,
        ) -> Result<(), AssetErr> {
            use std::io::Write;
            if let Some(parent) = local_path.parent() {
                std::fs::create_dir_all(parent).unwrap();
            }
            // Open the handle before sleeping so both tasks hold a handle to the same inode
            // when a fixed .tmp name is used — mirroring what the real S3 fetcher does.
            let mut file = std::fs::File::create(local_path).unwrap();
            tokio::time::sleep(std::time::Duration::from_millis(self.delay_ms)).await;
            file.write_all(b"fetched-content").unwrap();
            Ok(())
        }

        async fn list_assets(&self, _asset_type: AssetType) -> Result<Vec<String>, AssetErr> {
            panic!("DelayedMockFetcher::list_assets");
        }
    }

    // Counts upstream fetch calls; optionally sleeps to interleave concurrent tasks.
    struct CountingFetcher {
        count: Arc<AtomicUsize>,
        delay_ms: u64,
    }

    #[async_trait]
    impl AssetFetcher for CountingFetcher {
        async fn fetch_asset(&self, _: AssetType, _: &Utf8UnixPath, local_path: &Path) -> Result<(), AssetErr> {
            self.count.fetch_add(1, Ordering::SeqCst);
            if let Some(parent) = local_path.parent() {
                std::fs::create_dir_all(parent).unwrap();
            }
            if self.delay_ms > 0 {
                tokio::time::sleep(Duration::from_millis(self.delay_ms)).await;
            }
            std::fs::write(local_path, b"fetched-content").unwrap();
            Ok(())
        }

        async fn list_assets(&self, _: AssetType) -> Result<Vec<String>, AssetErr> {
            panic!("CountingFetcher::list_assets not expected");
        }
    }

    // Returns AssetNotFound on the first call, succeeds on subsequent calls.
    struct FailOnceFetcher {
        has_failed: AtomicBool,
    }

    #[async_trait]
    impl AssetFetcher for FailOnceFetcher {
        async fn fetch_asset(&self, _: AssetType, _: &Utf8UnixPath, local_path: &Path) -> Result<(), AssetErr> {
            if !self.has_failed.swap(true, Ordering::SeqCst) {
                return Err(AssetErr::AssetNotFound("mock: first call fails".to_string()));
            }
            if let Some(parent) = local_path.parent() {
                std::fs::create_dir_all(parent).unwrap();
            }
            std::fs::write(local_path, b"fetched-content").unwrap();
            Ok(())
        }

        async fn list_assets(&self, _: AssetType) -> Result<Vec<String>, AssetErr> {
            panic!("FailOnceFetcher::list_assets not expected");
        }
    }

    fn read_file_contents(file: &mut File) -> String {
        let mut contents = String::new();
        file.read_to_string(&mut contents).unwrap();
        contents
    }

    #[tokio::test]
    async fn concurrent_fetches_of_same_asset_both_succeed() {
        // Regression: two concurrent get_asset calls for the same uncached asset must not
        // collide on the temp file. The sleep in DelayedMockFetcher forces both tasks to be
        // mid-fetch simultaneously, reproducing the interleaving.
        let temp_dir = test_temp_dir!();
        let cache_root = temp_dir.as_path_untracked().to_path_buf();

        let (p1, p2) = (
            CachingAssetProvider::new(
                Box::new(DelayedMockFetcher { delay_ms: 5 }),
                in_memory_lock_manager(),
                cache_root.clone(),
            ).unwrap(),
            CachingAssetProvider::new(
                Box::new(DelayedMockFetcher { delay_ms: 100 }),
                in_memory_lock_manager(),
                cache_root.clone(),
            ).unwrap(),
        );

        let (r1, r2) = tokio::join!(
            p1.get_asset(AssetType::OrthoImage, "test.jpg"),
            p2.get_asset(AssetType::OrthoImage, "test.jpg"),
        );

        assert!(r1.is_ok(), "first concurrent fetch failed: {r1:?}");
        assert!(r2.is_ok(), "second concurrent fetch failed: {r2:?}");
        assert!(cache_root.join("OrthoImage").join("test.jpg").exists());
    }

    #[tokio::test]
    async fn test_cache_hit_returns_cached_file_without_fetching() {
        let temp_dir = test_temp_dir!();
        let cache_root = temp_dir.as_path_untracked().to_path_buf();

        let cache_path = cache_root.join("OrthoImage").join("test.jpg");
        std::fs::create_dir_all(cache_path.parent().unwrap()).unwrap();
        std::fs::write(&cache_path, b"cached-content").unwrap();

        // Fetcher always errors — success proves it was never called
        let provider = CachingAssetProvider::new(
            Box::new(MockAssetFetcher { should_succeed: false }),
            in_memory_lock_manager(),
            cache_root,
        ).unwrap();

        let mut file = provider
            .get_asset(AssetType::OrthoImage, "test.jpg")
            .await
            .unwrap();
        assert_eq!(read_file_contents(&mut file), "cached-content");
    }

    #[tokio::test]
    async fn test_cache_miss_fetches_and_returns_file() {
        let temp_dir = test_temp_dir!();
        let cache_root = temp_dir.as_path_untracked().to_path_buf();

        let provider = CachingAssetProvider::new(
            Box::new(MockAssetFetcher { should_succeed: true }),
            in_memory_lock_manager(),
            cache_root.clone(),
        ).unwrap();

        let mut file = provider
            .get_asset(AssetType::OrthoImage, "test.jpg")
            .await
            .unwrap();
        assert_eq!(read_file_contents(&mut file), "fetched-content");

        assert!(cache_root.join("OrthoImage").join("test.jpg").exists());
    }

    #[tokio::test]
    async fn test_cache_miss_propagates_fetcher_error() {
        let temp_dir = test_temp_dir!();
        let cache_root = temp_dir.as_path_untracked().to_path_buf();

        let provider = CachingAssetProvider::new(
            Box::new(MockAssetFetcher { should_succeed: false }),
            in_memory_lock_manager(),
            cache_root,
        ).unwrap();

        let result = provider.get_asset(AssetType::OrthoImage, "test.jpg").await;
        assert!(matches!(result, Err(AssetErr::AssetNotFound(_))));
    }

    // --- list_assets_of_type ---

    fn manifest_path(cache_root: &PathBuf, asset_type: AssetType) -> PathBuf {
        cache_root.join(asset_type.as_ref()).join(MANIFEST_FILE_NAME)
    }

    #[tokio::test]
    async fn list_assets_no_manifest_fetches_upstream_and_writes_manifest() {
        let temp_dir = test_temp_dir!();
        let cache_root = temp_dir.as_path_untracked().to_path_buf();
        let provider = CachingAssetProvider::new(
            Box::new(ListMockFetcher {
                assets: Some(vec!["a.json".into(), "b.json".into()]),
            }),
            in_memory_lock_manager(),
            cache_root.clone(),
        ).unwrap();

        let mut result = provider
            .list_assets_of_type(AssetType::ObstructionIndex)
            .await
            .unwrap();
        result.sort();
        assert_eq!(result, vec!["a.json", "b.json"]);
        assert!(manifest_path(&cache_root, AssetType::ObstructionIndex).exists(),
            "manifest file should be written after first upstream fetch");
    }

    #[tokio::test]
    async fn list_assets_upstream_error_propagates() {
        let temp_dir = test_temp_dir!();
        let cache_root = temp_dir.as_path_untracked().to_path_buf();
        let provider = CachingAssetProvider::new(
            Box::new(ListMockFetcher { assets: None }),
            in_memory_lock_manager(),
            cache_root,
        ).unwrap();

        let result = provider.list_assets_of_type(AssetType::ObstructionIndex).await;
        assert!(matches!(result, Err(AssetErr::AssetDownloadError(_))));
    }

    #[tokio::test]
    async fn list_assets_manifest_cached_returns_without_calling_upstream() {
        let temp_dir = test_temp_dir!();
        let cache_root = temp_dir.as_path_untracked().to_path_buf();
        let manifest = manifest_path(&cache_root, AssetType::ObstructionIndex);
        std::fs::create_dir_all(manifest.parent().unwrap()).unwrap();
        std::fs::write(&manifest, "buildings.json\ntowers.json").unwrap();

        // Upstream panics if called — success proves the manifest was used.
        let provider = CachingAssetProvider::new(
            Box::new(MockAssetFetcher { should_succeed: false }),
            in_memory_lock_manager(),
            cache_root,
        ).unwrap();

        let mut result = provider
            .list_assets_of_type(AssetType::ObstructionIndex)
            .await
            .unwrap();
        result.sort();
        assert_eq!(result, vec!["buildings.json", "towers.json"]);
    }

    #[tokio::test]
    async fn list_assets_deleting_cached_json_files_does_not_affect_manifest_listing() {
        // With the old dir-scan approach, deleting asset files would change the result.
        // With the manifest approach, the list comes from the manifest regardless of what
        // files are present on disk.
        let temp_dir = test_temp_dir!();
        let cache_root = temp_dir.as_path_untracked().to_path_buf();
        let asset_dir = cache_root.join("ObstructionIndex");
        std::fs::create_dir_all(&asset_dir).unwrap();

        // Write the manifest and the actual files.
        let manifest = manifest_path(&cache_root, AssetType::ObstructionIndex);
        std::fs::write(&manifest, "buildings.json\ntowers.json").unwrap();
        std::fs::write(asset_dir.join("buildings.json"), b"{}").unwrap();
        std::fs::write(asset_dir.join("towers.json"), b"{}").unwrap();

        // Delete one of the asset files.
        std::fs::remove_file(asset_dir.join("buildings.json")).unwrap();

        let provider = CachingAssetProvider::new(
            Box::new(MockAssetFetcher { should_succeed: false }),
            in_memory_lock_manager(),
            cache_root,
        ).unwrap();

        let mut result = provider
            .list_assets_of_type(AssetType::ObstructionIndex)
            .await
            .unwrap();
        result.sort();
        // Both names still appear because the manifest is the source of truth.
        assert_eq!(result, vec!["buildings.json", "towers.json"]);
    }

    #[tokio::test(start_paused = true)]
    async fn concurrent_list_assets_calls_upstream_once() {
        // Two providers sharing a lock manager race to list assets with no cached manifest.
        // The one that wins the lock fetches from upstream; the other waits and then reads
        // the manifest the winner wrote, so upstream is called exactly once.
        let temp_dir = test_temp_dir!();
        let cache_root = temp_dir.as_path_untracked().to_path_buf();
        let counter = Arc::new(AtomicUsize::new(0));
        let manager = in_memory_lock_manager();

        struct CountingListFetcher { count: Arc<AtomicUsize> }
        #[async_trait]
        impl AssetFetcher for CountingListFetcher {
            async fn fetch_asset(&self, _: AssetType, _: &Utf8UnixPath, _: &Path) -> Result<(), AssetErr> {
                panic!("fetch_asset not expected");
            }
            async fn list_assets(&self, _: AssetType) -> Result<Vec<String>, AssetErr> {
                self.count.fetch_add(1, Ordering::SeqCst);
                tokio::time::sleep(Duration::from_millis(10)).await;
                Ok(vec!["a.json".into(), "b.json".into()])
            }
        }

        let h1 = {
            let count = Arc::clone(&counter);
            let mgr = Arc::clone(&manager);
            let root = cache_root.clone();
            tokio::spawn(async move {
                CachingAssetProvider::new(
                    Box::new(CountingListFetcher { count }),
                    mgr,
                    root,
                ).unwrap().list_assets_of_type(AssetType::ObstructionIndex).await
            })
        };
        let h2 = {
            let count = Arc::clone(&counter);
            let mgr = Arc::clone(&manager);
            let root = cache_root.clone();
            tokio::spawn(async move {
                CachingAssetProvider::new(
                    Box::new(CountingListFetcher { count }),
                    mgr,
                    root,
                ).unwrap().list_assets_of_type(AssetType::ObstructionIndex).await
            })
        };

        tokio::task::yield_now().await;
        // Advance past the 10 ms upstream delay — h1 writes the manifest and drops the lock.
        tokio::time::advance(Duration::from_millis(15)).await;
        tokio::task::yield_now().await;
        // Advance past h2's retry sleep — h2 wakes, acquires the lock, reads the manifest.
        tokio::time::advance(LOCK_AQUIRE_SLEEP_WAIT + Duration::from_millis(1)).await;
        tokio::task::yield_now().await;

        let r1 = h1.await.unwrap().unwrap();
        let r2 = h2.await.unwrap().unwrap();
        assert_eq!(counter.load(Ordering::SeqCst), 1, "upstream list_assets should be called exactly once");
        assert_eq!(r1.len(), 2);
        assert_eq!(r2.len(), 2);
    }

    // --- lock integration ---

    #[tokio::test]
    async fn second_fetch_of_same_asset_calls_upstream_once() {
        // Verifies that the cache is populated after the first fetch and the second
        // fetch is a cache hit without calling the upstream again.
        let temp_dir = test_temp_dir!();
        let cache_root = temp_dir.as_path_untracked().to_path_buf();
        let counter = Arc::new(AtomicUsize::new(0));

        let provider = CachingAssetProvider::new(
            Box::new(CountingFetcher { count: Arc::clone(&counter), delay_ms: 0 }),
            in_memory_lock_manager(),
            cache_root,
        ).unwrap();

        provider.get_asset(AssetType::OrthoImage, "test.jpg").await.unwrap();
        provider.get_asset(AssetType::OrthoImage, "test.jpg").await.unwrap();

        assert_eq!(counter.load(Ordering::SeqCst), 1, "upstream called more than once");
    }

    #[tokio::test]
    async fn lock_released_after_fetch_error_so_retry_succeeds() {
        // Verifies that LocalPathLock::drop runs even on the error path, so a subsequent
        // get_asset call is not blocked waiting to acquire the same lock.
        let temp_dir = test_temp_dir!();
        let cache_root = temp_dir.as_path_untracked().to_path_buf();

        let provider = CachingAssetProvider::new(
            Box::new(FailOnceFetcher { has_failed: AtomicBool::new(false) }),
            in_memory_lock_manager(),
            cache_root,
        ).unwrap();

        let first = provider.get_asset(AssetType::OrthoImage, "test.jpg").await;
        assert!(matches!(first, Err(AssetErr::AssetNotFound(_))));

        // Yield so the unlock task spawned by LocalPathLock::drop can run.
        tokio::task::yield_now().await;

        let second = provider.get_asset(AssetType::OrthoImage, "test.jpg").await;
        assert!(second.is_ok(), "lock should be released after error: {second:?}");
    }

    #[tokio::test(start_paused = true)]
    async fn concurrent_fetches_with_shared_lock_call_upstream_once() {
        // Two providers sharing a lock manager race to fetch the same uncached asset.
        // The second should get a cache hit after the first finishes, so the upstream
        // is called exactly once.
        let temp_dir = test_temp_dir!();
        let cache_root = temp_dir.as_path_untracked().to_path_buf();
        let counter = Arc::new(AtomicUsize::new(0));
        let manager = in_memory_lock_manager();

        let h1 = {
            let counter = Arc::clone(&counter);
            let manager = Arc::clone(&manager);
            let cache_root = cache_root.clone();
            tokio::spawn(async move {
                CachingAssetProvider::new(
                    Box::new(CountingFetcher { count: counter, delay_ms: 10 }),
                    manager,
                    cache_root,
                ).unwrap().get_asset(AssetType::OrthoImage, "test.jpg").await
            })
        };

        let h2 = {
            let counter = Arc::clone(&counter);
            let manager = Arc::clone(&manager);
            tokio::spawn(async move {
                CachingAssetProvider::new(
                    Box::new(CountingFetcher { count: counter, delay_ms: 10 }),
                    manager,
                    cache_root,
                ).unwrap().get_asset(AssetType::OrthoImage, "test.jpg").await
            })
        };

        // Let both tasks start: h1 acquires the lock and enters the fetcher sleep;
        // h2 finds the lock busy at its first attempt and enters the retry sleep.
        tokio::task::yield_now().await;

        // Advance past the 10 ms fetch delay — h1 writes the file, returns, drops the lock.
        tokio::time::advance(Duration::from_millis(15)).await;
        // Yield so the unlock task spawned by LocalPathLock::drop executes.
        tokio::task::yield_now().await;

        // Advance past h2's retry sleep — h2 wakes, acquires the now-free lock, gets a cache hit.
        tokio::time::advance(LOCK_AQUIRE_SLEEP_WAIT + Duration::from_millis(1)).await;
        tokio::task::yield_now().await;

        assert!(h1.await.unwrap().is_ok(), "h1 should succeed");
        assert!(h2.await.unwrap().is_ok(), "h2 should succeed");
        assert_eq!(counter.load(Ordering::SeqCst), 1, "upstream should be called exactly once");
    }

    // --- cache ID generation ---

    #[test]
    fn new_creates_cache_id_file() {
        let temp_dir = test_temp_dir!();
        let cache_root = temp_dir.as_path_untracked().to_path_buf();

        CachingAssetProvider::new(
            Box::new(MockAssetFetcher { should_succeed: false }),
            in_memory_lock_manager(),
            cache_root.clone(),
        ).unwrap();

        assert!(cache_root.join("cache-id").exists(), "cache-id file should be created");
    }

    #[test]
    fn cache_id_stable_across_multiple_new_calls() {
        // A second provider on the same cache root must return the same ID, not generate a new one.
        let temp_dir = test_temp_dir!();
        let cache_root = temp_dir.as_path_untracked().to_path_buf();

        CachingAssetProvider::new(
            Box::new(MockAssetFetcher { should_succeed: false }),
            in_memory_lock_manager(),
            cache_root.clone(),
        ).unwrap();
        let id1 = std::fs::read_to_string(cache_root.join("cache-id")).unwrap();

        CachingAssetProvider::new(
            Box::new(MockAssetFetcher { should_succeed: false }),
            in_memory_lock_manager(),
            cache_root.clone(),
        ).unwrap();
        let id2 = std::fs::read_to_string(cache_root.join("cache-id")).unwrap();

        assert_eq!(id1, id2, "same cache root should produce the same ID on every call");
    }

    #[test]
    fn concurrent_new_calls_produce_a_single_id_file() {
        // Exercises the rename_exclusive race: many threads simultaneously try to create the
        // cache-id file; exactly one wins and all losers fall back to reading the winner's ID.
        // After all threads complete there must be exactly one cache-id file and no leftover
        // .tmp files.
        let temp_dir = test_temp_dir!();
        let cache_root = temp_dir.as_path_untracked().to_path_buf();

        let handles: Vec<_> = (0..16).map(|_| {
            let root = cache_root.clone();
            std::thread::spawn(move || {
                CachingAssetProvider::new(
                    Box::new(MockAssetFetcher { should_succeed: false }),
                    in_memory_lock_manager(),
                    root,
                ).unwrap()
            })
        }).collect();

        handles.into_iter().for_each(|h| { h.join().unwrap(); });

        let id = std::fs::read_to_string(cache_root.join("cache-id"))
            .expect("cache-id file must exist after concurrent new calls");
        assert!(!id.is_empty(), "cache-id must not be empty");

        let tmp_files: Vec<_> = std::fs::read_dir(&cache_root).unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_name().to_string_lossy().ends_with(".tmp"))
            .collect();
        assert!(tmp_files.is_empty(), "no .tmp files should remain: {tmp_files:?}");
    }
}
