use std::fs::File;
use std::path::PathBuf;
use derive_new::new;
use typed_path::{Utf8UnixPath};
use crate::providers::backends::asset_fetcher::{AssetFetcher, AssetType};
use crate::types::errors::AssetErr;

#[async_trait]
pub trait AssetProvider {
    // TODO: Strongly type asset_id?
    async fn get_asset(&self, asset_type: AssetType, asset_id: &str) -> Result<File, AssetErr>;
}

#[derive(new)]
pub struct CachingAssetProvider {
    upstream_fetcher: Box<dyn AssetFetcher + Send + Sync>,
    cache_root: PathBuf,
}

#[async_trait]
impl AssetProvider for CachingAssetProvider {
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

        println!("Calling upstream fetcher for {asset_type:?} {asset_id:?}");

        let remote_path = Utf8UnixPath::new(asset_id);
        self.upstream_fetcher.fetch_asset(asset_type, remote_path, item_path).await?;

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

    fn read_file_contents(file: &mut File) -> String {
        let mut contents = String::new();
        file.read_to_string(&mut contents).unwrap();
        contents
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
