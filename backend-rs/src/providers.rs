use std::collections::HashMap;
use std::path::PathBuf;
use derive_getters::Getters;
use tokio::fs;
use typed_path::Utf8UnixPathBuf;
use crate::util::env::{expect_env, LOCAL_ASSET_CACHE_ROOT, LOS_ASSET_S3_BUCKET, LOS_ORTHOS_S3_PREFIX};
use crate::providers::asset_fetcher::{AssetType, S3AssetFetcher};
use crate::providers::fs_cache::{AssetProvider, CachingAssetProvider};
use crate::providers::ortho_provider::{CachingOrthoProvider};

pub mod ortho_provider;
pub mod fs_cache;
pub mod asset_fetcher;


#[derive(Getters)]
pub struct Providers<T: AssetProvider> {
    ortho_provider: CachingOrthoProvider<T>
}

pub type S3BackedProviders = Providers<CachingAssetProvider<S3AssetFetcher>>;

impl S3BackedProviders {
    pub async fn new_with_s3_from_env() -> Self {
        let prefix_map = HashMap::from([
            (AssetType::OrthoImage, Utf8UnixPathBuf::from(expect_env(LOS_ORTHOS_S3_PREFIX))),
        ]);

        let bucket = expect_env(LOS_ASSET_S3_BUCKET);
        let cache_root = PathBuf::from(expect_env(LOCAL_ASSET_CACHE_ROOT));
        fs::create_dir_all(&cache_root).await.expect("Failed to create cache root");

        let shared_config = aws_config::from_env().load().await;
        let s3_client = aws_sdk_s3::Client::new(&shared_config);

        let asset_fetcher = S3AssetFetcher::new(s3_client, bucket, prefix_map);
        let asset_provider = CachingAssetProvider::new(asset_fetcher, cache_root);
        Self {
            ortho_provider: CachingOrthoProvider::new(asset_provider)
        }
    }
}