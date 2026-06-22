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
