use crate::providers::backends::string_provider::NYCDOBSqliteStringProvider;
use crate::providers::backends::value_store::{InMemoryValueStore, RedisValueStore, ValueStore};
use crate::providers::elevation_tile_provider::{
    CachingElevationTileProvider, ElevationTileProvider,
};
use crate::providers::evaluation_result_provider::PointEvaluationResultProvider;
use crate::providers::footprint_provider::{CachingFootprintProvider, FootprintProvider, StringBackedFootprintProvider};
use crate::providers::meshdb_provider::ProgenitorMeshDBProvider;
use crate::providers::obstruction_provider::{CachingObstructionProvider, ObstructionProvider};
use crate::providers::ortho_provider::{CachingOrthoProvider, OrthoProvider};
use crate::types::errors::ProviderInitErr;
use crate::util::env::{LOCAL_ASSET_CACHE_ROOT, LOS_ASSET_S3_BUCKET, LOS_OBSTRUCTION_S3_PREFIX, LOS_ORTHOS_S3_PREFIX, LOS_TERRAIN_TILE_S3_PREFIX, MESHDB_API_TOKEN, expect_env, get_env, REDIS_URL, LOS_FOOTPRINTS_S3_PREFIX};
use backends::asset_fetcher::{AssetType, S3AssetFetcher};
use backends::fs_cache::{AssetProvider, CachingAssetProvider};
use derive_getters::Getters;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::fs;
use typed_path::Utf8UnixPathBuf;

pub mod backends;
pub mod elevation_tile_provider;
pub mod evaluation_result_provider;
pub mod footprint_provider;
pub mod meshdb_provider;
pub mod obstruction_provider;
pub mod ortho_provider;

#[derive(Getters)]
pub struct Providers {
    ortho_provider: Box<dyn OrthoProvider + Send + Sync>,
    footprint_provider: Box<dyn FootprintProvider + Send + Sync>,
    elevation_tile_provider: Box<dyn ElevationTileProvider + Send + Sync>,
    obstruction_provider: Box<dyn ObstructionProvider + Send + Sync>,

    meshdb_provider: ProgenitorMeshDBProvider,
    point_eval_result_provider: PointEvaluationResultProvider,
}

impl Providers {
    pub async fn new_from_env() -> Result<Self, ProviderInitErr> {
        let prefix_map = HashMap::from([
            (
                AssetType::OrthoImage,
                Utf8UnixPathBuf::from(expect_env(LOS_ORTHOS_S3_PREFIX)),
            ),
            (
                AssetType::ElevationTile,
                Utf8UnixPathBuf::from(expect_env(LOS_TERRAIN_TILE_S3_PREFIX)),
            ),
            (
                AssetType::Obstruction,
                Utf8UnixPathBuf::from(expect_env(LOS_OBSTRUCTION_S3_PREFIX)),
            ),
            (
                AssetType::ObstructionIndex,
                Utf8UnixPathBuf::from(expect_env(LOS_OBSTRUCTION_S3_PREFIX) + "_indexes"),
            ),
            (
                AssetType::BuildingFootprintWKT,
                Utf8UnixPathBuf::from(expect_env(LOS_FOOTPRINTS_S3_PREFIX)),
            ),
        ]);

        let bucket = expect_env(LOS_ASSET_S3_BUCKET);
        let cache_root = PathBuf::from(expect_env(LOCAL_ASSET_CACHE_ROOT));
        fs::create_dir_all(&cache_root)
            .await
            .expect("Failed to create cache root");

        let shared_config = aws_config::from_env().load().await;
        let s3_client = aws_sdk_s3::Client::new(&shared_config);

        // TODO: This is loosely coupled to S3, which is intentional. We want to swap the asset
        //       fetcher used in prod to something that uses generic HTTP, probably pointed at
        //       something mesh-internal
        let asset_fetcher = Box::new(S3AssetFetcher::new(s3_client.clone(), bucket, prefix_map));

        let asset_provider: Arc<dyn AssetProvider + Send + Sync> =
            Arc::new(CachingAssetProvider::new(asset_fetcher, cache_root));

        let value_store: Box<dyn ValueStore + Send + Sync> =
            if let Some(redis_url) = get_env(REDIS_URL) {
                println!("Found {} of {}, using for analysis state backend", REDIS_URL, redis_url);
                Box::new(RedisValueStore::new(redis_url.as_str())
                    .map_err(ProviderInitErr::RedisError)?)
            } else {
                println!("{} not set, using process memory for analysis state backend", REDIS_URL);
                Box::new(InMemoryValueStore::new())
            };

        Ok(Self {
            ortho_provider: Box::new(CachingOrthoProvider::new(Arc::clone(&asset_provider))),
            elevation_tile_provider: Box::new(CachingElevationTileProvider::new(Arc::clone(
                &asset_provider,
            ))),
            footprint_provider: Box::new(CachingFootprintProvider::new(Arc::clone(&asset_provider))),
            obstruction_provider: Box::new(
                CachingObstructionProvider::new(Arc::clone(&asset_provider))
                    .await
                    .map_err(ProviderInitErr::AssetPrefetchError)?,
            ),
            point_eval_result_provider: PointEvaluationResultProvider::new(value_store),
            meshdb_provider: ProgenitorMeshDBProvider::new(expect_env(MESHDB_API_TOKEN)),
        })
    }
}
