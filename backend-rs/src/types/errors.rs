use rocket::http::Status;
use std::fmt::Debug;
use std::num::ParseIntError;
use redis::RedisError;
use strum_macros::Display;

#[derive(Debug)]
pub enum MeshDBError {
    ApiError(String),
    DataError(String),
}

impl<T> From<progenitor_client::Error<T>> for MeshDBError
where
    T: Debug,
{
    fn from(value: progenitor_client::Error<T>) -> Self {
        MeshDBError::ApiError(format!("Error calling MeshDB Api: {}", value))
    }
}

#[derive(Debug, Display)]
pub enum TileParseErr {
    MissingSeparator,
    InvalidSubgrid,
    InvalidLASTileId,
    InvalidInt(ParseIntError),
}

#[derive(Debug)]
pub struct BINParseError(pub String);

#[derive(Debug, Display)]
pub enum AssetErr {
    AssetNotFound(String),
    AssetDownloadError(String),
    LocalFileSystemError(String),
    UnsupportedAssetType(String),
    AssetContentError(String),
}

#[derive(Debug)]
pub enum ProviderInitErr {
    RusqliteError(tokio_rusqlite::Error),
    RedisError(RedisError),
    AssetPrefetchError(AssetErr),
}

impl From<ParseIntError> for TileParseErr {
    fn from(e: ParseIntError) -> Self {
        TileParseErr::InvalidInt(e)
    }
}
impl From<tokio_rusqlite::Error> for ProviderInitErr {
    fn from(e: tokio_rusqlite::Error) -> Self {
        ProviderInitErr::RusqliteError(e)
    }
}
impl From<AssetErr> for Status {
    fn from(err: AssetErr) -> Self {
        // println!("{err:?}");
        match err {
            AssetErr::AssetContentError(_) => Status::UnprocessableEntity,
            AssetErr::AssetNotFound(_) => Status::NotFound,
            _ => Status::InternalServerError,
        }
    }
}
