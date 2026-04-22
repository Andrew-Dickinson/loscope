use std::num::ParseIntError;

#[derive(Debug)]
pub enum ParseErr {
    MissingSeparator,
    InvalidSubgrid,
    InvalidLASTileId,
    InvalidInt(ParseIntError),
}

impl From<ParseIntError> for ParseErr {
    fn from(e: ParseIntError) -> Self {
        ParseErr::InvalidInt(e)
    }
}