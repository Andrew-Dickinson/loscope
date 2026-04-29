use std::collections::HashMap;
use std::path::PathBuf;
use derive_getters::Getters;
use tokio::fs;
use typed_path::Utf8UnixPathBuf;
use crate::util::env::{expect_env, LOCAL_ASSET_CACHE_ROOT, LOS_ASSET_S3_BUCKET, LOS_ORTHOS_S3_PREFIX, NYC_DOB_SQLITE_DB_FILE};
use backends::asset_fetcher::{AssetType, S3AssetFetcher};
use backends::fs_cache::{AssetProvider, CachingAssetProvider};
use crate::providers::backends::string_provider::NYCDOBSqliteStringProvider;
use crate::providers::footprint_provider::{FootprintProvider, StringBackedFootprintProvider};
use crate::providers::ortho_provider::{CachingOrthoProvider, OrthoProvider};
use crate::types::errors::ProviderInitErr;

pub mod ortho_provider;
pub mod footprint_provider;
pub mod backends;

#[derive(Getters)]
pub struct Providers {
    ortho_provider: Box<dyn OrthoProvider + Send + Sync>,
    footprint_provider: Box<dyn FootprintProvider + Send + Sync>
}

impl Providers {
    pub async fn new_from_env() -> Result<Self, ProviderInitErr> {
        let prefix_map = HashMap::from([
            (AssetType::OrthoImage, Utf8UnixPathBuf::from(expect_env(LOS_ORTHOS_S3_PREFIX))),
        ]);

        let bucket = expect_env(LOS_ASSET_S3_BUCKET);
        let cache_root = PathBuf::from(expect_env(LOCAL_ASSET_CACHE_ROOT));
        fs::create_dir_all(&cache_root).await.expect("Failed to create cache root");

        let shared_config = aws_config::from_env().load().await;
        let s3_client = aws_sdk_s3::Client::new(&shared_config);

        // TODO: This is loosely coupled to S3, which is intentional. We want to swap the asset
        //       fetcher used in prod to something that uses generic HTTP, probably pointed at
        //       something mesh-internal
        let asset_fetcher = S3AssetFetcher::new(s3_client, bucket, prefix_map);
        let asset_provider = CachingAssetProvider::new(asset_fetcher, cache_root);

        Ok(
            Self {
                ortho_provider: Box::new(CachingOrthoProvider::new(Box::new(asset_provider))),

                // TODO: We probably don't want to bundle the 0.5-6.0 GB sqlite db with our builds
                //       or dynamically fetch it at runtime either, this should probably get reworked
                //       to use the asset-fetcher backend somehow
                footprint_provider: Box::new(
                    StringBackedFootprintProvider::new(
                        Box::new(
                            NYCDOBSqliteStringProvider::new(
                                &expect_env(NYC_DOB_SQLITE_DB_FILE)
                            ).await?
                        ),
                    )
                )
            }
        )
    }
}