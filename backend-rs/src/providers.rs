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
use crate::util::env::{LOCAL_ASSET_CACHE_ROOT, LOS_ASSET_S3_BUCKET, MESHDB_API_TOKEN, expect_env, get_env, REDIS_URL, LOS_ORTHOS_PREFIX, LOS_TERRAIN_TILE_PREFIX, LOS_OBSTRUCTION_PREFIX, LOS_FOOTPRINTS_PREFIX, LOS_ASSET_HTTP_BASE_URL};
use backends::asset_fetcher::{AssetType, S3AssetFetcher};
use backends::fs_cache::{AssetProvider, CachingAssetProvider};
use derive_getters::Getters;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::fs;
use typed_path::Utf8UnixPathBuf;
use crate::providers::backends::asset_fetcher::{AssetFetcher, HttpAssetFetcher, ASSET_FETCH_TIMEOUT};
use crate::providers::terrain_classification_tile_provider::{CachingTerrainClassificationTileProvider, TerrainClassificationTileProvider};

pub mod backends;
pub mod elevation_tile_provider;
pub mod evaluation_result_provider;
pub mod footprint_provider;
pub mod meshdb_provider;
pub mod obstruction_provider;
pub mod ortho_provider;
pub mod terrain_classification_tile_provider;

#[derive(Getters)]
pub struct Providers {
    ortho_provider: Box<dyn OrthoProvider + Send + Sync>,
    footprint_provider: Box<dyn FootprintProvider + Send + Sync>,
    elevation_tile_provider: Box<dyn ElevationTileProvider + Send + Sync>,
    terrain_classification_provider: Box<dyn TerrainClassificationTileProvider + Send + Sync>,
    obstruction_provider: Box<dyn ObstructionProvider + Send + Sync>,

    meshdb_provider: ProgenitorMeshDBProvider,
    point_eval_result_provider: PointEvaluationResultProvider,
}

impl Providers {
    pub async fn new_from_env() -> Result<Self, ProviderInitErr> {
        let prefix_map = HashMap::from([
            (
                AssetType::OrthoImage,
                Utf8UnixPathBuf::from(expect_env(LOS_ORTHOS_PREFIX)?),
            ),
            (
                AssetType::ElevationTile,
                Utf8UnixPathBuf::from(expect_env(LOS_TERRAIN_TILE_PREFIX)?),
            ),
            (
                AssetType::TerrainClassificationTile,
                Utf8UnixPathBuf::from(expect_env(LOS_TERRAIN_TILE_PREFIX)?),
            ),
            (
                AssetType::Obstruction,
                Utf8UnixPathBuf::from(expect_env(LOS_OBSTRUCTION_PREFIX)?),
            ),
            (
                AssetType::ObstructionIndex,
                Utf8UnixPathBuf::from(expect_env(LOS_OBSTRUCTION_PREFIX)? + "_indexes"),
            ),
            (
                AssetType::BuildingFootprintWKT,
                Utf8UnixPathBuf::from(expect_env(LOS_FOOTPRINTS_PREFIX)?),
            ),
        ]);

        let cache_root = PathBuf::from(expect_env(LOCAL_ASSET_CACHE_ROOT)?);
        fs::create_dir_all(&cache_root)
            .await
            .expect("Failed to create cache root");

        let bucket = get_env(LOS_ASSET_S3_BUCKET);
        let base_url = get_env(LOS_ASSET_HTTP_BASE_URL);
        let asset_fetcher: Box<dyn AssetFetcher + Send + Sync> = if bucket.is_none() && let Some(base_url) = base_url {
            Box::new(
                HttpAssetFetcher::new(
                    None,
                    base_url
                        .parse()
                        .map_err(|err| ProviderInitErr::EnvVarError(
                            format!("Invalid value {} for {}: {}", base_url, LOS_ASSET_HTTP_BASE_URL, err))
                        )?,
                    prefix_map
                )
            )
        } else if base_url.is_none() && let Some(bucket) = bucket {
            let shared_config = aws_config::from_env()
                .timeout_config(
                    aws_config::timeout::TimeoutConfig::builder()
                        .read_timeout(ASSET_FETCH_TIMEOUT)
                        .build()
                )
                .load()
                .await;
            let s3_client = aws_sdk_s3::Client::new(&shared_config);

            Box::new(
                S3AssetFetcher::new(s3_client.clone(), bucket, prefix_map)
            )
        } else {
            return Err(ProviderInitErr::EnvVarError(
                format!(
                    "You must set exactly one of the {} or {} env vars",
                    LOS_ASSET_HTTP_BASE_URL,
                    LOS_ASSET_S3_BUCKET
                ))
            );
        };

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
            terrain_classification_provider: Box::new(CachingTerrainClassificationTileProvider::new(Arc::clone(
                &asset_provider,
            ))),
            footprint_provider: Box::new(CachingFootprintProvider::new(Arc::clone(&asset_provider))),
            obstruction_provider: Box::new(
                CachingObstructionProvider::new(Arc::clone(&asset_provider))
                    .await
                    .map_err(ProviderInitErr::AssetPrefetchError)?,
            ),
            point_eval_result_provider: PointEvaluationResultProvider::new(value_store),
            meshdb_provider: ProgenitorMeshDBProvider::new(expect_env(MESHDB_API_TOKEN)?),
        })
    }
}
