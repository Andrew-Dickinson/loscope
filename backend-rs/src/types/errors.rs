use std::num::ParseIntError;

#[derive(Debug)]
pub enum ParseErr {
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

impl From<ParseIntError> for ParseErr {
    fn from(e: ParseIntError) -> Self {
        ParseErr::InvalidInt(e)
    }
}