use std::num::ParseIntError;
use strum_macros::Display;

#[derive(Debug,Display)]
pub enum TileParseErr {
    MissingSeparator,
    InvalidSubgrid,
    InvalidLASTileId,
    InvalidInt(ParseIntError),
}

#[derive(Debug)]
pub struct BINParseError(pub String);

#[derive(Debug,Display)]
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