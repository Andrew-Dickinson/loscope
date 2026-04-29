

pub const LOS_ASSET_S3_BUCKET: &str = "LOS_ASSET_S3_BUCKET";
pub const LOCAL_ASSET_CACHE_ROOT: &str = "LOCAL_ASSET_CACHE_ROOT";
pub const LOS_ORTHOS_S3_PREFIX: &str = "LOS_ORTHOS_S3_PREFIX";
pub const NYC_DOB_SQLITE_DB_FILE: &str = "NYC_DOB_SQLITE_DB_FILE";

pub fn expect_env(env_var_name: &str) -> String {
    String::from(std::env::var(env_var_name).expect(&format!("Please set env var {}", env_var_name)))
}