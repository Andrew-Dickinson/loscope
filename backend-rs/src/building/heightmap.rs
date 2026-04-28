use derive_getters::Getters;
use derive_new::new;
use array2d::{Array2D};
use geo_types::Polygon;
use crate::types::coords::NYSCoords2;
use crate::types::errors::BINParseError;


const BIN_LENGTH_CHARS: u8 = 7;
const PERMITTED_BIN_FIRST_CHAR: &[u8] = &[1, 2, 3, 4, 5];

#[derive(Debug)]
pub struct BINId(String);

impl BINId {
    pub fn parse(bin_id: &str) -> Result<BINId, BINParseError> {
        let chars: Vec<char> = bin_id.chars().collect();

        if chars.len() != BIN_LENGTH_CHARS as usize {
            return Err(BINParseError(format!("Invalid BIN ID: {bin_id}. Expected {BIN_LENGTH_CHARS} chars")));
        }

        let Some(first_digit) = chars[0].to_digit(10) else {
            return Err(BINParseError(format!("Invalid BIN ID: {bin_id}. All characters must be digits")));
        };
        if !PERMITTED_BIN_FIRST_CHAR.contains(&(first_digit as u8)) {
            return Err(BINParseError(format!("Invalid BIN ID: {bin_id}. First character must be one of {PERMITTED_BIN_FIRST_CHAR:?}")));
        };

        for c in chars {
            if !c.is_digit(10) {
                return Err(BINParseError(format!("Invalid BIN ID: {bin_id}. All characters must be digits")));
            }
        }

        Ok(BINId(bin_id.to_string()))
    }
}

#[derive(Debug, new, Getters)]
pub struct RooftopHeightMap {
    bin_id: BINId,
    sw_offset: NYSCoords2,

    // Values are in inches above the NY SP Long Island datum,
    // axes are [easting_local, northing_local] (add sw_offset to get global position)
    // Pixels outside the mask=true footprint are set to 0
    heightmap: Array2D<u16>,

    // A mask over the dimensions of heightmap, where true, the height is valid,
    // where false, it's not
    // TODO: Is this memory efficient? Do we care?
    mask: Array2D<bool>,

    // The shape of the underlying building footprint in NY SP LI coordinates
    poly_nys: Polygon
}
