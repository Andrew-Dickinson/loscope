use crate::providers::backends::asset_fetcher::{AssetFetcher, AssetType};
use crate::types::errors::AssetErr;
use derive_new::new;
use rand::Rng;
use std::fs;
use std::fs::{File};
use std::path::{PathBuf};
use typed_path::Utf8UnixPath;

#[async_trait]
pub trait AssetProvider {
    async fn get_asset(&self, asset_type: AssetType, asset_id: &str) -> Result<File, AssetErr>;
    async fn list_assets_of_type(&self, asset_type: AssetType) -> Result<Vec<String>, AssetErr>;
    fn get_local_asset_path(&self, asset_type: AssetType, asset_id: &str) -> PathBuf;
}

#[derive(new)]
pub struct CachingAssetProvider {
    upstream_fetcher: Box<dyn AssetFetcher + Send + Sync>,
    cache_root: PathBuf,
}

#[async_trait]
impl AssetProvider for CachingAssetProvider {
    fn get_local_asset_path(&self, asset_type: AssetType, asset_id: &str) -> PathBuf {
        self.cache_root.join(asset_type.as_ref()).join(asset_id)
    }

    async fn list_assets_of_type(&self, asset_type: AssetType) -> Result<Vec<String>, AssetErr> {
        let index_local_path = self.cache_root.join(asset_type.as_ref());
        if index_local_path.exists() {
            let file_list: Vec<String> = fs::read_dir(index_local_path)
                .map_err(|e| {
                    AssetErr::LocalFileSystemError(format!(
                        "Error reading local cached obstruction index {}",
                        e
                    ))
                })?
                .filter_map(|e| e.ok())
                .filter(|e| e.file_type().map(|e| e.is_file()).unwrap_or(false))
                .filter_map(|f| f.file_name().into_string().ok())
                .collect();
            if !file_list.is_empty() {
                return Ok(file_list);
            }
        }

        self.upstream_fetcher.list_assets(asset_type).await
    }

    async fn get_asset(&self, asset_type: AssetType, asset_id: &str) -> Result<File, AssetErr> {
        let item_path_buf = self.cache_root.join(asset_type.as_ref()).join(asset_id);
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

        // TODO: Expire cache so we don't leak and fill the disk

        println!("Calling upstream fetcher for {asset_type:?} {asset_id:?}");

        // Use a temp file with a random name to reduce the chance of a collision with another
        // thread that's downloading the same asset. 100k is relatively low, but the only
        // consequence of a collision is that an AssetErr will be thrown below when the
        // second thread tries to write it at the same time
        // TODO: A proper mutex accounting system keyed by local_path could probably resolve
        //  this for real
        let temp_path_buf = {
            let mut temp = item_path_buf.clone().into_os_string();
            temp.push(format!(
                "-{:05}.tmp",
                rand::thread_rng().gen_range(1..100_000)
            ));
            PathBuf::from(temp)
        };
        let temp_path = temp_path_buf.as_path();

        let remote_path = Utf8UnixPath::new(asset_id);
        self.upstream_fetcher
            .fetch_asset(asset_type, remote_path, temp_path)
            .await?;

        std::fs::rename(temp_path, item_path).map_err(|err| {
            AssetErr::LocalFileSystemError(format!(
                "Failed to move temp file {temp_path:?} to {item_path:?}: {err}"
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
    use crate::types::errors::AssetErr;
    use std::io::Read;
    use std::path::Path;
    use test_temp_dir::test_temp_dir;
    use typed_path::Utf8UnixPath;

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
        async fn fetch_asset(
            &self,
            _: AssetType,
            _: &Utf8UnixPath,
            _: &Path,
        ) -> Result<(), AssetErr> {
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
                cache_root.clone(),
            ),
            CachingAssetProvider::new(
                Box::new(DelayedMockFetcher { delay_ms: 100 }),
                cache_root.clone(),
            ),
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
            Box::new(MockAssetFetcher {
                should_succeed: false,
            }),
            cache_root,
        );

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
            Box::new(MockAssetFetcher {
                should_succeed: true,
            }),
            cache_root.clone(),
        );

        let mut file = provider
            .get_asset(AssetType::OrthoImage, "test.jpg")
            .await
            .unwrap();
        assert_eq!(read_file_contents(&mut file), "fetched-content");

        // File should now be present in the cache
        assert!(cache_root.join("OrthoImage").join("test.jpg").exists());
    }

    #[tokio::test]
    async fn test_cache_miss_propagates_fetcher_error() {
        let temp_dir = test_temp_dir!();
        let cache_root = temp_dir.as_path_untracked().to_path_buf();

        let provider = CachingAssetProvider::new(
            Box::new(MockAssetFetcher {
                should_succeed: false,
            }),
            cache_root,
        );

        let result = provider.get_asset(AssetType::OrthoImage, "test.jpg").await;
        assert!(matches!(result, Err(AssetErr::AssetNotFound(_))));
    }

    // --- list_assets_of_type ---

    #[tokio::test]
    async fn list_assets_no_cache_dir_delegates_to_upstream() {
        let temp_dir = test_temp_dir!();
        let cache_root = temp_dir.as_path_untracked().to_path_buf();
        // Cache dir for ObstructionIndex is absent — upstream should be called.
        let provider = CachingAssetProvider::new(
            Box::new(ListMockFetcher {
                assets: Some(vec!["a.json".into(), "b.json".into()]),
            }),
            cache_root,
        );

        let mut result = provider
            .list_assets_of_type(AssetType::ObstructionIndex)
            .await
            .unwrap();
        result.sort();
        assert_eq!(result, vec!["a.json", "b.json"]);
    }

    #[tokio::test]
    async fn list_assets_no_cache_dir_propagates_upstream_error() {
        let temp_dir = test_temp_dir!();
        let cache_root = temp_dir.as_path_untracked().to_path_buf();
        let provider =
            CachingAssetProvider::new(Box::new(ListMockFetcher { assets: None }), cache_root);

        let result = provider
            .list_assets_of_type(AssetType::ObstructionIndex)
            .await;
        assert!(matches!(result, Err(AssetErr::AssetDownloadError(_))));
    }

    #[tokio::test]
    async fn list_assets_cache_dir_exists_returns_file_names() {
        let temp_dir = test_temp_dir!();
        let cache_root = temp_dir.as_path_untracked().to_path_buf();
        let cache_dir = cache_root.join("ObstructionIndex");
        std::fs::create_dir_all(&cache_dir).unwrap();
        std::fs::write(cache_dir.join("buildings.json"), b"{}").unwrap();
        std::fs::write(cache_dir.join("towers.json"), b"{}").unwrap();

        // Upstream panics if called — success proves the cache was used.
        let provider = CachingAssetProvider::new(
            Box::new(MockAssetFetcher {
                should_succeed: false,
            }),
            cache_root,
        );

        let mut result = provider
            .list_assets_of_type(AssetType::ObstructionIndex)
            .await
            .unwrap();
        result.sort();
        assert_eq!(result, vec!["buildings.json", "towers.json"]);
    }

    #[tokio::test]
    async fn list_assets_cache_dir_exists_but_empty_delegates_to_upstream() {
        let temp_dir = test_temp_dir!();
        let cache_root = temp_dir.as_path_untracked().to_path_buf();
        std::fs::create_dir_all(cache_root.join("ObstructionIndex")).unwrap();
        // Cache dir for ObstructionIndex is empty — upstream should be called.
        let provider = CachingAssetProvider::new(
            Box::new(ListMockFetcher {
                assets: Some(vec!["a.json".into(), "b.json".into()]),
            }),
            cache_root,
        );

        let mut result = provider
            .list_assets_of_type(AssetType::ObstructionIndex)
            .await
            .unwrap();
        result.sort();
        assert_eq!(result, vec!["a.json", "b.json"]);
    }

    #[tokio::test]
    async fn list_assets_cache_dir_excludes_subdirectories() {
        let temp_dir = test_temp_dir!();
        let cache_root = temp_dir.as_path_untracked().to_path_buf();
        let cache_dir = cache_root.join("ObstructionIndex");
        std::fs::create_dir_all(&cache_dir).unwrap();
        std::fs::write(cache_dir.join("real.json"), b"{}").unwrap();
        std::fs::create_dir_all(cache_dir.join("subdir")).unwrap();

        let provider = CachingAssetProvider::new(
            Box::new(MockAssetFetcher {
                should_succeed: false,
            }),
            cache_root,
        );

        let result = provider
            .list_assets_of_type(AssetType::ObstructionIndex)
            .await
            .unwrap();
        assert_eq!(result, vec!["real.json"]);
    }
}
