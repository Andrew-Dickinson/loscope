use std::fmt::format;
use crate::types::errors::ProviderInitErr;

pub const LOS_ASSET_S3_BUCKET: &str = "LOS_ASSET_S3_BUCKET";
pub const LOS_ASSET_HTTP_BASE_URL: &str = "LOS_ASSET_HTTP_BASE_URL";
pub const LOCAL_ASSET_CACHE_ROOT: &str = "LOCAL_ASSET_CACHE_ROOT";
pub const LOS_ORTHOS_PREFIX: &str = "LOS_ORTHOS_PREFIX";
pub const LOS_TERRAIN_TILE_PREFIX: &str = "LOS_TERRAIN_TILE_PREFIX";
pub const LOS_OBSTRUCTION_PREFIX: &str = "LOS_OBSTRUCTION_PREFIX";
pub const LOS_FOOTPRINTS_PREFIX: &str = "LOS_FOOTPRINTS_PREFIX";

pub const MESHDB_API_TOKEN: &str = "MESHDB_API_TOKEN";

pub const REDIS_URL: &str = "REDIS_URL";

pub const LOS_DEBUG_DUMP_DIR: &str = "LOS_DEBUG_DUMP_DIR";

pub const LOS_MAX_ANALYSIS_MEMORY_BYTES: &str = "LOS_MAX_ANALYSIS_MEMORY_BYTES";
pub const LOS_OBSTRUCTION_BYTES_PER_TILE_ESTIMATE: &str = "LOS_OBSTRUCTION_BYTES_PER_TILE_ESTIMATE";
pub const LOS_MEMORY_ESTIMATE_SAFETY_FACTOR: &str = "LOS_MEMORY_ESTIMATE_SAFETY_FACTOR";

// See analysis::memory_paranoid. When set (to any non-empty value), every reservation-guarded
// endpoint tracks the real size of each non-trivial allocation it makes and panics -- after
// logging the full breakdown -- the moment the running total for a request exceeds what
// memory_budget reserved for it.
pub const LOS_MEMORY_PARANOID_MODE: &str = "LOS_MEMORY_PARANOID_MODE";

// See util::memory_profiler.
pub const LOS_MEMORY_PROFILE_PATH: &str = "LOS_MEMORY_PROFILE_PATH";
pub const LOS_MEMORY_PROFILE_INTERVAL_MS: &str = "LOS_MEMORY_PROFILE_INTERVAL_MS";

// See util::download_concurrency_profiler.
pub const LOS_DOWNLOAD_CONCURRENCY_PROFILE_PATH: &str = "LOS_DOWNLOAD_CONCURRENCY_PROFILE_PATH";
pub const LOS_DOWNLOAD_CONCURRENCY_PROFILE_INTERVAL_MS: &str = "LOS_DOWNLOAD_CONCURRENCY_PROFILE_INTERVAL_MS";

pub fn expect_env(env_var_name: &str) -> Result<String, ProviderInitErr> {
    get_env(env_var_name).ok_or(
        ProviderInitErr::EnvVarError(format!("Please set env var {}", env_var_name))
    )
}

pub fn get_env(env_var_name: &str) -> Option<String> {
    std::env::var(env_var_name)
        .ok()
        .and_then(|value| if value.is_empty() { None } else { Some(value) })
}
