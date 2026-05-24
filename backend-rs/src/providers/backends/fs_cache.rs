use std::fs::File;
use std::path::PathBuf;
use derive_new::new;
use rand::Rng;
use typed_path::{Utf8UnixPath};
use crate::providers::backends::asset_fetcher::{AssetFetcher, AssetType};
use crate::types::errors::AssetErr;

#[async_trait]
pub trait AssetProvider {
    // TODO: Strongly type asset_id?
    async fn get_asset(&self, asset_type: AssetType, asset_id: &str) -> Result<File, AssetErr>;
    fn get_local_asset_path(&self, asset_type: AssetType, asset_id: &str) -> PathBuf;
}

#[derive(new)]
pub struct CachingAssetProvider {
    upstream_fetcher: Box<dyn AssetFetcher + Send + Sync>,
    cache_root: PathBuf,
}

#[async_trait]
impl AssetProvider for CachingAssetProvider {
    fn get_local_asset_path<'a>(&self, asset_type: AssetType, asset_id: &'a str) -> PathBuf {
        self.cache_root.join(asset_type.as_ref()).join(asset_id)
    }

    async fn get_asset(&self, asset_type: AssetType, asset_id: &str) -> Result<File, AssetErr> {
        let item_path_buf = self.cache_root.join(asset_type.as_ref()).join(asset_id);
        let item_path = item_path_buf.as_path();

        // If we have the asset cached on disk already, return it
        match File::open(item_path) {
            Ok(file_handle) => return Ok(file_handle),
            Err(e) => {
                if e.kind() != std::io::ErrorKind::NotFound {
                    return Err(
                        AssetErr::LocalFileSystemError(
                            format!("Error checking for cached asset at {item_path:?}: {e}")
                        )
                    )
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
            temp.push(format!("-{:05}.tmp", rand::thread_rng().gen_range(1..100_000)));
            PathBuf::from(temp)
        };
        let temp_path = temp_path_buf.as_path();

        let remote_path = Utf8UnixPath::new(asset_id);
        self.upstream_fetcher.fetch_asset(asset_type, remote_path, temp_path).await?;

        std::fs::rename(temp_path, item_path).map_err(|err| {
            AssetErr::LocalFileSystemError(
                format!("Failed to move temp file {temp_path:?} to {item_path:?}: {err}")
            )
        })?;

        Ok(
          File::open(item_path)
            .or_else(|err| Err(
                AssetErr::LocalFileSystemError(
                    format!("Unable to open fetched file at {item_path:?}: {err}")
                )
            ))?
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Read;
    use std::path::Path;
    use test_temp_dir::test_temp_dir;
    use typed_path::Utf8UnixPath;
    use crate::types::errors::AssetErr;

    struct MockAssetFetcher {
        should_succeed: bool,
    }

    #[async_trait]
    impl AssetFetcher for MockAssetFetcher {
        async fn fetch_asset(&self, _: AssetType, _: &Utf8UnixPath, local_path: &Path) -> Result<(), AssetErr> {
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
    }

    struct DelayedMockFetcher { delay_ms: u64 }

    #[async_trait]
    impl AssetFetcher for DelayedMockFetcher {
        async fn fetch_asset(&self, _: AssetType, _: &Utf8UnixPath, local_path: &Path) -> Result<(), AssetErr> {
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
            CachingAssetProvider::new(Box::new(DelayedMockFetcher { delay_ms: 5 }), cache_root.clone()),
            CachingAssetProvider::new(Box::new(DelayedMockFetcher { delay_ms: 100 }), cache_root.clone()),
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
            cache_root,
        );

        let mut file = provider.get_asset(AssetType::OrthoImage, "test.jpg").await.unwrap();
        assert_eq!(read_file_contents(&mut file), "cached-content");
    }

    #[tokio::test]
    async fn test_cache_miss_fetches_and_returns_file() {
        let temp_dir = test_temp_dir!();
        let cache_root = temp_dir.as_path_untracked().to_path_buf();

        let provider = CachingAssetProvider::new(
            Box::new(MockAssetFetcher { should_succeed: true }),
            cache_root.clone(),
        );

        let mut file = provider.get_asset(AssetType::OrthoImage, "test.jpg").await.unwrap();
        assert_eq!(read_file_contents(&mut file), "fetched-content");

        // File should now be present in the cache
        assert!(cache_root.join("OrthoImage").join("test.jpg").exists());
    }

    #[tokio::test]
    async fn test_cache_miss_propagates_fetcher_error() {
        let temp_dir = test_temp_dir!();
        let cache_root = temp_dir.as_path_untracked().to_path_buf();

        let provider = CachingAssetProvider::new(
            Box::new(MockAssetFetcher { should_succeed: false }),
            cache_root,
        );

        let result = provider.get_asset(AssetType::OrthoImage, "test.jpg").await;
        assert!(matches!(result, Err(AssetErr::AssetNotFound(_))));
    }
}
