pub const LOS_ASSET_S3_BUCKET: &str = "LOS_ASSET_S3_BUCKET";
pub const LOCAL_ASSET_CACHE_ROOT: &str = "LOCAL_ASSET_CACHE_ROOT";
pub const LOS_ORTHOS_S3_PREFIX: &str = "LOS_ORTHOS_S3_PREFIX";
pub const LOS_TERRAIN_TILE_S3_PREFIX: &str = "LOS_TERRAIN_TILE_S3_PREFIX";
pub const LOS_OBSTRUCTION_S3_PREFIX: &str = "LOS_OBSTRUCTION_S3_PREFIX";
pub const NYC_DOB_SQLITE_DB_FILE: &str = "NYC_DOB_SQLITE_DB_FILE";

pub const MESHDB_API_TOKEN: &str = "MESHDB_API_TOKEN";

pub const REDIS_URL: &str = "REDIS_URL";

pub fn expect_env(env_var_name: &str) -> String {
    get_env(env_var_name).unwrap_or_else(|| panic!("Please set env var {}", env_var_name))
}
pub fn get_env(env_var_name: &str) -> Option<String> {
    std::env::var(env_var_name)
        .ok()
        .and_then(|value| if value.is_empty() { None } else { Some(value) })
}
