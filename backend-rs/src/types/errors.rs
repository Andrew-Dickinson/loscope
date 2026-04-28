use std::num::ParseIntError;

#[derive(Debug)]
pub enum TileParseErr {
    MissingSeparator,
    InvalidSubgrid,
    InvalidLASTileId,
    InvalidInt(ParseIntError),
}

#[derive(Debug)]
pub enum AssetErr {
    AssetNotFound(String),
    AssetDownloadError(String),
    LocalFileSystemError(String),
    UnsupportedAssetType(String),
    AssetContentError(String),
}

impl From<ParseIntError> for TileParseErr {
    fn from(e: ParseIntError) -> Self {
        TileParseErr::InvalidInt(e)
    }
}